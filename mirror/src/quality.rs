//! Receive limits: SDP bitrate negotiation, plus a decoded-frame display-rate fallback.
use crate::stream_settings::Quality;
use std::time::{Duration, Instant};

pub fn answer_sdp(text: &str, quality: Quality) -> String {
    if !quality.custom() {
        return text.to_owned();
    }
    let mut video = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with("m=") {
            video = line.starts_with("m=video ");
        }
        if video
            && ((quality.bitrate_kbps.is_some()
                && (line.starts_with("b=AS:") || line.starts_with("b=TIAS:")))
                || (quality.fps.is_some() && line.starts_with("a=framerate:")))
        {
            continue;
        }
        let mut line = line.to_owned();
        if video && line.starts_with("a=fmtp:") {
            if let Some(b) = quality.bitrate_kbps {
                line.push_str(&format!(";x-google-max-bitrate={b}"));
            }
            if let Some(f) = quality.fps {
                line.push_str(&format!(";max-fr={f}"));
            }
        }
        out.push(line.clone());
        if video && line.starts_with("c=") {
            if let Some(b) = quality.bitrate_kbps {
                out.push(format!("b=AS:{b}"));
                out.push(format!("b=TIAS:{}", b * 1000));
            }
            if let Some(f) = quality.fps {
                out.push(format!("a=framerate:{f}"));
            }
        }
    }
    out.join("\r\n") + "\r\n"
}
#[derive(Default)]
pub struct FrameLimiter {
    next: Option<Instant>,
    pub dropped: u64,
}
impl FrameLimiter {
    pub fn allow(&mut self, now: Instant, fps: Option<u32>) -> bool {
        let Some(fps) = fps else {
            return true;
        };
        let interval = Duration::from_secs_f64(1.0 / f64::from(fps.max(1)));
        let slack = Duration::from_micros(500);
        if self.next.is_some_and(|next| now + slack < next) {
            self.dropped += 1;
            return false;
        }
        self.next = Some(match self.next {
            Some(next) if now.saturating_duration_since(next) < interval => next + interval,
            _ => now + interval,
        });
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quality_is_video_scoped_and_auto_preserves_sdp() {
        let s = "v=0\r\nm=video 9 RTP/AVP 96\r\nc=IN IP4 0.0.0.0\r\na=fmtp:96 packetization-mode=1\r\nm=audio 9 RTP/AVP 111\r\nc=IN IP4 0.0.0.0\r\na=fmtp:111 minptime=10\r\n";
        assert_eq!(answer_sdp(s, Quality::default()), s);
        let out = answer_sdp(
            s,
            Quality {
                bitrate_kbps: Some(2000),
                fps: Some(15),
            },
        );
        let (video, audio) = out.split_once("m=audio").unwrap();
        assert!(video.contains("b=TIAS:2000000"));
        assert!(video.contains("max-fr=15"));
        assert!(!audio.contains("bitrate"));
    }
    #[test]
    fn decoded_frame_limiter_caps_without_accumulating_delayed_frames() {
        let mut limiter = FrameLimiter::default();
        let start = Instant::now();
        let mut n = 0;
        for i in 0..60 {
            if limiter.allow(start + Duration::from_micros(i * 16667), Some(15)) {
                n += 1;
            }
        }
        assert_eq!(n, 15);
        assert_eq!(limiter.dropped, 45);
        assert!(limiter.allow(start + Duration::from_secs(20), Some(15)));
    }
}
