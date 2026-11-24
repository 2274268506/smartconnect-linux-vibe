# UDP bursts interrupt high-resolution desktop video

A live desktop at the PC's native resolution showed incoming RTP and repeated multi-second
stalls in decoded/rendered output, even while input counts increased. The host's kernel UDP
drop counter increased from roughly 1,700 to over 3,000 packets during observation; the receive
buffer was only 212,992 bytes. This is evidence of local receive loss, not a renderer-lateness
failure. It does not exclude additional Wi-Fi jitter or phone-side delays.

`receive_buffer::ReceiveBuffers` discovers `nicesrc` elements, takes their NiceAgent plus stream
and component IDs, and calls the public `nice_agent_get_sockets` API. For datagram GSocket objects
it requests SO_RCVBUF=4 MiB. Owned socket references are local to each update and are released
correctly with the libnice array. Socket object identity and observed buffer size deduplicate
updates; a reused identity with a different buffer is tuned again. Existing larger buffers are
preserved. TCP sockets are skipped. Kernel limits still apply; no sysctls or socket FDs belonging
to other processes are modified.

The host periodically checks for newly gathered sockets. `udpReceiveBuffers` reports effective
kernel sizes, with Linux's doubled accounting; logs include sizes and failures without addresses.
This avoids requiring root and preserves the user's resolution and window preferences.

Validation: 41 mirror tests passed, one pre-existing desktop-only test excluded. New tests
create libnice sockets through a real NiceAgent/nicesrc pair and verify repeated tuning is
stable, and check effective SO_RCVBUF against the kernel cap. Existing packet-loss recovery
media tests still pass. Live validation after deployment is recorded below.

Live deployment: restarted only the mirror host through its existing supervisor and reopened
the desktop with the previous PC-native dimensions and borderless setting. The control daemon
and GUI stayed running. The two media UDP sockets now report 8,388,608 effective bytes (Linux
accounting). Both kernel drop counters stayed zero. During a 21-second observation with user
input, rendered frames increased from 425 to 772 and decoded frames from 428 to 776, with no
watchdog keyframe requests. A later check reached 1,066 rendered frames and 660 input events,
still with zero kernel drops and zero recovery requests. These observations support the burst
buffer fix but are not a controlled latency/throughput benchmark or a guarantee against every
phone/network/driver stall. No reduction in desktop resolution was needed.
