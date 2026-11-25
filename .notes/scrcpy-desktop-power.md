# Independent desktop capture power

## Problem and evidence

The phone's native Smart Connect desktop has its own display/power group. Standard scrcpy creates an AUTO_MIRROR capture in the physical main screen's group. **OBSERVED:** the desktop source stayed ON while the capture followed the main screen's power state. Restoring only that capture's per-display power did not give a consistent physical-screen sleep/wake lifecycle. The user reported desktop blackout on automatic sleep and failure to wake the phone by tapping its physical screen.

Android removes independent-group behavior from an ordinary auto-mirror display. The working prototype instead uses an own-content trusted virtual capture and a WindowManager-provided mirror of the existing desktop, parented to an owned container on the capture's layer stack. **OBSERVED:** this capture belongs to the desktop group, renders the full desktop/taskbar and preserves cloned input windows. It neither creates a replacement desktop nor changes the phone's timeout, gestures, keyguard or physical power settings.

## Implementation

`mirror/android/capture/` is original Java code. Its two compatibility entrypoints load the installed scrcpy server unchanged in a separate DEX classloader; the original cleanup helper remains active. A proxy within that one server process scopes changes to a virtual AppStream desktop source and a scrcpy capture mirroring exactly that source. AUTO_MIRROR is replaced with OWN_CONTENT_ONLY, OWN_DISPLAY_GROUP and TRUSTED. A WindowManager mirror supplies video; a retained container/layer pair sets scale and layer stack, is removed on capture release, and is cleaned at process exit. Failed setup restores the original capture flags and falls back to stock capture. SECURE capture flags are not added.

`capture_server.rs` writes a private two-entry STORE archive containing the original launcher DEX and the installed server bytes. It uses ZIP CRC32, fixed entry names and bounded server assets; it never downloads or persists a copy of upstream code in the repository. `SCRCPY_SERVER_PATH` is honored as the original asset location, followed by installed-prefix discovery. The temporary host directory is removed on worker exit; extracted phone assets use a shutdown hook because upstream explicitly calls System.exit.

`scrcpy.rs` enables the launcher only for desktop sessions on Android 15+, retaining the existing capture-power helper as a fallback. `SMARTCONNECT_SCRCPY_CAPTURE_ISOLATION=0` disables the adapter for diagnostics. App streams, physical mirroring, startup wake/unlock preferences and global ADB state are unchanged.

## Validation

Pure Java scope tests reject physical displays, other apps/viewers and mismatched sources. Rust tests check private permissions/cleanup; an independent Python ZIP reader verifies CRCs and unchanged installed-server bytes. The mirror release suite and real-device rendering/cleanup checks are recorded after execution. The decisive device check is automatic physical-screen sleep with the desktop still updating, followed by double-tap wake on the phone; a visible desktop alone does not establish that result.


## Physical gesture follow-up

The user confirmed the deployed capture isolation keeps the desktop running after the phone automatically sleeps. They corrected the baseline: physical double-tap wake works after all projections close, but not while desktop mode runs. A previous tentative report that it failed without projection was withdrawn and must not be used as evidence.

**OBSERVED:** during desktop operation the physical group is Dozing while the aggregate device state remains Awake. Both Motorola double-tap sensors exist and the user's secure double-tap preference is enabled. **INFERRED:** the phone's normal wake handling can suppress the gesture while an independent desktop keeps aggregate interactivity true.

`DesktopWake.java` subscribes to the existing Motorola hardware double-tap sensors only within a successful isolated desktop capture. It accepts the confirmed double-tap event type, maps only known inner/outer sensors to physical displays, requires an active native desktop, an enabled user preference, a sleeping physical display and a known-clear proximity sensor, and debounces duplicates. Only then does a bounded owned `input -d <physical-display> keyevent KEYCODE_WAKEUP` process request physical wake. This never sends POWER toggles, dismisses keyguard, reads credentials or changes settings. Listener/process teardown follows server exit. Device verification of this follow-up remains pending until executed.

The scoped wake bridge passed pure JVM guard tests. A bounded sensor-only device probe (capture predicate always false, so no wake action possible) verified sensor registration and receipt of initial proximity state; it unregisters on completion. The updated host is deployed. Physical wake during an active desktop still requires user verification. Network ADB was reconnected after an address change without changing saved device selections or restarting the global ADB server.

Follow-up: the initial KEYCODE_WAKEUP subprocess path did not establish physical wake while projection remained active. It has been replaced by an existing per-display power-service gesture wake call, with fresh display-service state reads and the same gesture/preference/proximity/source-lifetime guards. The callback runs on an owned daemon worker and unregisters on exit; no power-key toggle or keyguard dismissal is used.
