# scrcpy video encoding selection

## What and why

Expose video codec (H.264/H.265/AV1) and optional phone MediaCodec encoder in app and desktop scrcpy settings. Preserve H.264/automatic encoder for older settings and keep app/desktop profiles independent. Built-in capture negotiation is unchanged.

## How

Shared typed options validate codecs and bounded ASCII encoder names. Shared GTK encoding rows provide translated controls with per-page persistence/visibility/launch sensitivity. The host forwards bounded values as separate command arguments and records selection in existing diagnostics. A failing encoder uses existing scrcpy error handling; unsupported hardware is not silently substituted. Users can inspect available phone encoders with scrcpy’s list command.

## Validation

Check old-setting migration, invalid request rejection, per-child argument isolation and isolated app/desktop GUI persistence. Verify a real phone H.265 capture where available, then run locked workspace build/tests and native-package smoke checks before release. Raw device output stays outside the repository.

Locked release workspace build passed without warnings; 350 workspace tests passed, 0 failed, 42 optional checks skipped. The isolated app settings check passed with H.265/custom encoder persistence while preserving other modes. The phone control connection was unavailable during this release, so H.265/AV1 hardware playback is not claimed.

Both isolated GUI checks passed: H.265/custom encoder for apps and AV1/custom encoder for desktop, including reopen and cross-mode persistence.
