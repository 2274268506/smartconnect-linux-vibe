# Gallery thumbnail helper

`Gallery.java` is original MIT-licensed code from this repository. The checked-in `gallery.dex`
requires no Java or Android SDK on the Linux runtime machine. Regenerate with JDK 17+ and an R8
jar (tested with R8 8.8.34):

```sh
R8_JAR=/path/to/r8.jar sh gui/android/build-gallery.sh
```

The build runs JVM-only image-bounds tests and emits only the helper, excluding tests. At runtime,
the gallery temporarily pushes this DEX to its own private shell-owned directory on an authorized
ADB phone and uses Android 9+ ImageDecoder to downsample and honor orientation. Output is JPEG at
256-pixel thumbnail or 2048-pixel preview bounds. Originals never change; GIF previews use a still
frame, and JPEG previews flatten transparency. Source files are capped at 64 MiB and 160 megapixels.
Canonical paths must remain within `/sdcard`; invalid images fail individually.

Owned phone/local helper assets are removed at job completion or cancellation (best effort after
connection loss or abrupt process exit). Image bytes stay in memory unless the user explicitly downloads an original.
No phone APKs, private photos, EXIF dumps or diagnostic captures belong in this directory.

Thumbnail pages use one `--batch` process for up to 24 images. Its binary SCG1 stream has an ASCII magic, u32 big-endian count, then one u32 big-endian JPEG length and payload per image. Zero length marks an unavailable image; each payload is capped at 1 MiB. Rust bounds and validates the full response. Single-image 2048-pixel previews retain the raw JPEG interface.
