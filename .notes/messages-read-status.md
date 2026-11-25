# Messages read status

## What and why

The daemon already exposes message `read` flags and thread unread counts. The UI displayed only nonzero counts, leaving fully read threads and individual messages without visible states. Show explicit Read/Unread thread suffixes and incoming bubble labels. Mark outgoing messages Sent with a tooltip explaining recipient read receipts are unavailable.

## How

`gui/src/messages.rs` keeps existing opening-time mark-read behavior, retaining SMS/MMS kind. After queuing requests it reads only the local daemon cache, up to nine reads over two seconds, and updates labels when phone changes arrive. Request errors are shown; an unconfirmed request retains the phone-reported flags and displays a refresh prompt. Closed dialogs and disconnected generations cannot initiate late read marks or update widgets. No new background or phone polling, storage or private logs. Add English keys and Chinese, Spanish and French translations.

## Validation

Synthetic regressions cover incoming read/unread versus outgoing state, matching SMS/MMS identities, delayed confirmation, bounded unconfirmed requests and command failure. The isolated GTK regression checks list status suffixes and bubble-state updates without phone I/O. Run locked workspace release build/tests; no real SMS is sent or marked as part of validation.

Verified: locked workspace release build passed without compiler warnings; locked workspace release tests passed 328 tests (0 failed, 38 environment-dependent tests ignored). The isolated GTK Messages regression passed separately with private XDG directories. No real message state was changed.
