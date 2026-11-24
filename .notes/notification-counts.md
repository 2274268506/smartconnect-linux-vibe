# SMS and notification count separation

PhoneStatus.unread comes from service 100 cmd 1018 unreadCnt: the phone’s unread SMS/MMS
count. PhoneStatus.notifications is the forwarded application-notification list. The old GTK
Phone page put the former count on a button that opened the latter list, so two unread phone
messages could lead to a completely empty Notifications page. No notifications were missing
from the list in that case.

The Phone page now shows SMS/MMS as a separate non-interactive label with “view on phone”.
The notification shortcut counts the same locally visible entries as the Notifications tab.
Both use visible_notifications, excluding keys in the local dismissal set. Refreshing the
notification list also refreshes the phone summary, so Dismiss/Clear all changes the shortcut
immediately without waiting for another daemon status message. No phone read state is changed.

A desktop-session regression reproduces unread=2 with an empty notification list, then adds
notifications, exercises the shortcut and local dismiss/clear, and checks that SMS status stays
independent. It also verifies notifications can remain visible when unread SMS is zero. The
wire format remains backward compatible, and no SMS body or sender is read for this fix.

Validation: the isolated GTK regression passed for two unread SMS/MMS with no notifications,
notification navigation/counts, immediate local dismiss/clear updates and the SMS-zero case.
The existing GUI and mirror unit suites also passed. No daemon restart or wire-format change
is needed; only the GTK application is rebuilt/restarted.
