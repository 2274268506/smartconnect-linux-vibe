# First public Vibe release

## What and why

Publish v0.2.0 of smartconnect-linux-vibe with explicit upstream attribution at the start of README and the Chinese installation guide. Preserve the MIT license and original executable names. Direct downloads/source checkout to this fork so installation does not silently retrieve the upstream version.

## Packaging

Workspace crates, Noctalia manifest and Arch metadata share version 0.2.0. The native x86_64 Arch package provides/conflicts with smartconnect-linux and ships the eight binaries, desktop files, user unit, plugin, documentation and license. SDL3 is a build dependency for the computer IME adapter; scrcpy/ADB remain optional tools. Package documentation/plugin paths match the Vibe package name. The inherited AppImage build feed points to this fork, but no AppImage is promised for this release.

Source archives come from the release commit with git archive; no working-tree configuration, pairing material, local AGENTS, target directory or logs are included. Release assets include package/source checksums and installation instructions. Existing user sessions are not changed by the release packaging checks.

## Validation

The 0.2.0 workspace passed all 313 release tests. Native Arch packaging completed using its configured release build flags. An isolated extraction verified all eight executables respond to --help, every dynamic dependency resolves on the build host, the daemon reports 0.2.0, the user unit invokes /usr/bin/smartconnectd, license bytes are preserved, fork documentation matches and no plugin settings are included. The source installer passed an isolated no-build dry run. No system package was installed and no user projection was restarted by these checks. A fresh-machine install is not claimed. Platform scope is current Arch/EndeavourOS x86_64; other distributions use source installation and must meet library version requirements.
