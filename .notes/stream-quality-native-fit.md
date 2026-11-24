# App native fit and per-stream quality

## What and why

The user requested bitrate/FPS controls for app streaming and phone desktop, plus a fixed app
ratio that can fill a monitor's height or width at 1:1 source/display pixels. Defaults retain
automatic quality, and the PC-to-phone Display configuration is independent.

## How

`shared/stream_settings.rs` holds validated quality/layout types and overflow-safe even-pixel
geometry. The GTK app uses the selected monitor's physical size (logical dimensions × scale),
rounds only the derived side to even pixels and rejects oversized results. It recalculates
native fit on app launch. A different ratio may extend beyond the monitor on the other axis.
`gui/src/stream_controls.rs` shares independent automatic/manual bitrate and FPS rows between
app settings and desktop. Preferences persist separately and preserve unrelated fields.

App start requests carry their own quality/layout, including queue/inflight/session ownership.
Desktop quality is copied only onto desktop starts. Changing an open session's settings is
rejected until it is closed; the desktop Apply button handles restart.

The phone's start bean has geometry but no known bitrate/FPS fields. Rather than inventing
wire keys, the host places bitrate ceilings into video answer SDP (`b=AS`, `b=TIAS` and
`x-google-max-bitrate`). Optional `max-fr` and `a=framerate` request frame-rate restriction,
but remote support cannot be assumed. `mirror/src/quality.rs` also caps presentation after
H.264 decoding using monotonic deadlines, exposing `displayRateDropped`. This does not force
phone encoding FPS or reduce bandwidth for locally skipped frames. The UI calls it a display
frame-rate ceiling, and leaves negotiation automatic by default.

XWayland fixed-ratio windows use existing WM size hints with an aspect constraint. Native fit
adds borderless client geometry and min/max size equal to the phone-reported video dimensions;
manual fixed ratios remain resizable. The decoder still preserves aspect ratio. Encoder,
compositor, XWayland scaling and external window rules may affect physical pixel mapping;
a request alone does not prove the final output size. No captures, APK material or secrets
are stored in the repository.

## Validation

See subsequent entries for suite, isolated GTK and actual-device checks.

The normal suites passed (58 mirror, 46 GUI; existing display-dependent exclusions retained).
Two isolated GTK tests passed for app layout/quality persistence and desktop quality isolation.
They exercised physical-height fitting, oversized-fit rejection, invalid odd dimensions and
preservation of unrelated desktop/display/language/recent-app settings.

On the actual device, an app requested at 1080x1920 with 9:16 height fit, 1500 kbit/s and
15 display FPS started and rendered. X11 inspection showed client width 1080, height 1920,
top edge at zero, equal min/max dimensions, matching aspect hints and disabled decorations.
Decoded caps also showed 1080x1920. Session counters showed 13 presented and 25 locally
rate-limited frames at the sampled point, demonstrating that the phone did not fully honor
the remote FPS hint and the local fallback was needed. Desktop started at 1280x720 with
2000 kbit/s and 20 display FPS, rendered and reported its independent profile. Both test
sessions were explicitly closed. This validates negotiation/start and local FPS enforcement;
it does not establish a constant encoded bitrate, acceptance of higher-than-default bitrate,
or complete sender-side FPS control on this phone. Real GUI preference values were preserved.

The final release host and GUI were deployed. The GUI reported connected/paired, the original
daemon remained running, real gui.json bytes were unchanged, no test stream remained, and the
phone desktop size was restored to the saved desktop dimensions. The host was restarted after
testing to clear its temporary desktop quality profile. AGENTS.md was not modified.
