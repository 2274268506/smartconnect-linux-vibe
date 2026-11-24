# scrcpy apps, fullscreen desktops and multilingual UI

## What and why

A PC-matched phone display did not fill the monitor because scrcpy independently chose a smaller default window. The GUI now requests real fullscreen when desktop resolution follows the computer. Custom desktop sizes stay windowed. Apps gain the same optional scrcpy backend while retaining geometry, DPI, quality and fixed-aspect controls. New GUI preferences default to English; bundled Chinese, French and Spanish remain selectable.

## Implementation

`mirror/src/scrcpy.rs` accepts a bounded window title, fullscreen or native window dimensions, and shared quality flags. `mirror/src/main.rs` retains renderer/ADB options per queued app, keeps its existing Smart Connect source flowing without a decoder, and captures the reported display ID. Owned-child exit and source close clean up each other. Package-keyed `appErrors` are independent of desktop errors. X11 window lookup by child PID applies existing native fit/aspect hints without changing SDL WM_CLASS.

`gui/src/app_resolution.rs`, `apps.rs` and `mobile_desktop.rs` expose and validate the new settings, check host capabilities, and report asynchronous failures. Settings remain independent across apps, desktop and PC-to-phone display. The app backend remains built-in unless selected; existing explicit language preferences remain valid.

`gui/src/i18n.rs` resolves four languages, with exact-key JSON catalogs and English fallback. `i18n-macros/` retains Rust format checking and marks argument boundaries so translated numbered fields can reorder values, retain numeric precision and safely display braces in phone content. Arguments are evaluated once. All translations ship locally.

## Constraints and validation

Authorization is still required for ADB. No arbitrary CLI flags, new display launch or phone-side permission bypass is introduced. Each source/ADB pair retains two phone capture pipelines; concurrent stream capacity depends on the phone. Native sizing requires the existing X11/XWayland path. Fullscreen can retain bars when source and monitor aspect ratios differ.

Release builds and 116 ordinary tests passed (49 GUI, 66 mirror, 1 formatting macro). Three isolated GTK tests passed for app settings, desktop settings and switching all four languages. Real-device checks passed: desktop client geometry 2880×1920 with the fullscreen window-manager state; a 1080×1920 calculator scrcpy window with 9:16 native-height hints, unchanged SDL WM_CLASS, 2 Mbit/s and 30 FPS arguments; viewer-close cleanup; and built-in 720×1280 app rendering. An intentionally missing ADB target produced an app-only error while the live desktop retained its session and child. Initial desktop startup was refused by the phone; retry succeeded. The English UI and app scrcpy selection were deployed while preserving other preferences and the running daemon. Subsequent explicit language changes are retained. All test streams were closed and injected startup errors cleared. Raw logs/screenshots remain outside the repository.
