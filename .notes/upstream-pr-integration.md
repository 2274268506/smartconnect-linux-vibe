# Open PR integration

## Scope and provenance

All five open PR heads were fetched and merged on an isolated local integration branch, after preserving the workspace's existing customizations. Only the reviewed integration delta was applied back; the original working branch and AGENTS.md were preserved. No remote PR was merged or published.

| PR | Contribution | Head |
|---|---|---|
| [#6](https://github.com/rudrahnpatel/smartconnect-linux/pull/6) | Reject incomplete clipboard-image metadata | f36a1707d43b116bec55e1b7c695804db6ad645d |
| [#7](https://github.com/rudrahnpatel/smartconnect-linux/pull/7) | Native Messages page | 3e2b4c948676937238e1e833b8d1d9279a466957 |
| [#8](https://github.com/rudrahnpatel/smartconnect-linux/pull/8) | USB cable discovery, handshake and profile | 9f786f537e954b829fdbcc48356f93100b61572f |
| [#9](https://github.com/rudrahnpatel/smartconnect-linux/pull/9) | Native display rotation/input coordinates | 7fa645f10583de57e34bf5a3a18be2945218ba3c |
| [#10](https://github.com/rudrahnpatel/smartconnect-linux/pull/10) | XVideo focus-loss workaround documentation | 7072f60f8c50a47c43daa112ecbfea3ba5f3935c |

## Adaptations and constraints

Messages integrates all four languages, preserves failed drafts, suppresses duplicate sending, reports handoff rather than delivery, parents dialogs, clears private content on disconnect, and renders phone text literally. A cache-only control flag permits bounded local reads after one explicit sync, with capability-based fallback for old daemons; it does not restore removed periodic phone polling. Read marks preserve message kind. No real SMS is sent by tests.

Rotation uses glvideoflip and GPU conversion on the default GL path; legacy sinks retain videoflip. Source dimensions determine orientation, explicit fixed layouts preserve geometry, complete desktops keep their configured orientation and scrcpy is not rotated twice. Existing receive recovery, FPS limiting, input handling, ADB preparation/cancellation and folded-mouse support remain intact.

USB subprocesses are bounded and raw protocol frames are not logged. Only confirmed recent cable handshakes suppress LAN endpoint learning, so ordinary USB Ethernet interfaces retain reconnect behavior. The user-owned RNDIS profile remains optional, respects existing profiles and never takes default routes. Upstream cable hardware validation is recorded in the original note; the current machine's hardware path still needs a real cable test. The XVideo change is documentation, not an automatic renderer switch.

## Validation

- `cargo check --workspace --locked`, `cargo build --release --workspace --locked` and `cargo test --release --workspace --locked` passed; the workspace test run passed 306 tests. The TLS cable handshake test uses an in-process simulated peer, not physical USB hardware.
- Five additional desktop checks passed: private Messages preview cleanup, four-language settings, desktop resolution settings, custom app geometry, and GL output-cap dimensions for all four rotations. Desktop checks used isolated configuration and cache directories.
- Deployed workspace release binaries and restarted the daemon/GUI. The phone reconnected and GUI preference bytes were preserved. The optional NetworkManager profile was present with both default-route policies disabled; no physical USB cable connection was tested.
- Native GL frames rendered, a per-virtual-display rotation event reached the host, and the window changed to landscape. However, a compositor screenshot of the forced calculator rotation on the current foldable showed distorted/clipped content. Correct visual rotation on this device remains unresolved; synthetic output-cap checks do not establish correct pixels or input alignment. The rotation request was reset and the test source closed. The upstream device verification must not be generalized to this phone. scrcpy folded-mouse selection and owned process/source cleanup passed independently.
- No real SMS was sent or real conversation opened. Actual SMS delivery, physical USB operation and complete native rotated-picture/input correctness remain unverified. Screenshots, logs and device details remain outside the repository.
