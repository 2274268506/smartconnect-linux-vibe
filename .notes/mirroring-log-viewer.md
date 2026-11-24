# Persistent mirroring diagnostics and viewer

Previous helper logs were split across runtime and GUI-cache locations and lacked a viewer.
The stream host now records useful metadata automatically, with a persistent XDG state path,
2 MiB files and two backups. This is separate from legacy console capture and raw session logs.

`mirror/src/diagnostics.rs` accepts Rust events and GStreamer warnings/errors through a bounded
nonblocking channel. Disk writes occur on a worker; queue pressure drops entries with a later
warning. Graceful exit attempts a bounded flush. Existing JSON CLI commands remain before
writer initialization; new --logs/--log-path commands work offline. Local five-second media
statistics include receive/decode/render progress and density without adding phone polling.

The shared storage module centralizes private file creation, cross-process flock coordination,
rotation, bounded tail reads, redaction and search/severity filtering. Raw transport caps,
unknown service payloads, changed window titles and typed unmapped keys are no longer printed.
Known credential/device fields and network addresses are removed from dedicated logs. Address
validation preserves timestamps and codec colorimetry values that merely resemble IPv6.

`gui/src/logs.rs` provides a singleton nonmodal window from the device menu and Desktop page.
It asynchronously refreshes a bounded local tail, supports search, warning/error filtering,
follow/pause, clipboard copy, folder opening and filtered retained-history export. Pending reads
are ignored while paused, and closing releases the timer/state; reopening creates a fresh view.
The shared read path re-sanitizes export data. Newly created exports use private permissions.

Tests cover privacy, rotation/order/permissions and filtering. The isolated desktop regression
checks live display, timestamps, search, pause/resume, actual filtered-file export and window
close/reopen. CLI failure-path and live-host validation are recorded below after deployment.

Validation: 41 GUI and 45 mirror unit/media tests passed, plus the isolated GTK viewer test.
The latter also wrote a filtered export and checked mode 0600. A separate isolated CLI run
with no daemon recorded its startup failure, flushed on exit, and supported offline --logs;
--log-path did not start recording. --sessions remained valid JSON.

Deployment updated the mirror host and GTK app while keeping the daemon running. One active
app stream was restored; its native GStreamer warnings, five-second media statistics and later
window closure appeared in the persistent log. The log was readable with UTC timestamps,
mode 0600 inside a 0700 directory, and an audit found no ICE credential fields or private IPv4
addresses. Desktop notification inhibition remained as configured. No raw live logs are copied
into this note. Existing runtime/cache console captures retain their separate policies.
