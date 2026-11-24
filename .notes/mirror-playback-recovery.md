# Mirror playback recovery

The receive path previously used the depayloader's defaults (no keyframe request or wait after
loss) and coupled decoding to conversion and rendering. The installed driver can decode H.264,
but that alone does not prove that a phone stream renders smoothly. No real-phone improvement
is claimed by this change.

`mirror/src/playback.rs` builds the visible receive chain. `rtph264depay` requests a keyframe
after loss, waits for it, and negotiates complete access units so waiting takes effect. A
two-frame downstream-leaky queue follows the decoder: rendering may discard an old picture
without losing the compressed references required for future pictures. Upstream jitter and
decoder buffers remain outside this bound. Headless notification sessions remain unchanged.

`mirror/src/main.rs` logs the decoder, sink and GL backend selected for each visible session.
Existing environment overrides are retained. Software-decoder and native-Wayland comparisons
must affect the host process, not just the one-shot client that sends it a request.

Behavioral tests send synthetic encoded RTP through the actual receive chain. One slows
rendering and checks that all input frames decode but stale pictures are shed and the newest
arrives. The other loses an RTP packet through a jitterbuffer and checks that a keyframe is
requested and decoded pictures resume. They require GStreamer's x264 encoder and libav decoder.

Keep actual phone logs outside the repository: they contain session and network identifiers.

Playback performance update: the default GL sink now receives the decoder's native format via a
passthrough instead of an unconditional CPU videoconvert; GL handles upload and color conversion.
This permits native-memory negotiation but does not guarantee DMA-BUF on every GL platform.
Custom sinks keep a CPU conversion path. The GL sink presents immediately and does not retain a last-sample reference. WebRTC jitter buffering defaults to 80 ms
(MIRROR_LATENCY_MS, 20–1000); this trades buffering latency against network-jitter tolerance.

One local synthetic comparison decoded and displayed 120 H.264 frames at 2880×1920: software
decode + CPU conversion used about 1.62 CPU seconds versus 1.06 with VA decode + native-format GL
rendering (elapsed 1.57 vs 1.40 seconds). This single capacity check is not an end-to-end latency
or subjective-smoothness measurement. Negotiated caps and actual-phone rendering counters are
checked separately. Raw benchmarks and diagnostic data remain outside the repository.

Session renderStats exposes cumulative renderer counts. GL/driver teardown runs on a worker
thread with a strong pipeline reference, after notifying the phone and releasing held input,
so cleanup does not block host requests for other sessions.

A live synthetic RTP check at 2880×1920 exposed severe drops when sink clock synchronization
was enabled with a small lateness budget. The final path keeps sync=false: 119 of 120 synthetic
frames were rendered, with zero sink-lateness drops (one frame shed before the sink). This
combined local encoder/decoder exercise is a data-flow check, not a remote frame-rate claim.

Current-phone check after the update could not complete: the phone returned service 1 error
147 (confirmation timeout), so no new real stream was available for rendering-rate measurement.
The updated hardware-default host and Chinese GUI are running; real-device smoothness remains
unverified until phone-side confirmation is granted.

After phone-side unlock/confirmation, a real 2880×1920 desktop session resumed with vah264dec,
native NV12 negotiation, glimagesink/X11 and the 80 ms jitter buffer. Two five-second update
intervals measured about 20.1 and 13.6 rendered fps with zero sink drops; subsequent static
intervals produced no new frames. No decoder errors were recorded. This confirms the updated
path is active but is not a controlled continuous-scroll or sustained-60-fps result. The phone
stream reports a variable frame rate, so idle intervals must not be counted as playback failure.
