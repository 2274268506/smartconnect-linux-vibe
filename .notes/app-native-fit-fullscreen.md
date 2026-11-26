# Fullscreen for monitor-matching app fits

## Cause

The GUI correctly computed 2880×1920 for 3:2 width fit on a matching monitor, but only requested borderless client geometry. KDE could constrain that ordinary window to its work area; borderless does not imply fullscreen.

## Change

`Layout::fills_monitor` compares fitted physical dimensions to the launch monitor. The GUI sends a per-launch boolean `fullscreen`; the host validates fixed non-manual fit, preserves it through the queue/session and rejects conflicting pending/open profiles. scrcpy uses its existing fullscreen flag, while built-in X11/XWayland requests `_NET_WM_STATE_FULLSCREEN`. Manual geometry and mismatched ratios retain their existing behavior, without changing the phone ratio or stretching the picture. No setting migration is needed.

## Validation

Regression checks cover both fit axes, mismatched/manual/disabled layouts, boolean validation and queue isolation for both renderers. An isolated X11 integration check verifies fullscreen state plus full-monitor geometry under the actual window manager. Locked release workspace build passed without warnings; workspace tests passed (347 passed, 0 failed, 42 ignored). Both new desktop-dependent checks were then run explicitly and passed. Reloaded the idle host and GUI without changing saved preferences. A real calculator scrcpy session reported fullscreen, its KDE window had `_NET_WM_STATE_FULLSCREEN`, and client geometry was exactly 2880×1920. Closed the test session afterward; no pending sessions or app errors remained.
