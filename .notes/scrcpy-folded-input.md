# Folded scrcpy control

## Evidence and cause

The earlier folded-screen change restored capture power and video, but Android still listed the scrcpy mirror display as noninteractive. The source Smart Connect calculator display remained interactive. SDK keyboard input changed calculator content; SDK pointer clicks did not. Source code confirms scrcpy SDK coordinates target its mirror display, while keyboard events target the original source. A scrcpy UHID mouse associates its input port directly with the original display on modern Android.

## Change

`shared/desktop_renderer.rs` adds `foldMouse`, enabled by default for missing/old settings. App and desktop GUI switches save independently and explain relative capture release. `mirror/src/scrcpy.rs` uses a bounded SDK query and requires Android SDK 35+, scrcpy 4+ and a virtual source before selecting UHID. This conservative gate uses the source-association implementation verified in upstream scrcpy 4/5. Unsupported or unknown combinations and opt-out retain SDK mouse mode. Keyboard remains explicitly SDK to preserve existing text/IME behavior. Logs identify the effective mode.

No helper/phone power-state changes, custom input injection, global focus changes or keyboard-layout changes are required. Existing lifecycle cleanup remains owned by scrcpy and the session. New host capability checks prevent an old host from silently ignoring the enabled option.

## Validation

A folded-phone diagnostic viewer using UHID changed calculator content through real viewer clicks; SDK keyboard still worked. Input diagnostics confirmed the scrcpy port associated with the original source display, not its noninteractive mirror. Test material is outside the repository. Release workspace build and 295 automated tests passed, plus four isolated GTK tests. The integrated folded calculator session selected source-bound UHID and SDK keyboard; typing 7 followed by a pointer click changed the visible result to 78. Diagnostics confirmed source association. The owned diagnostic projection was closed after inspection.

Later keyboard/relative sensitivity work is documented in [scrcpy input settings](scrcpy-input-settings.md); the SDK-only keyboard statements above describe the earlier implementation.
