#!/bin/sh
# Build only this repository's launcher; no upstream server is downloaded or embedded here.
set -eu
: "${R8_JAR:?Set R8_JAR to a jar containing com.android.tools.r8.D8}"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM
mkdir -p "$build_dir/classes" "$build_dir/dex"
javac --release 8 -Xlint:all,-options -d "$build_dir/classes" "$script_dir/capture/Server.java" "$script_dir/capture/CleanUp.java" "$script_dir/capture/DesktopWake.java" "$script_dir/capture/ScopeTest.java"
java -cp "$build_dir/classes" com.genymobile.scrcpy.ScopeTest
set -- "$build_dir/classes/com/genymobile/scrcpy/Server.class" "$build_dir/classes/com/genymobile/scrcpy/Server\$Capture.class" "$build_dir/classes/com/genymobile/scrcpy/CleanUp.class" "$build_dir/classes/com/genymobile/scrcpy/DesktopWake.class"
java -cp "$R8_JAR" com.android.tools.r8.D8 --min-api 26 --output "$build_dir/dex" "$@"
cp "$build_dir/dex/classes.dex" "$script_dir/capture-launcher.dex"
