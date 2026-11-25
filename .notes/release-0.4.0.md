# v0.4.0 release

## What

Publish accumulated photo-gallery, SMS read/status, ADB peer-rediscovery and main-window Quit changes. Remove application restart buttons, delayed restart spawning, the unused language-state accessor, stale translations and duplicate note-index entries. Refresh current documentation and align workspace, plugin and Arch versions at 0.4.0.

## Cleanup scope

Delete disposable local v0.2.0/v0.3.0 package/source archives and previous makepkg extraction directories. Preserve active build binaries, user settings/pairing, existing AGENTS.md, required embedded DEX files, historical release notes and remote release assets. No deprecated source/backup files were found to remove wholesale.

## Publication and checks

Build/test the locked workspace in release mode. Verify isolated GTK language persistence and window lifecycle, plus relevant SMS and Gallery checks. Build the native Arch x86_64 package, inspect its eight binaries/dependencies/service paths and run the source installer in isolated dry-run mode. Generate source from the release commit, excluding local instructions/settings/build products; check all remote asset digests before publishing. Physical double-tap wake, native separate/bidirectional audio, video gallery playback and recursive transfers remain outside the verified feature set.

Verified before packaging: locked workspace release build passed without warnings; 339 tests passed, 0 failed, 40 optional tests skipped. Six isolated GTK checks passed: language persistence, close/reopen responsiveness, SMS conversation controls, home counter independence, Gallery paging/cancellation and Files navigation. Both original Android helper build/scope suites passed. Package/source/remote-asset verification is required before publication.

Native-package verification passed: all eight extracted programs provide help successfully, dependencies resolve, daemon version is 0.4.0, the systemd unit uses /usr/bin, and the source installer passes an isolated dry-run. Source archive and remote SHA-256 checks are publication gates, recorded in the GitHub release.
