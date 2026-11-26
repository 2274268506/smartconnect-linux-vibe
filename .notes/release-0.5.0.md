# v0.5.0 release

## Scope

Publish scrcpy video codec/encoder controls alongside monitor-matching app fullscreen, Wi-Fi-over-PAN address selection and guarded BLE cleanup. Align workspace, plugin and Arch package versions at 0.5.0. Preserve existing pairing and per-mode preferences; old scrcpy profiles remain H.264 with automatic encoder choice.

## Validation and publication

Require a warning-free locked release workspace build/test run, isolated app/desktop settings checks, Arch package inspection and eight-binary help smoke tests. Generate source from the release commit, excluding untracked local instructions, build products, settings and raw device output. Upload the package, source, Chinese install guide and SHA256SUMS to a draft GitHub release; compare downloaded asset digests before publishing. Hardware codec support is device-dependent and must not be claimed without a live capture.

Validation: locked release workspace build passed warning-free; 350 tests passed, 0 failed, 42 optional tests skipped. Both affected isolated GTK settings checks passed. The source installer dry-run passed. Phone control was unavailable during this release, so new H.265/AV1 hardware playback remains unverified; prior native-fit fullscreen and Wi-Fi/PAN checks are retained in their feature notes.

The native Arch package passed inspection: all eight extracted programs returned help successfully, shared libraries resolved, daemon reports 0.5.0, service paths use /usr/bin, and private settings/local AGENTS.md are excluded. Publication assets will be verified by downloading the draft before making it public.
