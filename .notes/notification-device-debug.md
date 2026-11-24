# Diagnose missing phone notifications with network ADB

The phone was connected and the desktop notification cache was empty. Smart Connect's
notification-listener permission was granted and its listener was live. The phone's Share
notifications switch and the current PC checkbox were enabled, but inspected entries in
Allowed apps were unchecked. At the user's explicit request, Selected all apps through the
phone's own settings UI and verified checked entries after returning from the picker. No
notification-listener regrant, pairing reset or service-version patch was needed.

A real application's posted/updated notification reached service 5 and appeared in the daemon's
status list. The desktop mute list was empty. KDE reported Inhibited=true with a long-lived
Do Not Disturb setting. The user explicitly chose to retain it and synchronize only to the
notification list; desktop notification policy was therefore preserved.

Synthetic Shell notifications were not a valid end-to-end proof: test delivery depends on
source-app Android permission and the phone's app filter. A controlled local Chrome download
completed, but Chrome's Android POST_NOTIFICATIONS permission was denied and no Chrome system
notification was posted. The test did not justify changing browser permissions. The local
HTTP listener and ADB reverse mapping were removed; the generated download was cleaned up.

Inspection of the installed Smart Connect build confirmed a per-app notification allow list
and independent global/per-device gates. The temporary APK/decompilation material was kept
outside the repository; no decompiled code, notification text or device identifiers are recorded
here. Documentation now distinguishes allowed-app configuration, source-app notification
permission, PC mute rules and desktop popup inhibition. This was a configuration correction,
not a daemon notification-parser change.
