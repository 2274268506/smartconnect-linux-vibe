# Phone desktop page and resolution controls

`gui/src/mobile_desktop.rs` adds a dedicated Desktop mode tab. It manages only the `desktop`
view served by smartconnect-mirror: presets, custom even landscape dimensions, start, stop,
restart to apply, a bounded startup wait, and the phone's reported resolution. Work runs off the
GTK thread. Polls are limited to a visible page, a pending start or an active desktop.

`DesktopPrefs` stores width and height under gui.json's desktop field. An atomic GUI-field update
preserves other settings and never writes display.json. New strings use the existing English /
Simplified Chinese translation path. This page is distinct from PC-to-phone screen extension.

`gui/src/apps.rs` checks the host's new capabilities command before using dimensions, because
an older host otherwise silently ignores them. Applying a new size closes only a desktop stream,
waits for it to leave the host and briefly lets the phone release its display before restarting.
The host rejects starting desktop over a whole-screen mirror or virtual-phone session.

The host validates all dimensions before sending service 1 command 33 and the desktop start
(view 2, reply 0). The following service 15 start uses the same dimensions and proportional
physical size. Normal app streams retain their existing bounds. CLI --desktop --size WxH uses
the same path, or initializes the equivalent settings in a standalone viewer.

Session responses include actual reported width/height and desktop requestedWidth/requestedHeight;
the phone can limit a requested size. Protocol payload tests and GTK tests cover forwarding,
invalid input, default behavior, presets, persistence and independence from display/language
preferences. Device verification is recorded separately; passing synthetic tests alone does not
prove a phone supports every preset.

Device check: a connected Motorola razr+ 2025 returned 1280×720, then 1920×1080 after the
desktop was closed and requested again. Both sessions received video. The Smart Connect app
version was not recorded. QHD, 4K and custom dimensions have validation/UI coverage but were
not checked on this phone. Raw diagnostic logs stay under /tmp, outside version control.

Automatic resolution and window style: new installations select the monitor containing the GTK
window (first monitor as fallback), multiply logical dimensions by fractional scale and fit the
result into the even landscape codec bounds. `followComputer` records the mode separately from
width/height, so it remains automatic across restarts; older explicit saved dimensions remain
manual. `borderless` defaults off and is saved independently of display.json.

The host advertises desktopBorderless only for the glimagesink X11 window path. A desktop_window
control request updates only desktop sessions; their atomic style flag is applied by the existing
window poller through _MOTIF_WM_HINTS. Removing/restoring decorations does not restart media or
change other app windows. Native Wayland handles are no longer passed to the X11 title helper.
Unsupported backends and malformed booleans fail explicitly.

Device/window validation for automatic resolution and borderless mode: the connected phone
reported 2880×1920, matching the PC's current physical mode at 200% desktop scale. On KDE
Wayland's XWayland video window, enabling borderless changed _NET_FRAME_EXTENTS from
0,0,48,0 to 0,0,0,0; disabling it restored 0,0,48,0. The same media session remained active
through both toggles. The user-facing switch remains off by default.
