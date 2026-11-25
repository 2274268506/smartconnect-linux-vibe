# ADB file transfer

## What and why

Add **Files → ADB file transfer** beside Storage and sharing. ADB is already used for scrcpy; an explicitly authorized debugging transport provides a practical bidirectional file tool while native File Station remains read-only. This is independent of native pairing, mounting and PC-folder exports.

## How

- `adb_files.rs` owns GTK device selection, an optional user-triggered network connection, folder navigation, multi-file dialogs, activity/completed-file progress and Cancel. Opening/refreshing the tool discovers devices; merely constructing it does no ADB I/O. Multiple devices require explicit selection.
- `adb_transfer.rs` pins each operation to a selected serial and runs CLI subprocesses off-thread. A bounded channel carries transient results to GTK. Cancellation/timeout kills and reaps the owned process group; the ADB server stays running. Quit requests cancellation without blocking the main loop.
- Listings use NUL-delimited paths, shell quoting, directory-first sorting and explicit `/sdcard` validation. Symlink entries are skipped. No listing is logged or persisted.
- Uploads use uniquely reserved staging directories and Android `mv -nT`; downloads use private directories and Linux no-replace rename. Existing files are protected. Batches stop at the first failure; completed files remain. Cleanup is best effort if connectivity is lost or the process is forcibly terminated.

## Scope and validation

Only regular files transfer; folder browsing does not imply recursive folder transfer. Progress indicates activity and file counts, not byte percentage. Wireless pairing remains an external `adb pair` step when required. Gallery metadata/thumbnails and native protocol writes remain future work.

Unit tests cover path validation, NUL/Unicode handling, shell metacharacters, atomic collision protection and process cancellation. An optional desktop test checks idle controls and Cancel with no ADB process. An opt-in phone roundtrip checks synthetic Unicode/metacharacter names, content equality and preservation of existing destinations; it touches only its unique owned fixtures. Validation results are recorded after execution.

Verified: workspace `cargo check --workspace --locked`; GUI release suite (60 passed, 13 optional tests ignored); both isolated GTK tests passed; the opt-in authorized-phone roundtrip passed, including Chinese/space/shell-metacharacter names and both upload/download collisions. No personal files were used.


## Selection correction

GTK multiple selection requires modifier keys to deselect a row; this was not discoverable in the initial file list. Add explicit checkboxes on file rows and a **Clear selection** button; Escape also clears selection when the list has focus. The checkboxes and GTK Ctrl/Shift selection synchronize in both directions. Folder rows are navigable without becoming selected, and single-click activation is explicitly disabled so folder activation follows the documented double-click behavior. The existing isolated GTK regression test exercises individual deselection, native synchronization and clearing all selected files, including Download sensitivity. This changes only selection UI, not transfer cancellation or transport.

Selection fix verified with the GUI release suite (60 passed) and the isolated GTK regression test (passed). Checkbox deselection and Clear selection both empty the selected-file set and disable Download when appropriate.
