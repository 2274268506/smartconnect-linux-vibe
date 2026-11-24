# Folded scrcpy capture and cancellable startup

## Problem and evidence

On the test foldable, the physical inner screen was OFF while the cover screen and Smart Connect app virtual display were ON. scrcpy created another display in the default power group and its capture was OFF, yielding no first texture. A targeted per-display ON request restored the calculator without waking the inner screen. This is capture power, not a lost source stream or a reason to disable keyguard.

## Changes and rationale

`mirror/android/DisplayPower.java` uses reflection to call the Android integer per-display power API. It snapshots pre-existing captures before a ready handshake, matches a new scrcpy shell-owned virtual display to the requested Smart Connect source through bounded adapter discovery, and monitors its ID/unique ID. It refuses ambiguous matches and never selects physical displays. The checked-in DEX is built from original source with the supplied regeneration script, so normal builds need no Android SDK. `mirror/src/scrcpy.rs` uploads a unique helper artifact, owns its cancellable process group and stdin, and removes the upload on every exit path. Unsupported APIs degrade to the existing viewer. No permanent phone setting changes.

The host now exposes pending packages and `cancel {pkg}`. Queue/in-flight removal and session close are paired with a short local cancellation marker; late invites/offers cannot create a pipeline, and known late session IDs receive the existing close command. Explicit retry clears the marker. GUI worker controls serialize socket startup and cancellation while GTK stays responsive. Desktop, app and whole-screen startup provide Cancel; desktop timeout uses the same cleanup. Other apps remain independent.

## Validation

121 ordinary GUI/mirror tests pass, including queue isolation, late offers, retry, view cancellation and cancelling before host creation. Four isolated GTK tests pass: desktop/app preparation cancellation plus existing desktop/app settings regressions. Folded-device checks passed: visible calculator frames with the inner physical screen OFF; recovery after powering only the owned capture OFF; source/viewer/helper cleanup; cancellation before app, desktop and whole-screen offers; no cancelled window reopening; no test helper uploads left. Concurrent scrcpy app captures also restored their own power independently. The new GUI preserves saved language and quality preferences. Pure JVM helper tests reject physical/wrong-owner/wrong-source/existing/ambiguous captures. Research logs/captures are private temporary files outside the repository.

Full-workspace release build and tests also passed: 289 tests passed, zero failures. Optional desktop/device cases remain separately run or ignored as documented.
