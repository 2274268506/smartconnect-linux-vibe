# SMS actions and ADB rediscovery

## What and why

Users could not distinguish unread state easily, had no bulk action or conversation Close button, and stale ADB endpoints survived Wi-Fi changes. Phone-side marking also failed despite read permission.

## How

Use solid theme-accent unread badges and bold conversation titles. Add `sms_mark_all_read` to snapshot only incoming unread SMS/MMS and queue existing kind-aware commands, with no optimistic cache edits. Show results from bounded cache reads. The GUI falls back to existing per-message commands when an older running daemon lacks the bulk command; other command errors do not trigger fallback actions. Add a dialog header with Close.

Read-only phone diagnostics found the official app’s WRITE_SMS operation ignored while READ_SMS was granted. Native raw-instruction inspection confirms cmd 8 processes valid data and updates the SMS provider; earlier structured decompilation inverted the null guard. Enable WRITE_SMS for the current user through authorized ADB on this device, leaving the default SMS role unchanged. Document opt-in setup and restoration for other users. No real unread messages are used as test fixtures.

ADB scans read the connected paired Smart Connect peer, validate a unicast non-loopback IP, and try authorized ADB using the current port (default 5555). Follow peer changes only when the page is mapped, defer rediscovery during jobs, and never replay a file transfer. Preserve explicit address edits and device choices; replace old automatic endpoints after network changes. No subnet scan or ADB server restart.

## Validation

Synthetic tests cover bulk target filtering and distinct SMS/MMS IDs, paired address changes and IPv6/invalid peers, plus isolated GTK unread styling, bulk sensitivity and Close behavior. Run locked workspace release build/tests and isolated GUI checks. No private diagnostics, addresses, message bodies, identifiers or phone APKs enter the repository.

Verified: final locked workspace release build passed without warnings; 331 tests passed, 0 failed, 38 environment-dependent tests skipped. Both isolated GTK Messages and ADB page regressions passed. The first GTK attempt used an expired desktop Xauthority path; rerunning with the live session authority passed. A native mark request against an already-read SMS recorded an allowed WRITE_SMS access and left unread totals unchanged. The current paired peer address matches an authorized ADB device. Real unread bulk marking and a physical Wi-Fi handover were not used as test fixtures.
