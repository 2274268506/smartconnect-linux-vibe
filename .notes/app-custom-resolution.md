# Custom resolution for standalone app streams

Previously service 15 cmd 1 always bounded an app to 1920x1080 with supportCustomDisplay=false,
so the phone fitted its own portrait/landscape size into that bound. The new optional app
profile supplies exact displayWidth/displayHeight/displayDensity and supportCustomDisplay=true.
Its displayScale stays 1.0; density directly controls Android app layout/text size.

`shared/app_display.rs` defines geometry validation, wire fields and CLI parsing. The host carries
profiles on StartReq and SessionInfo/Session, preserving queued launches independently; other
apps, view sessions and headless notification actions retain their original parameters. Invalid
requests fail before phone traffic. A changed profile for an existing or pending app is rejected
with instructions to close/reopen rather than reporting success or disrupting an active app.
Explicit geometry disables automatic window-resize requests for that session.

Apps settings adds automatic/custom mode, portrait/landscape presets, width/height and DPI.
Defaults save under apps.streaming by editing only that field, preserving recents and unrelated
JSON values. App tiles snapshot defaults at launch and check host capability. Running rows
report actual size/density and requested size when different. The GUI defaults remain automatic.

Tests cover geometry and CLI boundaries, per-request queue isolation, unchanged automatic and
desktop behavior, preference preservation and an isolated native GTK settings dialog. Live
phone acceptance is recorded below after deployment. The phone/app can still impose encoder
limits, orientation or letterboxing; the settings are a request, not a universal guarantee.

Validation: normal GUI/mirror suites passed (44 GUI tests and 49 mirror tests, with existing
integration exclusions), plus the isolated native GTK settings test. The latter checked preset
changes, rejection of odd width, DPI persistence and preservation of unrelated settings. CLI
DPI-without-size and odd-size requests returned nonzero without starting an app.

On the actual phone, a safe app session requested at 720x1280 and 240 DPI reported exactly those
values and rendered frames; the decoded caps also confirmed 720x1280. Changing its profile while
open was rejected. After explicitly closing it, an automatic launch returned to the phone's
540x1320, 210-DPI fit with null requested dimensions and rendered successfully. Test windows
were closed afterward; real GUI defaults remained automatic. The updated host and GUI are
running. This verifies one portrait size and one app, not every size/orientation/phone.
