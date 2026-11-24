# Capture a complete Smart Connect desktop with scrcpy

## What and why

The user saw a taskbar-less desktop from `scrcpy --new-display` and asked whether scrcpy
could show the complete Motorola desktop. Network ADB access was explicitly authorized.
No launcher replacement, phone configuration change or application code change was needed
for the tested combination.

## Procedure and observations

Start the phone desktop through this project's existing desktop command. Query `sessions`
for `pkg:desktop` and its current `displayId`, or run `scrcpy --list-displays`. Capture that
existing display using `scrcpy --display-id=ID --no-audio --video-bit-rate=8M --max-fps=60`.
Do not create a second desktop with `--new-display` for this method.

OBSERVED on Motorola Android 16 with scrcpy 5.0.1: a short recorded capture at 2880x1920
contained the full desktop, taskbar, app-drawer entry, navigation and status area. The recording
completed and a frame was decoded for visual inspection. Normal X11 scrcpy playback also
created a visible window and negotiated VA-API decoding with a 2880x1920 texture.
Record-only playback with automatic hardware decoding crashed the local client; a software
retry (`--hwdec=disabled --no-playback --no-control`) completed successfully. This observation
does not establish a general upstream cause. Android warned about missing protected-buffer
support on the source display; ordinary desktop capture nevertheless succeeded.

The source Smart Connect window must remain alive (minimize it to avoid duplicate windows).
Closing it destroyed the source display and scrcpy disconnected; the display ID changed on
restart. The project's daemon/host keeps its normal heartbeat and encoder active, so this
currently uses two capture/encoding sessions and does not provide scrcpy-only initialization
or prove lower latency. Bitrate/FPS command flags are requested limits, not measured throughput.

Research screenshots, videos, device dumps, addresses and identifiers remain outside the
repository in a private temporary directory. Existing GUI preferences and AGENTS.md were
not modified. A normal scrcpy viewer and its minimized source desktop were left available
for the user to try.

The subsequently integrated backend is described in [scrcpy-desktop-integration.md](scrcpy-desktop-integration.md); it removes the manual owner window and manages capture lifetime.

The original example above predates the [audio fix](scrcpy-audio.md); current sessions default to phone-output audio and only pass `--no-audio` when explicitly disabled.
