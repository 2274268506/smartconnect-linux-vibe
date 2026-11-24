# GTK interface language

The app's device-menu Preferences dialog now offers Follow system, 简体中文 and English.
`gui/src/i18n.rs` resolves the saved `Language` at startup. Explicit choices override locale
variables; system mode checks LC_ALL, LC_MESSAGES and LANG in order. Unknown saved language
names fall back to system mode so newer settings remain loadable by this version.

Static UI translations are embedded from `gui/src/zh_cn.json`. Missing entries fall back to
English. Dynamic UI messages use `tr_format!` with English and Chinese literals at the call site,
so Rust checks both sets of format arguments. Protocol tokens, paths and phone-supplied content
are left unchanged. The Noctalia plugin has a separate UI and is outside this change.

Preferences saves only the language field atomically, preserving other and future GUI settings
and leaving shared display.json untouched. Display edits load fresh GUI settings before saving,
so a pending language choice cannot be overwritten by the page's cached preferences.

Restart to apply launches the same executable after the current GUI process exits, using quoted
positional shell arguments, and quits the application without the tray Quit action's daemon stop.
Normal GUI shutdown still cleans up GUI-owned helper sessions and mounts. A failed save restores
the persisted selection; reopening Preferences keeps a pending restart available.

Tests cover locale precedence, explicit overrides, fallback, translated text, serialization and
preserving unrelated preferences. A separate desktop-session GTK test opens Preferences in both
languages, changes the language, then changes display FPS and verifies that the choice survives.

Earlier iterations defaulted new preferences to Chinese. The current default is English, with
French and Spanish added alongside Chinese and system mode. Startup still initializes translations
once before building widgets. See [the extension note](scrcpy-app-fullscreen-i18n.md) for current
catalog formatting, coverage and runtime validation.
