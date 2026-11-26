# Notes index

References to `evidence-2026-10-20/` in these notes (and in some code comments) point to the
author's private research folder: captures, logcats, decompiled output and raw specs. It is not
published. The public protocol reference is [`docs/protocol/`](../docs/protocol/README.md).

- [rung4-pairing.md](rung4-pairing.md) — the QR + control-WebSocket daemon (rung 4); the solved verCode version gate (903013000); the QR `cmd:1` fix; BLE shown to be a red herring for first pair.
- [wifi-reconnect.md](wifi-reconnect.md) — reconnect without a QR over `/readyfor/mdns`: AES-GCM discovery frames with our per-pair key, avahi advert/browse, Connect button, daemon-restart dial-back, post-pair tile refresh, and the Bluetooth LE fallback for desktop Connect.
- [background-reconnect.md](background-reconnect.md) — reconnect with the phone app in the background: why Wi-Fi is refused (no listener, phone never auto-connects an os=6 PC), BLE wake as on Windows; connect lock, cancel+rescan on LE timeout, 30 s scans, presence-probe loop with 60 s cap on LAN, resume/network/Bluetooth-on nudges, 70 s dead-link drop, dynamic invite IPs, `--no-ble-wake`; measured timings
- [rung5-session-and-pairing-commit.md](rung5-session-and-pairing-commit.md) — working end to end: genuine-shaped verify reply, `serviceVersions` gating services, the EXCHANGE_KEY pairing commit (device listed + connected), two-way clipboard text sync via wl-clipboard, and the empty-`sn` identity-poisoning pitfall.
- [notifications-and-desktop-ui.md](notifications-and-desktop-ui.md) — phone notifications → desktop (zbus), Reply window (qs), status/control socket API + CLI, Noctalia plugin; why action buttons need app streaming (svc 15); dedupe/coalescing, themed icons, per-app mute.
- [screen-mirroring.md](screen-mirroring.md) — phone screen mirroring: daemon relay (watch/send), Rust webrtcbin viewer, svc 16 input, and the silent-failure pitfalls found on device.
- [sms-messages.md](sms-messages.md) — SMS over svc 204: MessageData envelope, cmd 18 enable gate, local thread grouping, sms_* control commands.
- [gtk-gui-frontend.md](gtk-gui-frontend.md) — GTK4/libadwaita desktop app (`gui/`): control-socket client, Phone/Apps (app streaming via mirror host)/Display/Files/Notifications/Pair pages, `gui.json` prefs, bottom switcher bar on narrow windows. (Layout superseded by gtk-ui-redesign.md; backend/wiring still current.)
- [gtk-ui-redesign.md](gtk-ui-redesign.md) — GTK app layout restructure to the shared mockup (header device menu holding Pair/Preferences/Disconnect, 5-view switcher, Phone card with inline stats + connected/away/unpaired/service-down states and a persisted last device, Apps grid+recents, Display/Files cards, Notifications ⋯ menu, Pair dialog); theme-native (no colour CSS); all backend wiring preserved.
- [gui-app-icons.md](gui-app-icons.md) — GTK app icon resolver (`<iconName>` → `<pkg>.png` in `~/.cache/smartconnect-linux/app-icons/` → exact themed/desktop icon → AdwAvatar initials), off-thread PNG decode + mtime cache, FileMonitor rescan; mirror harvests svc 15 cmd 2/13 icons; `--list-apps --json` keeps `iconName`
- [cross-control.md](cross-control.md) — svc 26 Cross Control: Wayland edge strip + pointer lock client driving phone input over svc 16
- [cross-control-frontend-ui.md](cross-control-frontend-ui.md) — Cross Control toggle + edge selector in Noctalia panel and GTK app; why the notification mute UI waited for `pkg` (since added)
- [app-streaming.md](app-streaming.md) — per-app streaming host (`smartconnect-mirror --host/--app/--list-apps`): one webrtcbin window per svc15 session, phone-initiated invites, shared instanceId, X11 titles; opt-in window resize (cmd 17, debounced), repeated-invite dedupe
- [helper-supervision.md](helper-supervision.md) — daemon spawns/restarts `smartconnect-mirror --host` after verify and `smartconnect-crosscontrol --accept` on phone svc26 cmd 0; crosscontrol single-instance lock + takeover; reply-window qs exit fix
- [display-input-injection.md](display-input-injection.md) — phone-as-display (svc 202) data-channel input: packet decode (touch/mouse/key/pen), touch gesture state machine, niri virtual pointer (pinned per output) + virtual keyboard injection
- [phone-feature-tiles.md](phone-feature-tiles.md) — why tiles appear (serviceVersions + svc 25 `cap` map), tile→svc table, PC lock (svc 22, loginctl) and Hotspot (svc 13, PC-chosen SoftAP + nmcli); daemon-owned map with helper overrides (cross control in-use bit no longer hides other tiles)
- [display-screen-capture.md](display-screen-capture.md) — phone-as-display (svc 202) PC capture: niri Mutter ScreenCast by connector (no picker), portal fallback w/ restore token, Extend via vkms Virtual-1 on/custom-mode/off, why `vapostproc` (niri casts DMA-BUF only); output-position pinning fix; logical vs pixel stream size
- [open-source-publication.md](open-source-publication.md) — MIT license, name + disclaimer, CLAUDE.md vs git-ignored CLAUDE.local.md, hardened .gitignore, docs layout, pre-publish checks
- [phone-as-display.md](phone-as-display.md) — phone-as-display (svc 202) session: `smartconnect-display` mirror/extend/test-pattern CLI, cmd 0/5/1/2/70/26/52 flow, webrtcbin sendonly answer (the `a=inactive` race), raw SCTP tap for string-PPID input packets, source swap on resize; keyboard button → wvkbd on-screen keyboard (cmd 62); `--display-scale` (niri output scale, default 200 %)
- [webcam.md](webcam.md) — phone camera as PC webcam (svc 3): `smartconnect-webcam` WebRTC answerer → v4l2loopback (virtual devices only) or PipeWire camera node; phone-tile vs PC-initiated handshake, heartbeat, why `/readyfor/webcam` is irrelevant, pipewiresink buffer-pool stall; camera-flip fix (repeated offers kept, not re-answered); `--quality` bitrate presets
- [user-documentation.md](user-documentation.md) — README/docs/CONTRIBUTING/CHANGELOG set: which page owns which fact (status matrix, control-socket API from control.rs, binary lookup, file permissions), why security gaps are documented, how Arch package names were verified; the final pre-publication docs pass (matrix re-cut, no 🚧, CLI tables from `--help`)
- [security-hardening.md](security-hardening.md) — opt-in `--session-log` with redaction of keys/tokens/passwords/clipboard/SMS, 0700 state dir + 0600 files (QR, identity, endpoints), `pcAddress` replaces the hard-coded 192.168. ICE filter
- [notification-background-actions.md](notification-background-actions.md) — notification action buttons run without a window: what "stream up" means on the phone (cmd 4 → display + app, intent fires with cmd 13), daemon local event `watch` svc -1 `notification_action` (control-socket API), headless host sessions closed after cmd 13
- [phone-view-invites.md](phone-view-invites.md) — Screen share / Mobile desktop tiles: `--extra-tiles 1` (svc 25 cap), phone svc 1 cmd 0 → host cmd 1 `{view, reply:1}` → `mirror`/`desktop`/`mobile` streams, `smartconnect-mirror --desktop`
- [ice-candidate-filter.md](ice-candidate-filter.md) — mirror/host ICE candidates: `SMARTCONNECT_ICE_ADDRESSES`, else the daemon's `pcAddress`, else private IPv4 minus virtual interfaces (`ip -j`); UDP only
- [display-settings-and-phone-start.md](display-settings-and-phone-start.md) — phone as display: shared `~/.config/smartconnect-linux/display.json` (GUI-written defaults, flags override), daemon starts `smartconnect-display` on phone svc 202 cmd 50, single-instance lock + `--replace`, cmd 26 so a failed start never leaves the phone waiting, display ICE candidate filter
- [display-rotation.md](display-rotation.md) — phone as display `--follow-rotation`: cmd 7 (1 landscape / 2 portrait), extend output turned in place (custom mode, `transform 90` fallback, output never switched off), portrait cap 1920, input retargeting; cmd 70 uses the same in-place path
- [display-audio.md](display-audio.md) — phone as display `--audio`: default output's monitor → Opus on the offer's recvonly audio section, cmd 54 mute/unmute, cmd 27 when no audio is sent; preflight so audio can't take video down
- [display-frontend-ui.md](display-frontend-ui.md) — phone-as-display controls in the GTK app (Display page) and Noctalia plugin (panel button, settings, `display` IPC): shared `display.json`, detached sessions found via /proc/pgrep + a shared `pid N` log, `--help` flag probing, Apply restart, offline test recipe.
- [messaging-notification-avatar.md](messaging-notification-avatar.md) — why WhatsApp notifications showed your own picture (messaging `person_icon` is the MessagingStyle user = phone owner; no per-sender or large icon on the wire); `sender_avatar` (use it only when `person_name == title`, callers always) and app icon from the icon cache → themed → `apk_icon`
- [notification-mute-ui.md](notification-mute-ui.md) — per-app notification mute in both front-ends: per-notification toggle, Muted apps list with Unmute, `notif_mute` replies carrying the whole list.
- [packaging-and-workspace.md](packaging-and-workspace.md) — one Cargo workspace (merged lock, root-only profiles, zbus 5.19 for unified tokio/async-io features), helper lookup `SMARTCONNECT_<NAME>_BIN` → next to the executable → `PATH` (no build-dir paths), `packaging/install.sh`/`uninstall.sh` (plugin copy or `--link`, `--enable`, `--dry-run`), launcher + icon, Arch PKGBUILD
- [publication-audit.md](publication-audit.md) — pre-publication audit: privacy scan of every publishable file (fixes made), the WIP commit's leftovers, dependency licenses (all permissive), file modes, and the exact steps before the first push (fresh orphan `main`, `+x` on the install scripts, repository URLs)
- [noctalia-panel-redesign-and-reconnect.md](noctalia-panel-redesign-and-reconnect.md) — panel: compact header, 2×2 action tiles with option chips, content-sized height; fresh-Socket reconnection (Quickshell sockets never redial), singleton not reloaded by plugin toggle
- [gui-tray-and-lifecycle.md](gui-tray-and-lifecycle.md) — GTK app: ksni StatusNotifierItem tray (state icon + Show/Hide/Connect/Disconnect/Quit), window close hides to tray, open starts the daemon (systemd unit or child) with a Starting… state, Quit stops it, "Start at login" autostart entry + `--background` flag
- [gui-files-page.md](gui-files-page.md) — GTK app Files page: mounts the phone's storage via `smartconnect-files` (Mount/Open in Files/Unmount; labels per gtk-ui-redesign.md), tracked foreground child logging to a file, mount-point detection + grace period, error banner from the log tail, configurable folder in `gui.json` (default `~/Phone`), Quit unmounts (close-to-tray keeps it)
- [files-mount-resilience.md](files-mount-resilience.md) — `smartconnect-files mount` survives phone reconnects/daemon restarts (re-handshake supervisor, EIO while down, inodes by (parent,name) + epoch re-LOOKUP, 10-min give-up), clean unmount on SIGINT/TERM/HUP, stale-mount detection via `statfs` (plain `stat` is served from cache) in the helper, GTK Files page (live/offline/stale/unresponsive probe, remount-then-open) and Noctalia tile (probe script, adopt/clear)
- [noctalia-phone-files-tile.md](noctalia-phone-files-tile.md) — Noctalia panel Phone files tile: mounts the phone's storage via `smartconnect-files` as a tracked Quickshell Process (running≈mounted), `mountpoint` check + grace fallback for Mounting→Mounted, chip Open (xdg-open) / Unmount (`fusermount3 -u`), default ~/Phone + settings; no auto-unmount on close (stale-mount caveat)
- [file-access-and-clipboard-images.md](file-access-and-clipboard-images.md) — RE of the three file-moving subsystems (tunnelled NFSv3, Share Hub svc 30/168, fex svc 2); the LAN plain-TCP/mutual-TLS paths that avoid the proprietary tunnel; what was documented then (the fex part is superseded by fex-app-icons.md); daemon now recognises image/file clips
- [pc-to-phone-file-sharing.md](pc-to-phone-file-sharing.md) — `smartconnect-files serve`: the phone browses a PC folder (default: home, hidden files invisible) in Smart Connect's Files tab and Android's file browser; verified on a phone after device-found fixes (64-byte handles, cmd 2006→2007, remembered fingerprints); a record router answering Lenovo NFS procedures 22/23/27 (thumbnails, category listings, delete-to-Trash); tiles "206"/"20"; firewall zone; GTK *Share with the phone* section
- [fex-app-icons.md](fex-app-icons.md) — the phone's fex LAN transfer recovered live (HTTPS + pinned self-signed cert, `/websocket` action/ack text frames, `/download` zip, `status` types 0/1/2) and the `fex/` crate: `smartconnect-fex` CLI + daemon auto-download of svc 17 app icons into the icon cache (atomic, lock-coordinated); reusable for clipboard images
- [pc-lock-confirmed-locking.md](pc-lock-confirmed-locking.md) — PC lock tile reports "locked" only once `LockedHint` confirms it; Noctalia IPC / `--lock-command` / logind on the graphical session; failure notification; why GNOME under SDDM can't lock (GNOME Shell's lock screen needs GDM)
- [cross-control-gnome-portal.md](cross-control-gnome-portal.md) — crosscontrol split into core + layer-shell + InputCapture-portal (libei) backends for GNOME, `--backend auto`; `"202"` kept out of the svc 25 cap (it made the merged tile jump into Extend)
- [display-any-desktop.md](display-any-desktop.md) — phone-as-display on GNOME, KDE, Hyprland, sway and niri: per-desktop capture/extend/input backends, --list-outputs
- [cross-control-modes-page.md](cross-control-modes-page.md) — Cross Control modes at the top of the GTK Display tab (merged 2026-10-25): the three Windows modes (shared mouse/keyboard, Duplicate, Extend via the Display page), drag-the-phone position editor saved to `crosscontrol.json` (edge + span) that crosscontrol reads; tile tap brings the page forward
- [appimage.md](appimage.md) — AppImage for any distribution (Arch + quick-sharun, everything bundled), first-run Set up window, --install/--uninstall, pairing-window firewall hint

- [mirror-playback-recovery.md](mirror-playback-recovery.md) — Phone-to-PC receive recovery after packet loss, bounded decoded-frame queue, playback diagnostics and synthetic RTP regression tests.

- [gui-language-switch.md](gui-language-switch.md) — GTK language selector, built-in English/Simplified Chinese translations, locale resolution, preference persistence and restart-to-apply flow.

- [mobile-desktop-page.md](mobile-desktop-page.md) — Phone mobile desktop UI, saved resolution presets/custom sizes, capability-checked host controls, CLI --size and reported stream dimensions.

- [gui-window-close.md](gui-window-close.md) — Nonblocking, coalesced tray updates for main-window close/reopen responsiveness, with stalled-service and GTK regression tests.

- [notification-counts.md](notification-counts.md) — Distinguish unread SMS/MMS from forwarded app notifications; match the Phone shortcut and notification-list count after local dismissals.

- [mirror-video-stalls.md](mirror-video-stalls.md) — metadata-only video/input progress, bounded keyframe retries after decode/input stalls, and lost-feedback media regression.


- [mirror-udp-buffer.md](mirror-udp-buffer.md) — high-resolution burst loss, per-libnice-socket receive buffers, kernel-limit diagnostics and socket integration tests.

- [desktop-app-scaling.md](desktop-app-scaling.md) — independent desktop app density, default monitor scaling, restart-to-apply controls and density diagnostics.

- [notification-device-debug.md](notification-device-debug.md) — ADB diagnosis of allowed-app sharing, source-app permissions, and list-only synchronization under KDE Do Not Disturb.

- [mirroring-log-viewer.md](mirroring-log-viewer.md) — bounded persistent diagnostics, shared redaction/storage, live GUI search/export and offline CLI log viewing.

- [app-custom-resolution.md](app-custom-resolution.md) — independent custom app geometry/DPI, automatic defaults, per-launch queue isolation, GUI controls and actual-size diagnostics.

- [mirror-ice-path-timing.md](mirror-ice-path-timing.md) — identify Bluetooth PAN video bottlenecks, bind ICE gathering to selected addresses, and expose bounded pre/post-jitter metadata.

- [stream-quality-native-fit.md](stream-quality-native-fit.md) — independent app/desktop quality caps, fixed app ratios, physical-pixel fill calculations and XWayland aspect/native-window hints.

- [scrcpy-existing-desktop.md](scrcpy-existing-desktop.md) — real-device validation of scrcpy capturing a complete Smart Connect desktop, owner-session lifetime and local hardware-decode behavior.

- [scrcpy-desktop-integration.md](scrcpy-desktop-integration.md) — host-owned scrcpy desktop backend, authorized ADB selection, windowless owner stream, asynchronous startup and linked cleanup.

- [scrcpy-app-fullscreen-i18n.md](scrcpy-app-fullscreen-i18n.md) — fullscreen PC-matched desktops, scrcpy app sessions with native fit and isolated errors, and four bundled UI languages with English default.

- [scrcpy-fold-cancel.md](scrcpy-fold-cancel.md) — folded Android capture power recovery, owned helper lifetime, cancel-before-offer controls and late-reply suppression.

- [unicode-title-adb-wake.md](unicode-title-adb-wake.md) — UTF-8 X11 captions for KDE and cancellable optional ADB wake/keyguard preparation before app/desktop creation.

- [scrcpy-folded-input.md](scrcpy-folded-input.md) — noninteractive mirror input diagnosis, source-bound UHID mouse selection, unchanged SDK keyboard and relative-capture controls.

- [clipboard-image-validation.md](clipboard-image-validation.md) — strict incoming clipboard-image metadata validation (upstream PR #6).
- [usb-cable.md](usb-cable.md) — native USB cable discovery, handshake and optional NetworkManager profile (upstream PR #8).
- [mirror-rotation.md](mirror-rotation.md) — native rotation and source-coordinate input handling, adapted for GPU rendering and fixed layouts (upstream PR #9).

- [upstream-pr-integration.md](upstream-pr-integration.md) — local absorption of PRs #6–#10, conflict resolution, SMS/USB safety adaptations and validation.

- [scrcpy-input-settings.md](scrcpy-input-settings.md) — child-scoped relative mouse speed and independent source-bound keyboard for the phone IME.

- [scrcpy-computer-ime.md](scrcpy-computer-ime.md) — optional direct computer composition and session-bound Unicode delivery from scrcpy SDL3 windows.

- [scrcpy-audio.md](scrcpy-audio.md) — remove forced no-audio, control app/desktop audio and optionally select playback capture for independent PC volume.

- [vibe-release.md](vibe-release.md) — upstream attribution, v0.2.0 versions, public fork release, native Arch packaging and Chinese install guide.

- [app-opening-cancel.md](app-opening-cancel.md) — cancellation action in the Opening toast and pending-start lifetime cleanup.

- [builtin-audio-reception.md](builtin-audio-reception.md) — preserve audio SDP and receive offered Opus independently of the unresolved native audio control service.

- [native-audio-protocol.md](native-audio-protocol.md) — statically recovered service-21 commands, routing prerequisites and remaining live-handshake gap.

- [files-navigation.md](files-navigation.md) — file-tools landing page and persistent storage/share secondary page.

- [adb-file-transfer.md](adb-file-transfer.md) — independent USB/network ADB file tool, shared-storage navigation, asynchronous staged transfers, checkbox/clear selection, cancellation and collision protection.

- [notification-card-streaming.md](notification-card-streaming.md) — clickable notification summaries launch their corresponding app through the saved app-streaming workflow, keeping replies and menu actions independent.

- [scrcpy-desktop-hover.md](scrcpy-desktop-hover.md) — native top-edge hover supplement for the owned scrcpy desktop window, preserving auto-hidden app captions and fullscreen behavior.

- [scrcpy-desktop-power.md](scrcpy-desktop-power.md) — desktop-group capture isolation, session-private launcher around the unchanged installed scrcpy server, and physical sleep/wake validation.

- [scrcpy-app-power.md](scrcpy-app-power.md) — reuse isolated capture and guarded physical double-tap wake for app sessions, with tagged-name/source validation.

- [v0.3.0 release](release-0.3.0.md): version alignment, tested Arch packaging and documented streaming/audio limitations.

- [messages-read-status.md](messages-read-status.md) — explicit incoming read indicators and bounded phone-confirmation handling, with no recipient receipt inference.

- [main-window-actions.md](main-window-actions.md) — header Quit action and removal of application-restart controls.

- [sms-actions-and-adb-discovery.md](sms-actions-and-adb-discovery.md) — stronger unread styling, bulk read actions, dialog close, phone write-operation diagnosis and paired-address ADB rediscovery.

- [adb-gallery.md](adb-gallery.md) — read-only paged albums, phone-side bounded thumbnails/previews and explicit original downloads.

- [gallery-performance.md](gallery-performance.md) — amortize Android process startup per page, bounded thumbnail frames and a refresh-safe RAM LRU inspired by open-source image loaders.

- [sms-unread-refresh.md](sms-unread-refresh.md) — event-triggered unread metadata refresh and SMS/MMS scope alignment; no polling timer.

- [release-0.4.0.md](release-0.4.0.md) — gallery/SMS release, restart removal, obsolete-file cleanup and artifact validation.

- [bluetooth-pan-coexistence.md](bluetooth-pan-coexistence.md) — BLE cleanup ownership/shared-profile guards and persistent PAN configuration without adapter resets.

- [wifi-over-pan-addresses.md](wifi-over-pan-addresses.md) — interface-aware invitations, endpoint cache filtering, control-route preference and ADB target consistency without dropping PAN.

- [app-native-fit-fullscreen.md](app-native-fit-fullscreen.md) — true fullscreen for app fits matching the launch monitor, preserving native pixels and avoiding KDE work-area shrinkage.

- [scrcpy-video-encoding.md](scrcpy-video-encoding.md) — per-mode codec/encoder selection, validation, translated controls and scrcpy arguments.

- [release-0.5.0.md](release-0.5.0.md) — video encoding, native fullscreen and LAN/PAN coexistence release validation and publication.
