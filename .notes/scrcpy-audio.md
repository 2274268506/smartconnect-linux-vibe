# scrcpy phone audio

## Problem and change

The integrated scrcpy argument list unconditionally contained `--no-audio`, so every app/desktop projection was silent even when phone and desktop supported audio forwarding. Restore output audio by default and provide an independent **Play phone audio** switch in each Display method panel.

## Implementation

`ScrcpyOptions.audio` is a strict boolean, defaults to true for old settings and is reported in `requestedScrcpy`. Enabled emits `--audio-source=output`; disabled emits `--no-audio`. The source is phone output, not a microphone, and may include other apps. No codec/buffer override or require-audio flag is introduced. GUI capability checking rejects a stale host that would silently ignore the option. The existing owned child handles audio lifetime/cleanup; no Smart Connect SDP/audio protocol changes are needed.

Keep audio enabled in only one simultaneous viewer to avoid competing output captures. Unsupported Android/capture/codec environments can still show video. [Upstream audio documentation](https://github.com/Genymobile/scrcpy/blob/v5.0.1/doc/audio.md) documents Android 11+ and Android 11 unlock requirements.

## Validation

Release workspace build, 312 automated tests and two isolated GTK settings checks passed. Old preferences enable audio, the argument regression verifies output/no-audio exclusivity without changing input, and app/desktop audio opt-outs persist independently.

The updated GUI/host were deployed while idle and preserved existing preferences/connectivity. On Android 16/scrcpy 5, an owned Settings projection selected output audio, created an active unmuted desktop playback stream at 100% stream volume, and kept computer IME/compatibility mouse arguments. The desktop default sink was unmuted. Phone media volume was 0/15; a temporary quiet diagnostic volume change was restored to 0/15. Synthetic shell audio probes did not establish nonzero audio at the PC; this diagnostic limitation is distinct from ordinary app playback. After reopening the updated projection, playing normal media and setting phone media volume above zero, the user confirmed that sound was audible on the computer. No raw audio was saved; only numeric levels were computed in memory. Diagnostic source/child/helper uploads were cleaned up. No private audio recordings are placed in the repository.

## Independent volume mode

The follow-up `independentAudio` switch selects `--audio-source=playback` instead of `output`. It defaults off, preserving the user-confirmed working output path, and saves separately for app/desktop. Audio-off takes precedence. The host validates Android SDK >=33 (including failure for unknown levels), uses no duplication/require-audio flag and never writes or enforces phone media volume. Application capture exclusions may make playback mode silent, so output remains an explicit fallback.

The intended behavior is computer-controlled loudness independent of the phone's output level. Argument selection and API support alone do not establish muted-phone correctness on this device. Release workspace build, 313 automatic tests and two isolated GTK settings checks passed. The updated GUI/host were deployed while idle with preferences/connectivity preserved. An owned Android 16 Settings projection selected `--audio-source=playback`, retained the existing computer IME/UHID mouse arguments, and produced a desktop playback stream without logged audio initialization errors. The owned test stream/child were closed. The first user report of silence at phone volume zero was followed by inspection showing the active projection and both saved profiles still had `independentAudio:false`; that result does not establish playback-capture behavior. The requested option was then enabled in both local profiles and the GUI reloaded without closing the live stream. After restarting the projection with the saved setting, the user confirmed that ordinary media remained audible on the computer with phone media volume at zero. Independent capture is therefore user-verified on the current device/application; it is not a claim for apps that prohibit playback capture. Both local profiles remain enabled, with no phone-volume enforcement.
