# Notification card app streaming

## What and why

Make the GTK notification summary (icon, heading and body) a focusable button that opens its corresponding app stream. Previously cards were non-activatable and opening required the menu's phone-side notification intent. A separate card action gives predictable access to the configured app renderer without depending on the phone inviting a stream for that intent.

## How

`window.rs` resolves current notification data by its stable row key at activation, checks connectivity, and calls `AppsPage::open_notification`. Reply inputs, Send and the menu remain outside the card button, so using them does not trigger streaming. Notification body text is a normal label inside the button so a body click activates the card. Missing-package or disconnected cards are disabled.

`apps.rs` uses the cached App entry when available (retaining component/profile metadata) or a package/name fallback if notifications arrive before the launcher list. It reuses the existing tile launch workflow, saved renderer/settings, recents, pending deduplication, progress toast and Cancel. The actual component/profile behavior remains that of the existing launcher. The clicked button supplies the monitor anchor for fixed-aspect sizing.

This opens the app launcher, without promising a notification-specific deep link. The original Open on phone menu action still forwards the notification intent. Desktop popups, DND and phone-side permissions are unchanged. No notification contents or identifiers are logged.

## Validation

A pure regression test verifies known-component resolution, Unicode fallback names and missing identifiers. An optional isolated GTK test checks card structure, independent reply/menu controls, preserved drafts and connection/package guards without launching a phone application. Record execution results after running the checks.

Verified: GUI package check passed without warnings; release GUI suite passed (61 tests, with optional desktop/phone tests excluded); the new isolated GTK notification-card regression passed. A real notification-driven phone stream was not opened automatically during validation.
