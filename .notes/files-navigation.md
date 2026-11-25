# Files landing page and secondary controls

## What and why

Move the entire existing Files interface into Storage and sharing, leaving a first-level file-tools page for future management features. Do not invent disabled features or split the existing controls across separate implementations.

## How

`files_navigation` wraps the existing scroller in an adw NavigationPage and ToolbarView/HeaderBar. A first-level ActionRow pushes the registered detail page. The automatic header Back button returns home. The existing FilesPage and ShareSection instances remain alive; all mount/open/unmount/quick-folder/location/auto-mount/share callbacks, status updates and Quit cleanup retain their original ownership. All new labels have English/Chinese/French/Spanish translations.

## Validation

Workspace check, 54 GUI tests and the isolated desktop navigation check passed before deployment. The GUI was updated with preference bytes preserved; daemon/stream host were not restarted. The navigation check uses a synthetic content widget and verifies repeated push/pop retains its parent, without mounting storage or sharing real files.
