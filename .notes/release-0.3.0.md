# v0.3.0 release

## What

Publish the accumulated ADB file manager, notification app launch, cancellation, desktop caption hover and isolated scrcpy capture changes as v0.3.0. Align all workspace versions, the Noctalia manifest, Arch package and current installation commands.

## How

Run locked workspace release tests and builds, package the eight binaries for Arch x86_64, and verify extracted binaries, dependencies, service paths and installer dry-run. Archive the tagged Git commit for source distribution; publish the package, source, Chinese installation guide and SHA-256 checksums. Exclude local contributor instructions, settings and diagnostics.

## Why and verification limits

A minor release reflects new file-management functionality. Preserve upstream attribution and MIT licensing. App and desktop streaming continuity after automatic phone sleep and desktop caption restoration were confirmed on hardware. The newest per-display physical double-tap wake compensation still needs hardware confirmation. Native audio can receive negotiated Opus tracks, but separate audio negotiation and bidirectional native audio remain incomplete. No portable AppImage or fresh-machine installation is claimed.

## Release validation

Locked workspace release build passed without compiler warnings. The full locked release test run passed 325 tests, failed none and skipped 38 environment-dependent tests. Original Java capture/wake scope tests passed. Package smoke checks and publication are completed before marking the GitHub release public.
