# ADB photo gallery

## What and why

Add Files → Gallery beside ADB transfer and storage/sharing. Browse image folders in shared storage with 24-item pages, previews and explicit original downloads. Keep one SMS reply Close control by disabling automatic header title buttons.

## How

`gallery.rs` owns GTK navigation, device selection, cancellation and generation guards; bounded worker channels stream thumbnail results. Discovery reuses the file tools’ paired-peer lookup. `gallery_transfer.rs` uploads an original checked-in DEX into a private session directory, runs read-only thumbnail/preview jobs, and cleans exact owned files. `gui/android/Gallery.java` uses Android 9+ ImageDecoder, preserving orientation and bounding source size/pixels and output dimensions. Legacy ExifInterface required an application context on the test firmware, so the original manual orientation approach was replaced by the native decoder. Browsing transfers downsized JPEG only; images stay in RAM. Explicit downloads reuse atomic no-overwrite ADB staging. No SDK/Java runtime dependency on Linux, no phone-media writes or startup scan.

## Limits and validation

Folders, not MediaStore virtual collections; descending filename order, not EXIF-date ordering. No video playback, deletion, recursive all-photo scan or persistent cache. GIF uses one frame; JPEG preview flattens transparency. Offline cleanup is best effort. Tests use synthetic entries and an explicitly created, owned JPEG fixture; they never browse personal photos. Verify paging/filtering, decode bounds, orientation, helper cleanup and isolated GTK navigation/close controls.

Verified: final locked workspace release build passed without warnings; 334 tests passed, 0 failed and 40 optional tests were skipped. Isolated Gallery grid/cancellation, Files navigation and Messages single-close checks all passed. An owned synthetic JPEG with Orientation=6 passed phone-side 256/2048 sizing, orientation, invalid-path rejection and exact local/phone helper cleanup checks. No personal photos were read or changed.
