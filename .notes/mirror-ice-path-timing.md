# Mirroring stutter: ICE path and jitter timing

## What and why

A user still felt stutter at 720x1280. Five-second render averages concealed frame stalls.
New local-only `streamTiming` and `jitterStats` diagnostics distinguish arrival timing,
jitterbuffer hold, sender timestamp regressions and RTP loss. They are included in sessions
and the existing five-second log entries, without phone polling or payload retention.

Investigation found the active peer route on a Bluetooth PAN whose interface had an Ethernet
name. Advertising only Wi-Fi candidates did not prevent Bluetooth from receiving most video
bytes: libnice still gathered on all interfaces, allowing peer-reflexive selection. This
explains why changing the advertised-address environment alone was ineffective.

## How

`mirror/src/stream_timing.rs` observes jitterbuffer sink/src buffers, storing only an RTP
sequence number and monotonic arrival time for at most 8,192 pending packets. Frame marker
packets compare sender RTP intervals (H.264/90kHz) with local arrival intervals. Sequence and
timestamp wrap are supported; out-of-order marker packets do not count as timestamp regressions.
Session maxima include idle/startup effects and do not measure end-to-end latency.

`mirror/src/ice.rs::bind_agent` resolves address/list filters, expands IPv4 CIDRs over local
addresses, and calls public libnice address allocation/add/free APIs before stream creation.
This applies both to explicit overrides and the default control `pcAddress`; unrestricted
`all` and private-LAN fallback retain automatic gathering. PAN-only operation still works
when its address is selected. There is no automatic Wi-Fi assumption that might break a
phone reachable only via Bluetooth. Actual path selection must be verified on each device.

## Validation

Regression tests cover bounded metadata, RTP/sequence wrap, backward timestamps, jitter hold,
CIDR expansion and actual gathered socket addresses on the webrtcbin-owned NiceAgent. Existing
packet-loss recovery and slow-renderer tests remain enabled. Runtime measurements and final
suite results are recorded below after deployment.

Release validation passed: 52 mirror tests, 0 failures, 1 existing display-dependent exclusion;
the release binary built without warnings. The actual-agent socket test verified that an
explicit loopback selection gathers only loopback sockets, rather than merely hiding others.

After deploying binding, the daemon-supervised host used its original environment and the
now-LAN control address (no hardcoded address override). During 40 seconds of restored 720p
app operation, LAN RX increased about 10.3 MB and PAN RX about 3 KB. In the last three logged
five-second active intervals, rendering advanced 171/154/171 frames (about 31–34 fps), with
nearly identical decoded counts. RTP loss, lateness and sender timestamp regressions remained
zero; no new skew/decreasing-timestamp warnings were logged during that observation. Startup
jitterbuffer hold reached 158 ms for four packets; the count did not grow afterward. These
observations verify path selection and short-term improvement, not a constant-frame-rate or
end-to-end latency guarantee. The original daemon stayed running and the app was restored.
