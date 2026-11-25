# Shared app-stream power handling

## What and why

Apply the desktop capture power-isolation and guarded physical double-tap scheme to scrcpy app streams. An app’s source display also uses an independent native streaming group, but the original scrcpy capture joins the physical screen group. Reuse the existing launcher rather than creating another power helper or changing phone settings.

## How

`capture_server::supports_package` selects the desktop and valid Android app packages, excluding mirror/mobile view modes. `scrcpy::Spec::capture_isolation` carries that selection into the existing Android-version/asset checks. Existing rendering, quality, resolution, audio, keyboard/mouse settings and Cancel behavior stay in the same launch path.

`Server.streamSource` validates a virtual AppStream source, its package/desktop token and an optional bounded hexadecimal session prefix observed on native app displays. Each capture must still mirror the selected source ID exactly. `DesktopWake` shares that source predicate so the existing enabled-setting, real-double-tap, sleeping-physical-display and proximity guards apply to apps too. The launcher loads the installed server unchanged and preserves cleanup/fallback behavior.

## Validation

Host eligibility and JVM source-scope regressions cover apps/desktop, view exclusions, tagged names, malformed tags/packages and source mismatches. Device checks use the calculator only; private diagnostics remain outside the repository. App sleep continuity and physical gesture results must be recorded separately from automated test results.

The earlier desktop capture continuity fix was confirmed by the user. Physical double-tap wake during desktop operation was still awaiting validation; extending the code to apps alone does not establish that the physical wake fix works on the phone.

The user confirmed app streaming continues normally after automatic physical sleep; physical double-tap still needed further work. The shared wake bridge now reads display state directly from the display service (avoiding client cache state) and calls the existing per-display power-service gesture wake API rather than injecting a wake key that can be ignored while aggregate interactivity remains true. It logs only bounded static rejection reasons. Existing shell DEVICE_POWER permission was verified; no permission was granted or setting changed. Final physical-gesture behavior remains subject to the next device check.

Validation: host/workspace check passed, workspace release tests passed (325 passed, no failures), mirror release suite passed (92 passed, five optional tests ignored), and expanded JVM scope tests passed, including tagged app sources and both physical-panel sleeping-state guards. User confirmed app projection remains normal after the phone sleeps. A sensor-only probe resolved the per-display power API and confirmed registered sensors plus initialized, clear proximity without issuing any wake request. The latest per-display physical-gesture revision is deployed, with final real-gesture confirmation pending.
