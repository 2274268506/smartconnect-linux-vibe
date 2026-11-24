# scrcpy mouse speed and phone input method

## What and why

The compatibility mouse previously had no sensitivity control. SDK keyboard injection does not behave like a physical keyboard for phone IME composition. The requested input method is the phone IME, with the computer keyboard selecting candidates.

## Implementation

`shared/desktop_renderer.rs` adds `mouseSpeedPercent` (25–400, default 100) and `phoneIme` (default true), with strict integer/boolean validation and migration through serde defaults. App and desktop settings expose/save them independently in all four languages. Mouse speed is editable only while compatibility mouse mode is enabled; saved values survive toggling it.

`mirror/src/scrcpy.rs` gates both UHID devices independently on a virtual source, Android SDK 35+ and scrcpy major 4+. Phone IME uses `--keyboard=uhid`; disabled/unsupported cases use SDK. The existing scrcpy source-display association and child cleanup manage device lifetime. No phone layout/default IME is changed. Compatibility mouse speed uses `SDL_MOUSE_RELATIVE_SPEED_SCALE` on the child command only. It does not modify system-wide speed or transform SDK absolute pointer coordinates. Existing sessions must be reopened.

Source references: [SDL relative mouse sensitivity](https://wiki.libsdl.org/SDL3/SDL_HINT_MOUSE_RELATIVE_SPEED_SCALE), [scrcpy keyboard modes](https://github.com/Genymobile/scrcpy/blob/v5.0.1/doc/keyboard.md). The feature uses phone-side composition; it does not claim to inject arbitrary Unicode through SDK keyboard events or use the computer IME.

## Validation

Release workspace build and 309 automated tests passed. After the child IME-environment fix, all 83 mirror tests passed again. Two isolated GTK tests passed, exercising app/desktop save/reopen, independent sensitivity values and phone-IME opt-out. The installed GUI/host were restarted while idle; existing GUI preference bytes and daemon connectivity were preserved.

On the connected Android 16 phone with scrcpy 5, the Settings projection selected source-bound UHID keyboard/mouse and a child-only 150% mouse hint. Initially desktop Fcitx consumed test letters; after isolating the child IME environment, typing into the harmless Settings search inserted the expected Latin test text. Input diagnostics showed the keyboard/mouse associated with the source display. Chinese candidate composition was not demonstrated with the current phone IME configuration, so this remains a phone-IME/layout verification limit, not a claimed end-to-end Chinese success. All owned test sources/children were closed. No real messages were sent. Logs and device artifacts remain outside the repository.

With a UHID keyboard, the scrcpy child uses `SDL_IM_MODULE=none` and `XMODIFIERS=@im=none` so desktop Fcitx/XIM composition does not consume physical keys intended for the phone IME. SDK fallback leaves those variables inherited. These environment changes are scoped to the child; the desktop input method remains unchanged.
