#!/bin/sh
# Regenerate the embedded helper with JDK 17+ and an explicitly supplied R8/D8 jar.
set -eu
: "${R8_JAR:?Set R8_JAR to an R8 jar containing com.android.tools.r8.D8}"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM
mkdir -p "$build_dir/classes" "$build_dir/dex"
javac --release 8 -Xlint:-options -d "$build_dir/classes" "$script_dir/DisplayPower.java" "$script_dir/DisplayPowerTest.java"
java -cp "$build_dir/classes" org.smartconnect.DisplayPowerTest
java -cp "$R8_JAR" com.android.tools.r8.D8 --min-api 26 --output "$build_dir/dex" "$build_dir/classes/org/smartconnect/DisplayPower.class"
cp "$build_dir/dex/classes.dex" "$script_dir/display-power.dex"
