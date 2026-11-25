# scrcpy desktop caption hover

## What and why

Maximized phone desktop apps hide their title bar above the top edge and reveal it on pointer hover. The built-in renderer provides native mouse input; scrcpy SDK input targets a separate capture display. Preserve the phone's fullscreen behavior and supplement its original desktop edge monitoring instead of forcing tasks into freeform mode.

## Evidence

**OBSERVED:** the affected desktop display uses freeform as its default while its maximized app task reports fullscreen. A deliberately launched freeform calculator showed a caption, proving the capture can render window decorations. Sending native service-16 mouse moves to the original desktop's top edge revealed the existing maximized app's caption in the scrcpy image. A calculator explicitly forced into fullscreen by shell did not reproduce the phone's normal desktop maximization behavior and was removed after testing.

**INFERRED:** original-display hover monitoring is missed by scrcpy's clone-display SDK pointer route. Upstream describes the distinct display targets in its [Controller](https://github.com/Genymobile/scrcpy/blob/master/server/src/main/java/com/genymobile/scrcpy/control/Controller.java). No proprietary source is included here; private captures and analysis stay outside the repository.

## Implementation

`desktop_hover.rs` maps client pixels into centered aspect-fitted video and rejects letterbox areas. A state machine arms only at the top edge, follows native hover through the caption band, sends one exit move, deduplicates stationary samples and resets outside the owned client. `Session::sync_desktop_hover` runs only for visible scrcpy desktop sessions, using the existing X11 window poll and service-16 mouse movement. Pointer queries are throttled to at most 20 Hz and the owned client ID is cached per child PID, avoiding repeated window-list scans on the host’s 15 ms loop. X11 pointer ancestry prevents an obscured window from receiving input; held buttons suspend forwarding. No clicks are synthesized, task modes are untouched and ordinary scrcpy input is retained. Without X11, the supplement remains inactive.

## Validation

Pure mapping/state tests and an isolated X11 overlapping-window test cover geometry, edge entry/exit and client ownership. Record real-device results separately; do not claim caption button activation solely from a screenshot of a visible bar.

Verified: workspace check passed without warnings; mirror release suite passed (90 tests, 4 desktop-dependent tests ignored). The updated supervised host was deployed after closing only the desktop stream. The user reopened scrcpy desktop, maximized an app, and confirmed top-edge hover reveals the caption and its Restore button returns the app to a smaller window. The isolated-X11 overlap test remains optional and was not run because Xvfb is unavailable locally.
