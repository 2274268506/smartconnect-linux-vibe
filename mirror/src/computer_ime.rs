//! Session-private transport for committed text from the scrcpy SDL3 window.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, OpenOptionsExt},
            net::UnixDatagram,
            process::CommandExt,
        },
    },
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
const SHIM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/computer-ime.so"));
static NEXT: AtomicU64 = AtomicU64::new(0);
pub fn available() -> bool {
    !SHIM.is_empty()
}
pub fn channel() -> Result<(UnixDatagram, UnixDatagram), String> {
    let (rx, tx) = UnixDatagram::pair().map_err(|_| "Could not create computer input channel")?;
    rx.set_nonblocking(true)
        .map_err(|_| "Could not configure computer input channel")?;
    Ok((rx, tx))
}
pub fn receive(rx: &UnixDatagram) -> Vec<String> {
    let mut output = Vec::new();
    let mut bytes = [0u8; 8193];
    for _ in 0..32 {
        match rx.recv(&mut bytes) {
            Ok(n) if (1..=8192).contains(&n) => {
                if let Ok(text) = std::str::from_utf8(&bytes[..n]) {
                    if !text.contains('\0') {
                        output.push(text.to_owned());
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    output
}
pub struct Adapter(PathBuf);
impl Adapter {
    pub fn attach(command: &mut Command, socket: &UnixDatagram) -> Result<Self, String> {
        if !available() {
            return Err("Computer input requires a build with SDL3 development headers".into());
        }
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "smartconnect-ime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|_| "Could not prepare computer input adapter")?;
        let adapter = Self(dir);
        let path = adapter.0.join("adapter.so");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| "Could not write computer input adapter")?;
        file.write_all(SHIM)
            .map_err(|_| "Could not write computer input adapter")?;
        // The bridge is loaded into this child only. Preserve any existing preload libraries.
        let mut preload = path.into_os_string();
        if let Some(existing) = std::env::var_os("LD_PRELOAD").filter(|v| !v.is_empty()) {
            preload.push(":");
            preload.push(existing);
        }
        let fd = socket.as_raw_fd();
        command
            .env("LD_PRELOAD", preload)
            .env("SMARTCONNECT_IME_FD", fd.to_string());
        // SAFETY: fcntl is async-signal-safe; this only preserves the owned socket across exec.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(adapter)
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdl_commits_reach_only_the_private_channel_and_adapter_is_removed() {
        if !available() {
            return;
        }
        let (rx, tx) = channel().unwrap();
        let mut command = Command::new("unused");
        let adapter = Adapter::attach(&mut command, &tx).unwrap();
        let dir = adapter.0.clone();
        let source = dir.join("test.c");
        let program = dir.join("test");
        fs::write(&source, include_str!("../native/computer_ime_test.c")).unwrap();
        let flags = Command::new("pkg-config")
            .args(["--cflags", "--libs", "sdl3"])
            .output()
            .unwrap();
        assert!(flags.status.success());
        assert!(
            Command::new("cc")
                .arg(source)
                .arg("-o")
                .arg(&program)
                .args(String::from_utf8(flags.stdout).unwrap().split_whitespace())
                .status()
                .unwrap()
                .success()
        );
        let mut child = Command::new(program);
        let loaded = Adapter::attach(&mut child, &tx).unwrap();
        let mut child = child.spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("SDL bridge test timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(status.success());
        assert_eq!(receive(&rx), ["中文🙂", "ASCII"]);
        drop(loaded);
        drop(adapter);
        assert!(!dir.exists());
    }

    #[test]
    fn commits_preserve_unicode_order_and_reject_invalid_packets() {
        let (rx, tx) = channel().unwrap();
        for text in ["中文", "é🙂", "ASCII"] {
            tx.send(text.as_bytes()).unwrap();
        }
        tx.send(&[0xff]).unwrap();
        tx.send(b"bad\0text").unwrap();
        tx.send(&vec![b'x'; 8193]).unwrap();
        assert_eq!(receive(&rx), ["中文", "é🙂", "ASCII"]);
        assert!(receive(&rx).is_empty());
    }
}
