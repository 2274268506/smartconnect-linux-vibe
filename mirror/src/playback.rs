//! Recover at a clean H.264 frame after loss, and keep slow rendering from accumulating frames.

use std::time::{Duration, Instant};

/// Metadata only: never retain video buffers, input coordinates or key contents.
#[derive(Default)]
pub struct Progress {
    pub packets: u64,
    pub decoded: u64,
    pub inputs: u64,
    packet_at: Option<Instant>,
    decoded_at: Option<Instant>,
    input_at: Option<Instant>,
    stalled_since: Option<Instant>,
    request_at: Option<Instant>,
    pub requests: u64,
    attempts: u32,
}

impl Progress {
    pub fn packet(&mut self, now: Instant) {
        self.packets += 1;
        self.packet_at = Some(now);
        self.stalled_since.get_or_insert(now);
    }

    pub fn decoded(&mut self, now: Instant) {
        self.decoded += 1;
        self.decoded_at = Some(now);
        self.stalled_since = None;
        self.attempts = 0;
    }

    pub fn input(&mut self, now: Instant) {
        self.inputs += 1;
        self.input_at = Some(now);
        self.stalled_since.get_or_insert(now);
    }

    /// Retry feedback lost along with video, but never restart a healthy static display.
    pub fn request_due(&mut self, now: Instant) -> bool {
        let age = |at: Instant| now.saturating_duration_since(at);
        let receiving = self
            .packet_at
            .is_some_and(|at| age(at) < Duration::from_secs(1));
        let awaiting_input = self.input_at.is_some_and(|at| {
            age(at) < Duration::from_secs(8) && self.decoded_at.is_none_or(|frame| at > frame)
        });
        if !(receiving || awaiting_input)
            || self
                .stalled_since
                .is_none_or(|at| age(at) < Duration::from_secs(2))
            || self
                .request_at
                .is_some_and(|at| age(at) < Duration::from_secs(2))
            || self.attempts >= 3
        {
            return false;
        }
        self.request_at = Some(now);
        self.attempts += 1;
        self.requests += 1;
        true
    }

    pub fn snapshot(&self, now: Instant) -> serde_json::Value {
        let age =
            |at: Option<Instant>| at.map(|at| now.saturating_duration_since(at).as_millis() as u64);
        serde_json::json!({
            "rtpPackets": self.packets, "decodedFrames": self.decoded,
            "inputEvents": self.inputs, "keyframeRequests": self.requests,
            "lastPacketAgeMs": age(self.packet_at), "lastDecodedAgeMs": age(self.decoded_at),
            "lastInputAgeMs": age(self.input_at),
        })
    }
}

pub fn request_keyframe(pad: &gst::Pad) -> bool {
    use gst::prelude::*;
    pad.send_event(
        gst_video::UpstreamForceKeyUnitEvent::builder()
            .all_headers(true)
            .build(),
    )
}

pub fn description(decoder: &str, sink: &str) -> String {
    // AU output is required for wait-for-keyframe to suppress damaged inter frames after loss.
    // Never leak compressed frames: dropping them would break later frames' references. Only
    // discard already decoded frames, which the decoder no longer needs to reconstruct video.
    // glimagesink already uploads and converts on the GPU. Do not force CPU conversion or
    // system-memory negotiation before it; the decoder and sink can select a native format.
    let gl = sink.split_whitespace().next() == Some("glimagesink");
    let convert = if gl {
        "glupload ! glcolorconvert name=conv ! glvideoflip name=flip"
    } else {
        "videoconvert name=conv n-threads=0 ! videoflip name=flip"
    };
    let timing = if gl {
        "sync=false enable-last-sample=false"
    } else {
        "sync=false"
    };
    format!(
        "queue ! rtph264depay request-keyframe=true wait-for-keyframe=true \
         ! video/x-h264,alignment=au ! h264parse ! {decoder} \
         ! queue name=render_queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream \
         ! {convert} ! taginject name=tag ! {sink} name=sink {timing}"
    )
}

pub const DEFAULT_LATENCY_MS: u32 = 80;

pub fn parse_latency(value: &str) -> Option<u32> {
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|value| (20..=1000).contains(value))
}

pub fn latency_ms() -> u32 {
    match std::env::var("MIRROR_LATENCY_MS") {
        Ok(value) => parse_latency(&value).unwrap_or_else(|| {
            eprintln!("invalid MIRROR_LATENCY_MS; use 20–1000 ms; using {DEFAULT_LATENCY_MS} ms");
            DEFAULT_LATENCY_MS
        }),
        Err(_) => DEFAULT_LATENCY_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gst::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    #[ignore="requires a desktop OpenGL context; use GST_GL_WINDOW=x11"]
    fn gpu_rotation_negotiates_upright_dimensions_for_every_quarter_turn() {
        gst::init().unwrap();
        for (direction,size) in [("identity",(160,120)),("90l",(120,160)),("180",(160,120)),("90r",(120,160))] {
            let desc=format!("videotestsrc num-buffers=1 ! video/x-raw,width=160,height=120 ! glupload ! glcolorconvert ! glvideoflip name=flip video-direction={direction} ! fakesink sync=false");
            let pipeline=gst::parse::launch(&desc).unwrap().downcast::<gst::Pipeline>().unwrap();
            pipeline.set_state(gst::State::Playing).unwrap();
            let message=pipeline.bus().unwrap().timed_pop_filtered(gst::ClockTime::from_seconds(5),&[gst::MessageType::Eos,gst::MessageType::Error]).expect("GL rotation completed");
            if let gst::MessageView::Error(error)=message.view(){panic!("GPU rotation failed: {} {:?}",error.error(),error.debug());}
            let caps=pipeline.by_name("flip").unwrap().static_pad("src").unwrap().current_caps().unwrap();
            let fields=caps.structure(0).unwrap();
            assert_eq!((fields.get::<i32>("width").unwrap(),fields.get::<i32>("height").unwrap()),size);
            pipeline.set_state(gst::State::Null).unwrap();
        }
    }

    #[test]
    fn static_video_does_not_request_keyframes_but_input_stalls_retry_with_a_limit() {
        let start = Instant::now();
        let mut progress = Progress::default();
        progress.packet(start);
        progress.decoded(start);
        assert!(!progress.request_due(start + Duration::from_secs(30)));
        progress.input(start + Duration::from_secs(31));
        assert!(!progress.request_due(start + Duration::from_secs(32)));
        assert!(progress.request_due(start + Duration::from_secs(33)));
        assert!(!progress.request_due(start + Duration::from_secs(34)));
        assert!(progress.request_due(start + Duration::from_secs(35)));
        assert!(progress.request_due(start + Duration::from_secs(37)));
        // More clicks do not bypass the retry limit; a decoded frame resets it.
        progress.input(start + Duration::from_secs(38));
        assert!(!progress.request_due(start + Duration::from_secs(39)));
        progress.decoded(start + Duration::from_secs(40));
        progress.input(start + Duration::from_secs(41));
        assert!(progress.request_due(start + Duration::from_secs(43)));
    }

    #[test]
    fn continued_packets_without_decoded_frames_trigger_recovery() {
        let start = Instant::now();
        let mut progress = Progress::default();
        progress.packet(start);
        progress.packet(start + Duration::from_secs(2));
        assert!(progress.request_due(start + Duration::from_secs(2)));
        progress.decoded(start + Duration::from_secs(3));
        assert!(!progress.request_due(start + Duration::from_secs(4)));
    }

    #[test]
    fn lost_keyframe_feedback_is_retried_and_video_resumes() {
        gst::init().unwrap();
        let desc = format!(
            "videotestsrc is-live=true num-buffers=300 pattern=ball \
             ! video/x-raw,width=160,height=120,framerate=60/1 \
             ! x264enc tune=zerolatency key-int-max=60000 ! rtph264pay mtu=300 \
             ! identity name=network ! rtpjitterbuffer latency=20 do-lost=true ! {}",
            description("avdec_h264", "fakesink")
        );
        let pipeline = gst::parse::launch(&desc)
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        let progress = Arc::new(Mutex::new(Progress::default()));
        let packets = Arc::new(AtomicUsize::new(0));
        let dropped_feedback = Arc::new(AtomicUsize::new(0));
        let state = progress.clone();
        let count = packets.clone();
        let feedback = dropped_feedback.clone();
        let network = pipeline
            .by_name("network")
            .unwrap()
            .static_pad("src")
            .unwrap();
        network.add_probe(
            gst::PadProbeType::BUFFER | gst::PadProbeType::EVENT_UPSTREAM,
            move |_, info| {
                match &info.data {
                    Some(gst::PadProbeData::Buffer(_)) => {
                        state.lock().unwrap().packet(Instant::now());
                        if count.fetch_add(1, Ordering::SeqCst) == 100 {
                            return gst::PadProbeReturn::Drop;
                        }
                    }
                    Some(gst::PadProbeData::Event(event))
                        if event
                            .structure()
                            .is_some_and(|s| s.name() == "GstForceKeyUnit")
                            && state.lock().unwrap().requests == 0 =>
                    {
                        feedback.fetch_add(1, Ordering::SeqCst);
                        return gst::PadProbeReturn::Drop;
                    }
                    _ => {}
                }
                gst::PadProbeReturn::Ok
            },
        );
        let state = progress.clone();
        pipeline
            .by_name("render_queue")
            .unwrap()
            .static_pad("sink")
            .unwrap()
            .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                state.lock().unwrap().decoded(Instant::now());
                gst::PadProbeReturn::Ok
            });
        pipeline.set_state(gst::State::Playing).unwrap();
        let bus = pipeline.bus().unwrap();
        let deadline = Instant::now() + Duration::from_secs(9);
        let mut frames_at_retry = None;
        let result = loop {
            let now = Instant::now();
            let due = progress.lock().unwrap().request_due(now);
            if due {
                frames_at_retry.get_or_insert(progress.lock().unwrap().decoded);
                request_keyframe(&network);
            }
            if let Some(message) = bus.timed_pop_filtered(
                gst::ClockTime::from_mseconds(20),
                &[gst::MessageType::Eos, gst::MessageType::Error],
            ) {
                break Some(message);
            }
            if now >= deadline {
                break None;
            }
        };
        pipeline.set_state(gst::State::Null).unwrap();
        let result = result.expect("test video did not complete");
        assert!(
            matches!(result.view(), gst::MessageView::Eos(_)),
            "{result:?}"
        );
        assert!(
            dropped_feedback.load(Ordering::SeqCst) > 0,
            "must lose initial keyframe feedback"
        );
        let before = frames_at_retry.expect("watchdog must retry lost feedback");
        assert!(
            progress.lock().unwrap().decoded > before + 10,
            "video did not resume after retry"
        );
    }

    #[test]
    fn jitter_buffer_settings_reject_invalid_and_excessive_values() {
        assert_eq!(parse_latency("80"), Some(80));
        assert_eq!(parse_latency(" 200 "), Some(200));
        for value in ["", "0", "-1", "19", "1001", "NaN"] {
            assert_eq!(parse_latency(value), None);
        }
    }

    fn finish(pipeline: &gst::Pipeline) {
        pipeline.set_state(gst::State::Playing).unwrap();
        let message = pipeline.bus().unwrap().timed_pop_filtered(
            gst::ClockTime::from_seconds(10),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        pipeline.set_state(gst::State::Null).unwrap();
        let message = message.expect("playback did not finish within 10 seconds");
        if let gst::MessageView::Error(error) = message.view() {
            panic!("playback failed: {} ({:?})", error.error(), error.debug());
        }
    }

    #[test]
    fn slow_renderer_drops_stale_frames_but_presents_the_latest() {
        gst::init().unwrap();
        let desc = format!(
            "videotestsrc num-buffers=90 ! video/x-raw,width=160,height=120,framerate=60/1 \
             ! x264enc tune=zerolatency key-int-max=30 ! rtph264pay ! {}",
            description(
                "avdec_h264",
                "identity sleep-time=20000 ! fakesink signal-handoffs=true"
            )
        );
        let pipeline = gst::parse::launch(&desc)
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        let decoded = Arc::new(Mutex::new(Vec::new()));
        let rendered = Arc::new(Mutex::new(Vec::new()));
        let received = decoded.clone();
        pipeline
            .by_name("render_queue")
            .unwrap()
            .static_pad("sink")
            .unwrap()
            .add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                if let Some(gst::PadProbeData::Buffer(buffer)) = &info.data {
                    received.lock().unwrap().push(buffer.pts());
                }
                gst::PadProbeReturn::Ok
            });
        let presented = rendered.clone();
        pipeline
            .by_name("sink")
            .unwrap()
            .connect("handoff", false, move |args| {
                let buffer = args[1].get::<gst::Buffer>().unwrap();
                presented.lock().unwrap().push(buffer.pts());
                None
            });
        finish(&pipeline);
        let decoded = decoded.lock().unwrap();
        let rendered = rendered.lock().unwrap();
        assert_eq!(
            decoded.len(),
            90,
            "compressed frames must all reach the decoder"
        );
        assert!(!rendered.is_empty());
        assert!(
            rendered.len() < decoded.len(),
            "a slow renderer must shed stale frames"
        );
        assert_eq!(
            rendered.last(),
            decoded.last(),
            "the newest frame must reach the screen"
        );
    }

    #[test]
    fn packet_loss_requests_a_keyframe_and_decoding_resumes() {
        gst::init().unwrap();
        let desc = format!(
            "videotestsrc is-live=true num-buffers=120 pattern=ball \
             ! video/x-raw,width=160,height=120,framerate=60/1 \
             ! x264enc tune=zerolatency key-int-max=30 ! rtph264pay mtu=300 \
             ! identity name=network ! rtpjitterbuffer latency=20 do-lost=true ! {}",
            description("avdec_h264", "fakesink signal-handoffs=true")
        );
        let pipeline = gst::parse::launch(&desc)
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();
        let packets = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let frames_after_loss = Arc::new(AtomicUsize::new(0));
        let count = packets.clone();
        let keyframes = requests.clone();
        pipeline
            .by_name("network")
            .unwrap()
            .static_pad("src")
            .unwrap()
            .add_probe(
                gst::PadProbeType::BUFFER | gst::PadProbeType::EVENT_UPSTREAM,
                move |_, info| {
                    match &info.data {
                        Some(gst::PadProbeData::Buffer(_))
                            if count.fetch_add(1, Ordering::SeqCst) == 100 =>
                        {
                            return gst::PadProbeReturn::Drop;
                        }
                        Some(gst::PadProbeData::Event(event)) => {
                            if count.load(Ordering::SeqCst) > 100
                                && event
                                    .structure()
                                    .is_some_and(|s| s.name() == "GstForceKeyUnit")
                            {
                                keyframes.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                        _ => {}
                    }
                    gst::PadProbeReturn::Ok
                },
            );
        let count = packets.clone();
        let recovered = frames_after_loss.clone();
        pipeline
            .by_name("sink")
            .unwrap()
            .connect("handoff", false, move |_| {
                if count.load(Ordering::SeqCst) > 101 {
                    recovered.fetch_add(1, Ordering::SeqCst);
                }
                None
            });
        finish(&pipeline);
        assert!(
            packets.load(Ordering::SeqCst) > 101,
            "the test must actually lose a packet"
        );
        assert!(
            requests.load(Ordering::SeqCst) > 0,
            "loss must request a clean keyframe"
        );
        assert!(
            frames_after_loss.load(Ordering::SeqCst) > 0,
            "video must resume after packet loss"
        );
    }
}
