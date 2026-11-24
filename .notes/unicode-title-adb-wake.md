# Unicode captions and pre-start ADB wake

## What and why

Fixed-aspect scrcpy app windows passed Chinese UTF-8 bytes to WM_NAME while declaring Latin-1 STRING. The host now uses UTF8_STRING for both title properties; SDL WM_CLASS remains intact. Apps and Desktop mode gain independent, disabled-by-default ADB wake/unlock switches for either renderer.

## Approach

The existing shared ADB options retain compatibility and add `wakeUnlock`. GUI controls reveal/validate device and port whenever ADB preparation is enabled, including built-in rendering. Host preparation actions wait in a package-keyed map. `WakePreparation` reuses authorized device selection and cancellable bounded subprocess execution without requiring scrcpy. It sends wake and normal keyguard dismissal, verifies policy, and releases the app/desktop action only on success. Cancellation drops the task and continuation. Existing pending/error reporting handles this stage. An injected task factory keeps host tests independent of actual ADB/device state.

Password/PIN/pattern/biometric locks require phone-side authentication. Unknown keyguard output fails conservatively. No credentials, lock settings or consent bypass are added. Phone policy output is never logged or committed.

## Validation

Regression checks cover command order, protected/unknown lock handling, interrupted ADB checks, delayed continuation, cancellation, error isolation, and settings persistence. The X11 test verifies exact Chinese/emoji UTF-8 bytes and property types. Full-workspace release build and tests passed (294 tests, zero failures). Four isolated GTK checks passed, including app/desktop option persistence and preparation cancellation. The X11 Unicode-property integration test passed. A real Chinese calculator scrcpy window had UTF8_STRING Chinese titles on both properties with WM_CLASS still scrcpy; real preflight preceded app setup, cancelled preparation created no late window, and built-in desktop also started through ADB preparation. Test streams were closed and saved preferences preserved during deployment. Raw phone/debug material remains outside the repository.
