# Home unread SMS consistency

## Evidence and cause

The home badge uses service-100 unreadCnt; conversations derive unread state from service-204 message beans. Read-only diagnostics showed a home count of one, zero unread cached conversations, and zero unread SMS/MMS provider rows. A single service-204 cmd 1016 query returned zero. Phone-side reads do not always push this status promptly.

## Change

`messages.rs` sends one metadata request on each connection edge, explicit open/refresh and after a read attempt completes. Local cache reads do not query the phone. Existing service-100 status handling publishes the fresh count to the home page. This does not suppress a valid unread count based on incomplete conversation data, and does not mark real messages read or send SMS. No periodic timer is added. The generic send API keeps the fix compatible with older running daemons.

`request_message_types` also aligns conversation sync with the SMS/MMS badge by requesting both kinds; contact sync is still explicit. Root tests check the request kinds and GUI tests check metadata counts, cache-only behavior and confirmed read-state handling. The existing isolated home notification test checks that fresh-zero SMS state hides the SMS badge without clearing app notifications.

Verified: locked workspace release build passed without compiler warnings; 339 tests passed, 0 failed, 40 optional tests skipped. Isolated Messages lifecycle and home SMS-versus-notification tests both passed. Read-only device verification confirmed the home and conversation unread counts are now both zero after metadata refresh. No real message was sent or marked read during this fix.
