# Video stalls after interaction

The reported symptom was a still app picture with ineffective clicks. Live checks showed the
control socket and phone heartbeat remained responsive, key events reached the daemon, and a
safe scroll later produced decoded/rendered updates. The original cause is not established;
static streams can legitimately stop producing frames. Do not claim the original stall was
proved to be packet loss.

`playback::Progress` tracks metadata only: received RTP packets, decoded frames before the
leaky render queue, and meaningful navigation input (not pointer motion). Session snapshots
expose counts and ages alongside existing renderer statistics. No input coordinates, key
contents or video buffers are retained for diagnostics.

`Session::recover_video` requests an upstream H.264 keyframe after two seconds of packets with
no decoded output, or recent meaningful input without a new decoded frame. Feedback goes to
the WebRTC video pad, following the same upstream route as depayloader requests. Requests are
at least two seconds apart and capped at three until decoding resumes. Input-based requests
expire after eight seconds; a static idle display produces none. This preserves session/display
identity and avoids a restart loop. No automatic app relaunch or input replay is performed.

Validation: 39 mirror tests passed (one desktop-only test excluded). The new media regression
loses an RTP packet and suppresses initial keyframe feedback with a very long encoder keyframe
interval, then verifies the watchdog retry restores decoded video. Timing tests check static
idle, active-packet stalls, throttling, retry limits and reset after decoding resumes. Actual
phone recovery from the original intermittent freeze remains unverified.

Deployment check: the rebuilt host reopened the existing app session without restarting the
daemon or GUI. A wheel event delivered through the X11 video window was observed by the
input counter; the two-second interval produced 19 additional decoded and rendered frames.
Recovery requests remained zero during this responsive interval. Concurrent user input may
contribute to counters; this validates wiring, not a controlled latency measurement or a
reproduction of the original freeze.
