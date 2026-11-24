# Virtual capture power helper

`DisplayPower.java` is this repository's original shell helper, not an APK or a fork of scrcpy. `display-power.dex` is embedded in the mirror binary. The host uploads it over authorized ADB and runs `app_process`; users do not need an Android SDK or PC Java runtime.

It snapshots existing captures before the viewer starts, then identifies a new shell-owned virtual display named scrcpy whose adapter mirror target matches the session source. It restores only that capture's power if OFF. Physical displays, device folding, keyguard and persistent settings are unchanged. Ambiguous discovery and unsupported APIs fail without choosing another display. Stdin EOF/source removal ends monitoring. The owning worker removes the upload on exit.

Regenerate with JDK 17+ and a locally supplied R8 jar (verified with R8 8.8.34):

```sh
R8_JAR=/path/to/r8.jar sh mirror/android/build.sh
```

The script first runs pure JVM discovery/scope tests (wrong source, physical display, wrong owner, existing capture and ambiguous capture rejection). It then compiles original Java source to Java 8 class format, runs D8 with minimum API 26, and replaces the embedded DEX. It downloads nothing. Review source and regenerated artifact together. Normal Cargo builds consume the checked-in artifact; the runtime workaround additionally requires Android's integer per-display power override API and shell permission. SDK/API level alone does not guarantee an OEM supports it.
