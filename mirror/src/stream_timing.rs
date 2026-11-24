//! Bounded metadata for locating stalls before and inside the RTP jitterbuffer.
use gst::prelude::*;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Default)]
pub struct Timing {
    received: u64,
    released: u64,
    pending: VecDeque<(u16, Instant)>,
    last_frame: Option<(u16, u32, Instant)>,
    timestamp_backwards: u64,
    max_clock_difference_ms: u64,
    max_hold_ms: u64,
    over_100ms: u64,
}
impl Timing {
    fn received(&mut self, sequence: u16, timestamp: u32, marker: bool, now: Instant) {
        self.received += 1;
        if self.pending.len() >= 8192 {
            self.pending.pop_front();
        }
        self.pending.push_back((sequence, now));
        if !marker {
            return;
        }
        if let Some((seq, ts, at)) = self.last_frame {
            // Ignore duplicate/reordered packets and allow normal sequence/timestamp wrap.
            if sequence.wrapping_sub(seq) as i16 <= 0 {
                return;
            }
            let delta = timestamp.wrapping_sub(ts) as i32;
            if delta < 0 {
                self.timestamp_backwards += 1;
            }
            let wall_ms = now.saturating_duration_since(at).as_millis() as i64;
            let rtp_ms = i64::from(delta) / 90; // negotiated H.264 RTP clock: 90 kHz
            self.max_clock_difference_ms = self
                .max_clock_difference_ms
                .max((wall_ms - rtp_ms).unsigned_abs());
        }
        self.last_frame = Some((sequence, timestamp, now));
    }
    fn released(&mut self, sequence: u16, now: Instant) {
        self.released += 1;
        if let Some(index) = self.pending.iter().position(|(seq, _)| *seq == sequence) {
            let (_, at) = self.pending.remove(index).unwrap();
            let ms = now.saturating_duration_since(at).as_millis() as u64;
            self.max_hold_ms = self.max_hold_ms.max(ms);
            if ms > 100 {
                self.over_100ms += 1;
            }
        }
    }
    pub fn snapshot(&self) -> Value {
        json!({"receivedPackets":self.received,"releasedPackets":self.released,
            "rtpTimestampRegressions":self.timestamp_backwards,
            "maxArrivalVsRtpDeltaMs":self.max_clock_difference_ms,
            "maxJitterHoldMs":self.max_hold_ms,"packetsHeldOver100Ms":self.over_100ms})
    }
}

fn header(buffer: &gst::BufferRef) -> Option<(u16, u32, bool)> {
    let map = buffer.map_readable().ok()?;
    let b = map.as_slice();
    if b.len() < 12 || b[0] >> 6 != 2 {
        return None;
    }
    Some((
        u16::from_be_bytes([b[2], b[3]]),
        u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
        b[1] & 0x80 != 0,
    ))
}

pub fn observe(pipeline: &gst::Pipeline, timing: Arc<Mutex<Timing>>) {
    pipeline.connect_deep_element_added(move |_, _, element| {
        if element
            .factory()
            .is_none_or(|f| f.name() != "rtpjitterbuffer")
        {
            return;
        }
        for (name, ingress) in [("sink", true), ("src", false)] {
            let timing = timing.clone();
            if let Some(pad) = element.static_pad(name) {
                pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                    if let Some(gst::PadProbeData::Buffer(buffer)) = &info.data {
                        if let Some((seq, ts, marker)) = header(buffer) {
                            let now = Instant::now();
                            let mut timing = timing.lock().unwrap();
                            if ingress {
                                timing.received(seq, ts, marker, now);
                            } else {
                                timing.released(seq, now);
                            }
                        }
                    }
                    gst::PadProbeReturn::Ok
                });
            }
        }
    });
}

pub fn jitter_stats(pipeline: &gst::Pipeline) -> Value {
    Value::Array(pipeline.iterate_recurse().into_iter().filter_map(Result::ok)
        .filter(|e|e.factory().is_some_and(|f|f.name()=="rtpjitterbuffer"))
        .map(|e| {
            let s=e.property::<gst::Structure>("stats");
            json!({"pushed":s.get::<u64>("num-pushed").ok(),"lost":s.get::<u64>("num-lost").ok(),
                "late":s.get::<u64>("num-late").ok(),"duplicates":s.get::<u64>("num-duplicates").ok(),
                "avgJitterNs":s.get::<u64>("avg-jitter").ok(),"rtxCount":s.get::<u64>("rtx-count").ok()})
        }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn distinguishes_network_timing_from_jitter_hold_and_handles_wrap() {
        let now = Instant::now();
        let mut t = Timing::default();
        t.received(65535, u32::MAX - 89, true, now);
        t.released(65535, now + Duration::from_millis(150));
        t.received(0, 0, true, now + Duration::from_millis(1));
        t.received(1, 9000, true, now + Duration::from_millis(101));
        t.received(2, 4500, true, now + Duration::from_millis(151));
        assert_eq!(t.timestamp_backwards, 1);
        assert_eq!(t.max_hold_ms, 150);
        assert_eq!(t.over_100ms, 1);
        assert_eq!(t.max_clock_difference_ms, 100);
        for n in 0..9000 {
            t.received(n, 0, false, now);
        }
        assert_eq!(t.pending.len(), 8192);
    }
}
