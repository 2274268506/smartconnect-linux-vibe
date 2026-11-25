# Virtual capture power helper

`DisplayPower.java` is this repository's original shell helper, not an APK or a fork of scrcpy. `display-power.dex` is embedded in the mirror binary. The host uploads it over authorized ADB and runs `app_process`; users do not need an Android SDK or PC Java runtime.

It snapshots existing captures before the viewer starts, then identifies a new shell-owned virtual display named scrcpy whose adapter mirror target matches the session source. It restores only that capture's power if OFF. Physical displays, device folding, keyguard and persistent settings are unchanged. Ambiguous discovery and unsupported APIs fail without choosing another display. Stdin EOF/source removal ends monitoring. The owning worker removes the upload on exit.

Regenerate with JDK 17+ and a locally supplied R8 jar (verified with R8 8.8.34):

```sh
R8_JAR=/path/to/r8.jar sh mirror/android/build.sh
```

The script first runs pure JVM discovery/scope tests (wrong source, physical display, wrong owner, existing capture and ambiguous capture rejection). It then compiles original Java source to Java 8 class format, runs D8 with minimum API 26, and replaces the embedded DEX. It downloads nothing. Review source and regenerated artifact together. Normal Cargo builds consume the checked-in artifact; the runtime workaround additionally requires Android's integer per-display power override API and shell permission. SDK/API level alone does not guarantee an OEM supports it.


## Desktop capture launcher

`capture/Server.java` and `capture/CleanUp.java` are original compatibility entrypoints around the installed, unchanged scrcpy server. For a native AppStream app or desktop on Android 15+, they scope a display-service proxy to that session's capture, replace auto-mirroring with an own-content trusted capture, and attach a retained WindowManager mirror/container on the capture layer stack. This keeps capture power separate from the physical screen. Original upstream cleanup is retained; shutdown hooks remove extracted server assets because scrcpy calls `System.exit`. Unsupported APIs restore the original capture flags.

Regenerate only the original launcher (no upstream server is bundled here):

```sh
R8_JAR=/path/to/r8.jar sh mirror/android/build-capture.sh
```

This runs JVM scope regressions and updates `capture-launcher.dex`. Runtime Rust code creates a private archive containing that DEX and the user's installed server asset. App and desktop sessions enable it; `SMARTCONNECT_SCRCPY_CAPTURE_ISOLATION=0` disables it for comparison. No Java/SDK runtime dependency is added for users.

`capture/DesktopWake.java` additionally bridges only enabled physical Motorola double-tap gestures while an isolated app/desktop stream remains active: known hardware sensors, explicit physical-display mapping, sleeping-state checks, proximity protection and duplicate suppression precede a per-display gesture wake request. No settings or credentials are changed, and listeners are removed and the wake bridge is deactivated on exit. JVM scope regressions cover its guard conditions.

Source names may include a hexadecimal session prefix for app streaming. The launcher accepts only virtual AppStream desktop/package names, excludes mirror/mobile view modes and keeps exact source-ID matching. The same guarded physical wake bridge covers app streams.
