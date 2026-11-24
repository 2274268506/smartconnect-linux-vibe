# Unread metadata refresh (historical polling experiment; removed)

Current behavior: the user requested removal of the 15-second query. The timer and dedicated
request helper/test have been removed; the collector now only consumes phone events. Explicit
SMS commands remain unchanged, and the zero-count subscriber regression is retained. A phone
that does not push local read changes can therefore leave a stale count. Application
notification sharing is a separate push channel; enabling it does not prove SMS read pushes.

The following describes the earlier implementation and its validation, not current behavior.

The Phone page retained two unread SMS/MMS after the messages were read on the phone. The
live daemon still had unread=2, so this was upstream of GTK rendering. A metadata-only request
on service 204 cmd 1016 produced service 100 cmd 1018 with unreadCnt=0, enabled=false and
granted=true, and the existing status subscription updated the desktop count immediately.
No message bodies, contacts or sender addresses were fetched.

The earlier SMS collector also ran a 15-second Tokio interval with missed ticks skipped. It sent
`request_status()` only while the daemon was connected. Existing unsolicited updates continued to flow
through StatusHub; the periodic query covers phones that do not push local read changes while
Messages is disabled. It does not invoke pc_ready/request_sync or change phone read state.
No changes to the control wire format or GUI are needed. Latency is up to the next interval
plus the phone's response time; this is not an instantaneous read receipt.

Regression coverage checks the metadata request envelope and that a zero unread update is
published to status subscribers while app notifications remain intact. Real-device status-only
refresh was verified with Messages disabled for the PC.

Validation: `cargo test --release -p smartconnect-linux --lib --locked` passed 50 tests;
one pre-existing desktop clipboard integration test was excluded.

Deployment: the user service now runs the rebuilt workspace daemon through a local systemd
override, preserving its previous command options. The GUI remains running. After the restart,
Wi-Fi dial-back was refused and Bluetooth wake did not complete; the phone must reopen Smart
Connect to reconnect. Continuous automatic 15-second replies on the deployed service have not
yet been verified. Pairing is retained. The earlier live zero-count reply was verified before
this service restart; do not confuse it with a post-deployment polling test.

Follow-up: the phone reconnected, and the deployed daemon was observed receiving successive
service 100 cmd 1018 replies at 15-second intervals. The current unread count is zero.

Removal validation: 49 daemon library tests passed, including phone-pushed zero-count updates;
one pre-existing clipboard desktop integration test was excluded. The daemon was rebuilt
without the periodic query. Post-deployment traffic checks are recorded separately below.

Post-removal deployment: the rebuilt daemon reconnected successfully. Across 35 seconds of
continuous connection, the service journal contained zero outgoing service 204 cmd 1016
queries. KDE notification inhibition remained enabled as requested. No active video sessions
needed restoration during this restart.
