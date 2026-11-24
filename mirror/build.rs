use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=native/computer_ime.c");
    println!("cargo:rerun-if-env-changed=CC");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("computer-ime.so");
    let flags = Command::new("pkg-config")
        .args(["--cflags", "sdl3"])
        .output();
    // The optional bridge must not prevent builds on systems without SDL3 headers.
    if env::var("HOST") != env::var("TARGET") || !flags.as_ref().is_ok_and(|r| r.status.success()) {
        fs::write(output, []).unwrap();
        return;
    }
    let flags = flags.unwrap();
    let status = Command::new(env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args([
            "-shared",
            "-fPIC",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "native/computer_ime.c",
            "-ldl",
            "-o",
        ])
        .arg(output)
        .args(String::from_utf8(flags.stdout).unwrap().split_whitespace())
        .status()
        .unwrap();
    assert!(
        status.success(),
        "could not compile SDL3 computer input bridge"
    );
}
