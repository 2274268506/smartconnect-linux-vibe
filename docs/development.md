# Development

How the repository is organised, how to build and test each part, how helpers implement phone
services through the daemon, how to add a service, and how to debug. Read
[Architecture](architecture.md) first for the big picture, and [CONTRIBUTING.md](../CONTRIBUTING.md)
for the documentation and privacy rules every change follows.

- [Repository layout](#repository-layout)
- [Building and testing](#building-and-testing)
- [Running things locally](#running-things-locally)
- [How helpers implement services](#how-helpers-implement-services)
- [Adding a service](#adding-a-service)
- [Protocol pitfalls](#protocol-pitfalls)
- [Debugging](#debugging)

## Repository layout

```
smartconnect-linux/
├── src/                  smartconnectd: daemon + CLI (binary), library crate `smartconnect`
│   ├── main.rs           options, subcommands, task startup
│   ├── server.rs         TLS/WebSocket server, session loop, shared Daemon state, reconnect
│   ├── control.rs        control socket ($XDG_RUNTIME_DIR/smartconnectd.sock)
│   ├── discovery.rs      mDNS (avahi), /readyfor/mdns discovery frames, connect_now
│   ├── ble_connect.rs    Bluetooth LE wake (GATT central), adapter power watcher (read-only)
│   ├── reconnect.rs      auto-reconnect loop, phone presence probe, resume/network watcher
│   ├── ble.rs            experimental BLE peripheral (disabled with --no-ble)
│   ├── identity.rs       ids, self-signed certificate, fingerprint
│   ├── invite.rs         pairing token and QR payload
│   ├── pairing.rs        paired-phone store (paired.json)
│   ├── proto.rs          envelope, advertised serviceVersions / caps / tile cap map
│   ├── clipboard.rs      service 9 + wl-clipboard
│   ├── notifications.rs  services 5/37 → org.freedesktop.Notifications, reply window
│   ├── status.rs         phone status for front-ends
│   ├── sms.rs            service 204 (+ 100)
│   ├── pc_lock.rs        service 22
│   ├── hotspot.rs        service 13
│   ├── webcam.rs         service 3 request detection (the media lives in webcam/)
│   ├── app_icons.rs      service 17 app list → background icon download (the client lives in fex/)
│   └── helpers.rs        supervision of the stream host, Cross Control, display and webcam helpers
├── tests/handshake.rs    plays the phone's side of pairing against an in-process daemon
├── mirror/               smartconnect-mirror: webrtcbin viewer, stream host, app list
├── crosscontrol/         smartconnect-crosscontrol: Wayland edge strip + pointer lock
├── display/              smartconnect-display: phone as display (capture, session, input)
├── webcam/               smartconnect-webcam: phone camera as webcam
├── files/                smartconnect-files: phone storage as a FUSE mount, and serve (a PC
│                         folder for the phone), both NFSv3 over mutual TLS
├── fex/                  smartconnect-fex: client for the phone's fex transfer server (lib + CLI)
├── gui/                  smartconnect-gui: GTK4/libadwaita app
├── noctalia-plugin/      Noctalia Shell plugin (QML)
├── desktop/              reply.qml (embedded into the daemon); mirror_prototype.py (old Python
│                         prototype of the viewer, superseded by mirror/)
├── packaging/            install.sh, uninstall.sh, systemd user unit, launcher + icon,
│                         arch/PKGBUILD
├── docs/                 user and developer documentation; docs/protocol/ = wire protocol
├── .notes/               design log: what / how / why for each change (index.md)
└── CLAUDE.md             the project rules (documentation + privacy), also for coding agents
```

## Building and testing

The repository root is a Cargo workspace. The daemon is the root package; the helpers and the GTK
app are members. They share one `Cargo.lock` and one target directory:

| Package | Directory | Binary | Build-time system libraries |
|---|---|---|---|
| `smartconnect-linux` | `.` | `smartconnectd` | `dbus-1` (BlueZ via `bluer`), a C compiler (`ring`) |
| `smartconnect-mirror` | `mirror/` | `smartconnect-mirror` | GStreamer core, base (video, gl, sdp), bad (webrtc) |
| `smartconnect-crosscontrol` | `crosscontrol/` | `smartconnect-crosscontrol` | `wayland-client` |
| `smartconnect-display` | `display/` | `smartconnect-display` | GStreamer (webrtc, sdp), `wayland-client` |
| `smartconnect-webcam` | `webcam/` | `smartconnect-webcam` | GStreamer (webrtc, sdp) |
| `smartconnect-files` | `files/` | `smartconnect-files` | a C compiler (`ring`). `fuser` is built without default features, so no libfuse headers; `fusermount3` (fuse3) is needed at run time only |
| `smartconnect-fex` | `fex/` | `smartconnect-fex` (and the `smartconnect_fex` library the daemon links) | a C compiler (`ring`) |
| `smartconnect-gui` | `gui/` | `smartconnect-gui` | GTK 4.18+, libadwaita 1.7+ |

```sh
cargo build --release --workspace            # everything
cargo test --release --workspace
cargo clippy --release --workspace
cargo build --release -p smartconnect-mirror # one package (or run cargo inside its directory)
```

- Keep every build **warning-free**.
- Rust 1.88 or newer (edition 2024, let-chains; `gui/` is edition 2021).
- Profiles live in the root `Cargo.toml` only (cargo ignores them in members): `rsa` is optimised
  even in debug builds, and `smartconnect-gui` is stripped in release builds.
- `cargo build` in the root without `--workspace` or `-p` builds only the daemon.
- Shared dependencies get the union of the features the members ask for. For example zbus is built
  with both the daemon's `tokio` backend and the display helper's `async-io` one; zbus 5.19 and
  later picks the backend at run time (tokio only inside a tokio runtime), which is why both
  manifests require `5.19`.
- All eight binaries land next to each other in `target/release/` (or your `CARGO_TARGET_DIR` /
  `.cargo/config.toml` `target-dir`). The daemon and the GTK app look for helpers next to
  themselves before `PATH`, so a daemon or GTK app started from there uses the helpers of the same
  build without installing anything. `SMARTCONNECT_<NAME>_BIN` points one of them at another file
  (see [how the programs find each other](installation.md#how-the-programs-find-each-other)). Never
  hard-code a build directory in code or defaults.
- A git-ignored `.cargo/config.toml` is a good place for a per-checkout `target-dir`, for example
  when the source tree sits on a slow filesystem.
- `packaging/install.sh --link` installs a build and links the Noctalia plugin to the checkout;
  `--no-build` reinstalls without running cargo; `--dry-run` shows what it would do.
- The first daemon start generates an RSA-2048 key. The `rsa` profile override above is why that
  doesn't take minutes in a debug build.

What the tests cover:

| Where | What |
|---|---|
| `tests/handshake.rs` | An in-process daemon on a random port, driven like the phone drives it: parse the QR, pin the certificate by fingerprint, upgrade `/readyfor/`, send the hello, check the verify reply and the service 25 tile message, the key exchange, clipboard negotiation, a wrong token being dropped, the token staying valid for its lifetime plus reconnect without a token, and the Connect-button trigger on `/readyfor/mdns`. |
| `src/*.rs` unit tests | Discovery frame layout (a known-answer vector), avahi output parsing, clipboard negotiation, echo suppression and truncation, notification bodies, fingerprints and icon names, Hotspot and PC lock message shapes, SMS request shapes and thread grouping, helper binary lookup and lock detection. |
| `display/` | Unit tests for input packet decoding, the touch gesture state machine and output parsing. Live tests are `#[ignore]`d. `capture_smoke_*` and `input_smoke_extend` run on whatever desktop is running (`cargo test --release -p smartconnect-display capture_smoke -- --ignored --nocapture --test-threads=1`); `gnome_smoke_*`, `hyprland_smoke_*` (works against a nested Hyprland: set `HYPRLAND_INSTANCE_SIGNATURE` and `WAYLAND_DISPLAY`) and `niri_smoke_*` are per desktop, and `smoke_move_pointer` moves the real cursor and needs `CURSOR_LOCATOR` set to a cursor-finding script. |
| `mirror/` | Unit tests for offer trimming, invite handling (repeated, headless action clicks, phone-view invites), window resize requests and ICE candidate selection. |
| `webcam/` | Unit tests for the message shapes, offer parsing and trimming, candidate filtering and loopback device selection. |
| `gui/` | Unit tests for display options ↔ helper flags, `display.json`/`gui.json`, session log parsing, a stand-in display session (start, detect, stop) and the helper lookup order. |
| `crosscontrol/` | No automated tests yet. |

None of these replace a test against a real phone: say in the PR what you tried on a device.

## Running things locally

- **Daemon in the foreground:** stop the service (`systemctl --user stop smartconnectd`), then
  `RUST_LOG=debug cargo run --release --bin smartconnectd -- --no-ble`. It uses the normal state
  directory, so your existing pairing keeps working.
- **A throwaway identity:** `--state-dir /tmp/sc-test --name "Test PC"` makes the phone see a
  different PC with its own pairing. Use it to test pairing flows without touching your real
  pairing, and remove the extra entry on the phone afterwards.
- **Helpers by hand:** start the daemon with `SMARTCONNECT_HELPERS=0` so it doesn't start its own
  stream host, then run `cargo run --release -p smartconnect-mirror -- --host`. A Cross Control
  client started by hand takes over from a running one anyway.
- **GTK app:** `cargo run --release -p smartconnect-gui`. It connects to whichever daemon owns the
  socket.
- **Noctalia plugin:** install it with `packaging/install.sh --link` (or link it by hand, see
  [Installation](installation.md#noctalia-plugin)), so the plugin directory is a symlink into the
  checkout and editing the QML is enough. Reload the plugin to pick up changes. Its helpers are
  found on `PATH`; point its two binary settings at your build directory to test a build you
  haven't installed.

## How helpers implement services

A phone service doesn't need code in the daemon. The daemon relays (see
[the relay model](architecture.md#the-relay-model)):

- `{"cmd":"watch","services":[N]}` streams `{"data":…,"service":N}` lines for every message the phone
  sends on service `N`;
- `{"cmd":"send","service":N,"data":{…}}` sends `data` to the phone on service `N`, wrapped in the
  envelope.

So a helper is a process with two control-socket connections. This is the minimal shape (Rust std,
the same structure as `mirror/` and `crosscontrol/`):

```rust
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use serde_json::{json, Value};

const SVC: i64 = 42; // the service this helper implements

fn main() -> anyhow::Result<()> {
    let path = std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR")?).join("smartconnectd.sock");

    // 1. Subscribe first, so no answer from the phone is missed.
    let watch = UnixStream::connect(&path)?;
    (&watch).write_all(format!("{}\n", json!({"cmd": "watch", "services": [SVC]})).as_bytes())?;

    // 2. A second connection for commands. Drain its replies, or the socket buffer eventually fills up.
    let mut cmd = UnixStream::connect(&path)?;
    let drain = cmd.try_clone()?;
    std::thread::spawn(move || for _ in BufReader::new(drain).lines() {});

    for line in BufReader::new(watch).lines() {
        let v: Value = serde_json::from_str(&line?)?;
        if v["service"] != SVC {
            continue; // the first line is {"ok":true,"message":"watching"}
        }
        if v["data"]["cmd"] == 0 {
            let reply = json!({"cmd": "send", "service": SVC, "data": {"ver": "1", "cmd": 1}});
            cmd.write_all(format!("{reply}\n").as_bytes())?;
        }
    }
    Ok(()) // the daemon closed the stream (it stopped or restarted)
}
```

Notes:

- Every watcher sees every frame of its services, so another instance of your helper sees them
  too. Filter by session id where the protocol has one (`display/` does this), or make the helper
  single-instance (`crosscontrol/` uses a `flock`, `mirror --host` probes its socket).
- When the daemon restarts, the watch stream ends. Exit cleanly or reconnect.
- Small, stateless services that react to the phone can live inside the daemon instead. Subscribe
  to `daemon.from_phone` and answer on `daemon.to_phone`, as `pc_lock.rs` and `hotspot.rs` do, and
  start the task from `main.rs`.

## Adding a service

1. **Document the protocol first** in `docs/protocol/<service>.md`: commands, payload shapes,
   sequences, pitfalls, each fact tagged OBSERVED or INFERRED. Use placeholders for every value
   (see [CONTRIBUTING.md](../CONTRIBUTING.md#privacy-rules)).
2. **Choose where it runs.** Media, Wayland or GStreamer work goes in a helper crate. Small
   request/response logic goes in a daemon module.
3. **Implement it.**
   - In the daemon: a `run(daemon)` task on `from_phone`/`to_phone`, spawned in `main.rs`.
   - As a helper: a new crate using `watch`/`send` as shown above.
4. **Advertise it** in `src/proto.rs`, and only once the PC side works:
   - `service_versions()`: the phone only brings a service up if the PC lists it in the handshake
     reply.
   - `service_caps()`: per-service capability levels, where the protocol needs them.
   - `device_status_caps()`: only for services with a **tile** on the phone. A tile appears only
     if the service is in both `service_versions()` and this map. Every service 25 message replaces
     the whole map and the phone stores it, so the map must always be complete. The static map uses
     the value 1 ("on"). A helper that needs to flag its own tile (Cross Control marks it "in use")
     sends a service 25 `cap` object through the control socket, and the daemon merges it into the
     full map (see [`send`](control-socket-api.md#send)).
   These take effect on the next connection. `tests/handshake.rs` checks the messages sent right
   after the handshake, so update it.
5. **If the phone can start the feature** (a tile tap), make sure something answers. Add
   supervision to `src/helpers.rs`, following `answer_cross_control` or `answer_webcam`: spawn the
   helper on the phone's request when none is running (check its lock first), and reply with an
   error if it can't be started, so the phone doesn't just time out.
6. **Front-end hooks:** new control commands go in `control.rs` (`json_command`) and must be
   documented in [control-socket-api.md](control-socket-api.md). GTK and Noctalia controls go in
   `gui/` and `noctalia-plugin/`.
7. **Tests:** unit tests for message shapes and parsing, plus a handshake test if the session
   changes.
8. **Documentation, in the same change:** a `.notes/<feature>.md` entry (what/how/why) plus a line
   in `.notes/index.md`, the feature's section in [usage.md](usage.md) with its verification
   status, the README feature matrix, and a line in `CHANGELOG.md` under `[Unreleased]`.

## Protocol pitfalls

Mistakes that cost the most debugging time so far. The full details are in the
[protocol pages](protocol/README.md).

- **The phone's JSON parsing is strict about types and fails silently.** Sending `0` where the
  phone expects a boolean makes its parser throw inside a background task. Nothing is logged, and
  the feature times out about 15 s later. Copy the exact types.
- **Some messages have lasting effects.** Clipboard `msgId` 101 disables the clipboard for this PC
  on the phone persistently. Never send clipboard PULL for text, msgIds 13–17 or
  `shareHubMountInfo`. A plaintext discovery frame (type 2) makes the phone unpair.
- **Tiles depend on the service 25 `cap` map**, not only on `serviceVersions`, and each message
  replaces the whole map.
- **Identity fields must be stable and non-empty.** An empty `sn` once made the phone's database
  refuse to commit the pairing. Recovering needed a new PC identity.
- **ICE:** offer only LAN host candidates. The phone tries every candidate you send, including
  Docker and VPN addresses.
- **Video:** the phone lists H.265 first. Trim the offer to H.264 before answering.
- **Stream instance id:** the phone drops all stream sessions when a start request carries a
  different `instanceId`. Every mirror process shares one id per login session.

## Debugging

| Tool | How |
|---|---|
| Daemon log | `RUST_LOG=debug` (or `RUST_LOG=smartconnect=debug,info`); `journalctl --user -u smartconnectd -f` under systemd. |
| Session logs | Start the daemon with `--session-log`: `~/.local/share/smartconnect-linux/sessions/*.jsonl` then gets every frame in and out of a session, with secrets and personal text redacted (`server.rs::redact`). [jq recipes](troubleshooting.md#where-the-logs-are). |
| Live traffic | `echo '{"cmd":"watch","services":[15]}' \| socat -t 86400 - UNIX-CONNECT:$XDG_RUNTIME_DIR/smartconnectd.sock` |
| Poking the phone | `send` on the control socket. Read [the warnings](control-socket-api.md#send) first. |
| Phone side | `adb shell setprop log.tag.MotoDesktopLog DEBUG`, restart Smart Connect, then `adb logcat` ([details](troubleshooting.md#phone-side-debug-logs)). This is how silent phone-side failures were found. |
| GStreamer | `GST_DEBUG='3,webrtc*:5'` on the viewer, display or webcam helper. The viewer also honours `MIRROR_DECODER` and `MIRROR_SINK`. |
| Helper output | Daemon-started helpers log to `$XDG_RUNTIME_DIR/smartconnectd/<name>.log`. Run them in a terminal for live output. |
| Phone display | `smartconnect-display --test-pattern` (no capture or input). `SMARTCONNECT_DISPLAY_DEBUG_RESIZE=1280x720` simulates a resolution request from the phone after 15 s, `SMARTCONNECT_DISPLAY_DEBUG_ROTATE=2` a turn to portrait (with `--follow-rotation`). The last SDP offer and answer are in `$XDG_RUNTIME_DIR/smartconnect-display.{offer,answer}.sdp`. To test the GTK app or the plugin without a phone, point `SMARTCONNECT_DISPLAY_BIN` (or the plugin's helper setting) at a stand-in script, as described in `.notes/display-frontend-ui.md`. |

Logs and captures from your own phone contain personal data. Never commit them or paste them into
issues unredacted (see [CONTRIBUTING.md](../CONTRIBUTING.md#privacy-rules)).

## Additional streaming and GUI regression checks

Video-stall recovery regression (local GStreamer encoder/decoder; no phone required):

```sh
cargo test --release -p smartconnect-mirror --locked playback::tests
```

This includes lost RTP plus suppressed keyframe feedback, a long encoder keyframe interval,
and verification that bounded retry restores decoding. Timing tests ensure static idle video
does not trigger recovery and repeated input cannot bypass the retry cap.

UDP receive-buffer integration tests create real libnice/GSocket objects locally:

```sh
cargo test --release -p smartconnect-mirror --locked receive_buffer::tests
```

The mirror links libnice directly; install its development/linker files when the distribution
separates them from the runtime plugin. The tests respect the host's `net.core.rmem_max`.

Desktop app-scaling controls can be checked with a desktop session and isolated
`XDG_CONFIG_HOME`/`XDG_DATA_HOME` (as in the other GTK tests):

```sh
cargo test --release -p smartconnect-gui --locked mobile_desktop::tests::app_scaling_saves_separately_and_requires_desktop_restart -- --ignored --exact
```

It verifies persistence, automatic monitor scaling, restart availability and isolation from
PC-to-phone Display preferences without starting a phone stream.

Mirroring diagnostic tests cover transport/address redaction, timestamp preservation, bounded
rotation with private permissions and text/severity filtering. Run the normal GUI/mirror suites.
The log-viewer integration test needs a desktop session and an isolated `XDG_STATE_HOME`:

```sh
cargo test --release -p smartconnect-gui --locked logs::tests::live_log_view_search_pause_resume_close_and_reopen -- --ignored --exact
```

It checks live updates, search, pause/resume, filtered export with private permissions, close
and reopen. Keep real logs and exported diagnostics outside the repository.

Custom app geometry tests cover dimension/DPI boundaries, CLI parsing, queue isolation,
automatic-mode defaults and raw preference-field preservation. The GTK dialog test requires
a desktop session and isolated `XDG_CONFIG_HOME`/`XDG_DATA_HOME`:

```sh
cargo test --release -p smartconnect-gui --locked app_resolution::tests::custom_app_geometry_validates_and_saves_without_changing_other_modes -- --ignored --exact
```

It validates presets, invalid-width Save disabling, DPI persistence and isolation from desktop,
display, language and recent-app preferences without launching phone apps.

App layout/quality regression coverage includes even-size physical fit calculations, unsupported
sizes, independent automatic bitrate/FPS, SDP media scoping, decoded-frame rate limiting and
queue isolation. The isolated app dialog test also checks fixed-ratio fit and oversized fits;
the desktop settings test checks separate quality persistence. Runtime measurements must
separate requested, received, decoded and presented rates; do not infer end-to-end latency
or encoder acceptance from saved preferences.

The scrcpy backend has regression tests for ADB parsing/selection, authorization/ambiguity,
argument mapping, bounded preparation cancellation, missing executables, first-frame detection
and owned-child reaping without stopping another process. Fake tools are temporary test scripts;
normal suites do not contact a phone. The existing isolated desktop settings GTK test now covers
backend/device/port/software-decode persistence and invalid settings. Actual-device checks should
verify one visible scrcpy window, no owner GL decoder, close/Stop cleanup, failure cleanup and
switching back to built-in mode. Raw logs/captures remain outside the repository.

The `i18n-macros/` workspace crate checks formatted translation arguments through Rust's `format!`. GUI `fr.json`, `es.json` and `zh_cn.json` are embedded catalogs; translation does not use a network service. Add English source keys and matching translated placeholders when changing UI text. Catalog tests check coverage and numbered dynamic placeholders; formatted values retain Rust precision and are evaluated once. New preferences default to English; existing explicit choices are retained.

Run `cargo test --release -p smartconnect-i18n-macros -p smartconnect-gui -p smartconnect-mirror --locked`. The three isolated GTK app-settings, desktop-settings and language-switch tests additionally cover scrcpy options and all four languages; run each with isolated XDG directories as described above. Real-device checks for this extension cover desktop fullscreen geometry, app native fit and quality flags, viewer-close cleanup, isolated app failures, and built-in app rendering.

Pending-start regressions cover queued cancellation without interrupting another app, rejecting late offers before SDP/pipeline creation, explicit retry, view stop messages, and cancellation before any host is started. Two isolated GTK tests exercise desktop/app Cancel before a session exists:

```sh
cargo test --release -p smartconnect-gui --locked mobile_desktop::tests::startup_cancel_is_available_before_a_session_and_clears_async_state -- --ignored --exact
cargo test --release -p smartconnect-gui --locked apps::tests::app_preparation_has_cancel_before_a_session_exists -- --ignored --exact
```

Use separate private `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` and **XDG_RUNTIME_DIR** for each case; the tests must not see the real mirror socket. Provide a desktop session (e.g. X11 via DISPLAY/GDK_BACKEND=x11). See `mirror/android/README.md` for regenerating the embedded helper. Real-device verification must confirm that the phone remains physically folded, the inner screen stays off, the capture produces visible content, and cancellation removes the source, viewer, helper and temporary upload.

ADB preflight regressions use fake tools and a controllable task factory: wake/dismiss/verify ordering without a scrcpy executable, protected-lock refusal, unknown-state refusal, cancellation interrupting an ADB check, no phone start before success, and error isolation. Normal tests never wake a real phone. Existing isolated app/desktop GTK settings tests additionally check the new switch, persistence, and ADB validation under the built-in renderer.

Unicode title properties have an X11 integration check:

```sh
cargo test --release -p smartconnect-mirror --locked tests::unicode_titles_preserve_utf8_in_both_x11_properties -- --ignored --exact
```

It creates only a temporary local window, checks UTF8_STRING and exact Chinese/emoji bytes on both title properties, and destroys the window. Real-device checks should additionally inspect a Chinese scrcpy app caption on KDE and confirm ADB preparation precedes phone stream setup. Protected-lock behavior is simulated in ordinary tests; do not inject credentials for testing.

Folded-mouse tests cover default/opt-out persistence, Android/scrcpy/source-version gating, unknown-SDK fallback, emitted UHID/SDK mouse arguments and independent UHID/SDK keyboard flags. The isolated app/desktop settings tests verify the switch saves independently. Real-device validation must keep the phone folded and exercise pointer movement and clicks that change app content, keyboard input, capture release, display association and cleanup; visible video alone does not prove input works.

The open-PR integration is recorded in `.notes/upstream-pr-integration.md`. New tests cover strict clipboard metadata, cable profile/invite policy, TLS cable handshake, confirmed-cable endpoint learning, Messages cache-only command selection and draft retention, and native rotation. Optional desktop tests include `messages::tests::disconnect_clears_message_previews_without_requesting_phone_data` and `playback::tests::gpu_rotation_negotiates_upright_dimensions_for_every_quarter_turn` (isolated XDG directories for GTK; `GST_GL_WINDOW=x11` for GL). SMS tests use synthetic data; do not send a real SMS or alter unread state to validate the UI. NetworkManager profile tests inspect arguments only and do not configure the real network.

Input settings regressions also cover old-preference defaults, strict 25–400% speed validation, independent phone-IME selection and child-local SDL sensitivity. The isolated app/desktop GTK checks exercise save/reopen and sensitivity when compatibility mode is disabled. For real-device verification, use a harmless text field rather than messaging, inspect source-associated UHID input, and verify candidate composition without recording private text.

`mirror/native/computer_ime.c` is an original SDL3 event adapter, compiled by `mirror/build.rs` when native SDL3 headers are available. `computer_ime` tests check Unicode ordering, malformed/oversize packet rejection, synthetic SDL Wait/Poll commits, exactly-once consumption and temporary-file cleanup. Isolated GTK tests verify app/desktop computer-mode persistence and phone-mode sensitivity. Real-device checks should compose harmless text in Settings, restore the computer IME state and close only owned test streams; never use a messaging send action.

Scrcpy audio regressions cover old settings defaulting to enabled, boolean validation, mutually exclusive output/no-audio arguments, absence of microphone/require-audio flags, and independent app/desktop GUI persistence. Device validation should check the owned scrcpy playback stream without recording private audio or changing system-wide volume/mute settings.

Independent audio tests cover output/playback/no-audio precedence, Android 13 SDK gating including unknown SDK failure, independent app/desktop persistence and disabling the control while audio is off. Device verification should compare ordinary media at phone media volume zero/nonzero without recording private audio; the shell tone helper used for earlier diagnostics did not establish a meaningful media signal. Do not treat an active desktop playback node as proof of audible volume independence.

## Vibe release packaging

All workspace packages and the Noctalia manifest use 0.3.0. Run the workspace release tests/build before packaging. Build the native Arch package with `CARGO_TARGET_DIR="$(pwd)/target" makepkg --dir packaging/arch --nocheck` only after those tests pass. The package is dynamically linked for current Arch x86_64, not a universal binary. Source archives must come from `git archive` of the release commit, excluding local AGENTS/settings/build output. Publish checksums alongside the native package and source archive; verify package contents and an extracted daemon version before publication.

The isolated apps::tests::app_preparation_has_cancel_before_a_session_exists test verifies the Opening toast Cancel action reaches the existing cancellation control before a session exists.

The h264_selection_preserves_audio_codec_and_parameters regression checks mixed video/Opus SDP. Separate native audio service dispatch remains unverified; never advertise it based solely on a working receive pipeline.

Files navigation: the optional desktop test `files::tests::navigation_retains_the_existing_content_when_returning_home` checks repeated push/pop keeps the same existing content widget parented. It constructs no mount/share controller and requires no phone, FUSE mount or daemon.


ADB transfer tests: `cargo test --release -p smartconnect-gui adb_transfer::tests` covers path validation, Unicode/NUL parsing, shell quoting, no-overwrite publication and cancellation of a running child. The ignored `adb_files::tests::idle_page_and_cancellation_controls` desktop test checks initial controls, checkbox deselection, native selection synchronization, Clear selection and cancellation without contacting a phone. The ignored `adb_transfer::tests::phone_roundtrip_preserves_names_and_existing_files` test requires an explicitly selected authorized device in `SMARTCONNECT_TEST_ADB_DEVICE`; it creates and cleans only a unique synthetic folder under `/sdcard/Download` and a temporary local folder. It checks Chinese/metacharacter filenames, content equality and collision protection. Never use personal files as fixtures.

Notification-card activation: `apps::tests::notification_target_preserves_known_component_and_handles_missing_launcher_list` checks cached launcher target resolution and package-only fallback. The optional desktop test `window::language_tests::notification_card_stream_target_preserves_reply_and_menu_controls` checks clickable content, independent reply/menu controls, draft preservation and disconnected/missing-package sensitivity using synthetic data with the app launcher disconnected. Run it with isolated XDG directories; it does not start a real phone app.

Desktop hover: `cargo test --release -p smartconnect-mirror desktop_hover::tests` checks scaled/letterboxed coordinates, rejecting out-of-video points, edge arming, caption-band movement, deduplication and exit/reset. The ignored `tests::desktop_hover_uses_only_the_uncovered_owned_client` test should run against an isolated X11 server (e.g. Xvfb); it moves only that server's pointer and verifies an overlapping window blocks forwarding. Real-phone validation must use a maximized app with the phone's normal auto-hidden caption, not an arbitrary activity forced into Android fullscreen via `am start --windowingMode 1`.


Desktop power launcher: regenerate its original DEX with `R8_JAR=/path/to/r8.jar sh mirror/android/build-capture.sh` (JDK 17+, R8 8.8.34 verified). The script runs pure Java scope tests and downloads nothing. Normal Cargo builds embed `capture-launcher.dex`; users need neither JDK nor Android SDK. Run `cargo test --release -p smartconnect-mirror` and the optional independent ZIP check `cargo test --release -p smartconnect-mirror capture_server::tests::independent_reader_checks_both_entries_crc_and_unchanged_server -- --ignored --exact` (Python 3). Device validation should inspect display groups, verify rendering/input, let the physical phone screen time out, test its physical double-tap wake, and check cleanup after closing the viewer. Avoid changing global sleep settings to make the test pass.

The capture launcher JVM tests also cover double-tap/fingerprint event separation, inner/outer sensor mapping, physical sleeping-state checks and enabled/active/proximity guards. Physical gesture validation must compare projection-active and projection-closed behavior; aggregate device interactivity alone does not indicate whether the physical display is asleep.

Shared app power isolation: `capture_server::tests::native_apps_and_desktop_opt_in_but_phone_views_do_not` checks host selection, and the launcher JVM scope tests check app/desktop names, hexadecimal session prefixes, malformed package/tag rejection and exact source matching. For device verification use a synthetic calculator app, inspect source/capture groups, let the phone sleep, and compare stream continuity and physical double-tap wake. No private app contents are required.

Physical wake diagnostics now include static rejection reasons (inactive source, disabled setting, uncleared proximity, or a physical display that is not asleep). Fresh display-service reads and per-display power-service gesture wake avoid the global interactive/wake-key path. Device checks must verify real gestures while the stream stays open, rather than closing the stream before reporting the result.
