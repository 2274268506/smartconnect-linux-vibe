//! Explicit, cancellable ADB file operations. No phone data is logged or persisted.
use std::os::unix::process::CommandExt;
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, String>;
pub type Cancel = Arc<AtomicBool>;
static NEXT: AtomicU64 = AtomicU64::new(0);
const LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub directory: bool,
}
impl Entry {
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

pub fn cancelled(cancel: &Cancel) -> bool {
    cancel.load(Ordering::Relaxed)
}
fn check(cancel: &Cancel) -> Result<()> {
    if cancelled(cancel) {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn remote_path(path: &str) -> Result<String> {
    if path.contains('\0')
        || !(path == "/sdcard" || path.starts_with("/sdcard/"))
        || path.split('/').any(|s| s == ".." || s == ".")
    {
        return Err("Choose a folder inside /sdcard".into());
    }
    Ok(path.trim_end_matches('/').to_owned())
}
fn capture(mut input: impl Read) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if out.len() + n <= LIMIT {
            out.extend_from_slice(&buf[..n]);
        } else {
            return Err(std::io::Error::other("ADB output exceeds limit"));
        }
    }
    Ok(out)
}
fn run(args: &[&str], cancel: &Cancel, timeout: Duration) -> Result<Vec<u8>> {
    run_at(&crate::mirror::binary("adb"), args, cancel, timeout)
}
fn run_at(binary: &Path, args: &[&str], cancel: &Cancel, timeout: Duration) -> Result<Vec<u8>> {
    check(cancel)?;
    let mut child = Command::new(binary)
        .args(args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ADB: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || capture(stdout));
    let err = std::thread::spawn(move || capture(stderr));
    let start = Instant::now();
    let status = loop {
        if cancelled(cancel) || start.elapsed() > timeout {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            break Err(if cancelled(cancel) {
                "Cancelled"
            } else {
                "ADB operation timed out"
            }
            .to_owned());
        }
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
    };
    let output = out
        .join()
        .map_err(|_| "ADB output worker failed")?
        .map_err(|e| e.to_string());
    let error = err
        .join()
        .map_err(|_| "ADB output worker failed")?
        .map_err(|e| e.to_string());
    let status = status?;
    let output = output?;
    let error = error?;
    if status.success() {
        Ok(output)
    } else {
        Err(format!(
            "ADB: {}",
            String::from_utf8_lossy(if error.is_empty() { &output } else { &error })
                .trim()
                .chars()
                .take(500)
                .collect::<String>()
        ))
    }
}
fn shell(device: &str, script: &str, cancel: &Cancel) -> Result<Vec<u8>> {
    run(
        &["-s", device, "shell", "-T", script],
        cancel,
        Duration::from_secs(20),
    )
}
pub fn devices(cancel: &Cancel) -> Result<Vec<String>> {
    let bytes = run(&["devices"], cancel, Duration::from_secs(15))?;
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| {
            let mut p = line.split_whitespace();
            let id = p.next()?;
            (p.next()? == "device").then(|| id.to_owned())
        })
        .collect())
}
pub fn connect(address: &str, cancel: &Cancel) -> Result<()> {
    if address.is_empty() || address.starts_with('-') || address.chars().any(char::is_whitespace) {
        return Err("Enter an ADB address (host:port)".into());
    }
    let out = run(&["connect", address], cancel, Duration::from_secs(20))?;
    let text = String::from_utf8_lossy(&out);
    if text.contains("connected to") {
        Ok(())
    } else {
        Err(text.trim().chars().take(500).collect())
    }
}
pub fn parse_listing(bytes: &[u8], parent: &str) -> Result<Vec<Entry>> {
    let fields: Vec<_> = bytes.split(|b| *b == 0).collect();
    let mut entries = Vec::new();
    if fields.last() != Some(&&b""[..]) || fields.len() % 2 != 1 {
        return Err("Invalid ADB directory listing".into());
    }
    for pair in fields[..fields.len() - 1].chunks_exact(2) {
        let path =
            std::str::from_utf8(pair[1]).map_err(|_| "Non-UTF-8 filename is not supported")?;
        remote_path(path)?;
        if path.rsplit_once('/').map(|p| p.0) != Some(parent) || !matches!(pair[0], b"d" | b"f") {
            return Err("Invalid ADB directory entry".into());
        }
        entries.push(Entry {
            path: path.into(),
            directory: pair[0] == b"d",
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name().cmp(b.name()))
    });
    Ok(entries)
}
pub fn list(device: &str, path: &str, cancel: &Cancel) -> Result<Vec<Entry>> {
    let path = remote_path(path)?;
    let script=format!("d={}; [ -d \"$d\" ] && [ -r \"$d\" ] && [ -x \"$d\" ] || exit 1; for p in \"$d\"/* \"$d\"/.[!.]* \"$d\"/..?*; do [ -L \"$p\" ] && continue; if [ -d \"$p\" ]; then printf 'd\\0%s\\0' \"$p\"; elif [ -f \"$p\" ]; then printf 'f\\0%s\\0' \"$p\"; fi; done; :",quote(&path));
    parse_listing(&shell(device, &script, cancel)?, &path)
}
fn token() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
fn local_name(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty() && !s.contains(['/', '\0']))
        .ok_or_else(|| "Unsupported filename".into())
}

pub fn upload(device: &str, local: &Path, folder: &str, cancel: &Cancel) -> Result<()> {
    let folder = remote_path(folder)?;
    let name = local_name(local)?;
    if !fs::metadata(local).map_err(|e| e.to_string())?.is_file() {
        return Err("Select a regular file".into());
    }
    let dest = format!("{folder}/{name}");
    let temp = format!("{folder}/.smartconnect-upload-{}.part", token());
    // Refuse collisions, including dangling symlinks. mkdir reserves our private staging directory.
    let stage = format!("{temp}.d");
    let payload = format!("{stage}/payload");
    let result = (|| {
        let reserve = format!(
            "if [ -e {d} ] || [ -L {d} ]; then printf %s 'A file with this name already exists' >&2; exit 1; fi; mkdir {s}",
            d = quote(&dest), s = quote(&stage)
        );
        shell(device, &reserve, cancel)?;
        let local = local
            .to_str()
            .ok_or("Non-UTF-8 local path is not supported")?;
        run(
            &["-s", device, "push", local, &payload],
            cancel,
            Duration::from_secs(24 * 3600),
        )?;
        check(cancel)?;
        shell(
            device,
            &format!(
                "[ ! -e {d} ] && [ ! -L {d} ] && mv -nT {p} {d} && [ ! -e {p} ]",
                d = quote(&dest),
                p = quote(&payload)
            ),
            cancel,
        )?;
        Ok(())
    })();
    let cleanup = Arc::new(AtomicBool::new(false));
    let _ = shell(
        device,
        &format!("rm -f {}; rmdir {}", quote(&payload), quote(&stage)),
        &cleanup,
    );
    result
}
pub fn publish(temp: &Path, dest: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let a = std::ffi::CString::new(temp.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let b = std::ffi::CString::new(dest.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}
pub fn download(device: &str, remote: &str, folder: &Path, cancel: &Cancel) -> Result<()> {
    let remote = remote_path(remote)?;
    let name = local_name(Path::new(&remote))?;
    let dest = folder.join(name);
    if dest.symlink_metadata().is_ok() {
        return Err("A file with this name already exists".into());
    }
    let stage = folder.join(format!(".smartconnect-download-{}.part", token()));
    // A private directory prevents another process replacing the local partial file.
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&stage)
        .map_err(|e| e.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    let result = (|| {
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let temp = stage.join("payload");
        shell(
            device,
            &format!("[ -f {p} ] && [ ! -L {p} ]", p = quote(&remote)),
            cancel,
        )?;
        run(
            &[
                "-s",
                device,
                "pull",
                &remote,
                temp.to_str()
                    .ok_or("Non-UTF-8 local path is not supported")?,
            ],
            cancel,
            Duration::from_secs(24 * 3600),
        )?;
        check(cancel)?;
        publish(&temp, &dest)
    })();
    let _ = fs::remove_file(stage.join("payload"));
    let _ = fs::remove_dir(&stage);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_remote_paths() {
        for s in [
            "/sdcard/../data",
            "/sdcardx/a",
            "/data",
            "/sdcard/./a",
            "/sdcard/a\0b",
        ] {
            assert!(remote_path(s).is_err());
        }
        assert_eq!(
            remote_path("/sdcard/Download/").unwrap(),
            "/sdcard/Download"
        );
    }
    #[test]
    fn listing_handles_unicode_newlines_and_directories() {
        let v = parse_listing(
            "f\0/sdcard/空 格\n'文件\0d\0/sdcard/Download\0".as_bytes(),
            "/sdcard",
        )
        .unwrap();
        assert!(v[0].directory);
        assert_eq!(v[1].name(), "空 格\n'文件");
        assert!(parse_listing(b"f\0/sdcard/../private\0", "/sdcard").is_err());
        assert!(parse_listing(b"f\0/sdcard/file", "/sdcard").is_err());
        assert!(parse_listing(b"", "/sdcard").unwrap().is_empty());
    }
    #[test]
    fn quoting_survives_shell_metacharacters() {
        let original = "空 格'\n$(false);`false`";
        let out = Command::new("sh")
            .args(["-c", &format!("printf %s {}", quote(original))])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, original.as_bytes());
    }
    #[test]
    fn publish_does_not_overwrite() {
        let dir = std::env::temp_dir().join(token());
        fs::create_dir(&dir).unwrap();
        let temp = dir.join("part");
        let dest = dir.join("final");
        fs::write(&temp, b"new").unwrap();
        fs::write(&dest, b"old").unwrap();
        assert!(publish(&temp, &dest).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"old");
        fs::remove_file(&dest).unwrap();
        publish(&temp, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn pre_cancelled_operations_do_not_spawn() {
        let c = Arc::new(AtomicBool::new(true));
        assert_eq!(
            run(&["version"], &c, Duration::from_secs(1)).unwrap_err(),
            "Cancelled"
        );
    }
    #[test]
    fn cancellation_reaps_the_running_child() {
        let c = Arc::new(AtomicBool::new(false));
        let signal = c.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            signal.store(true, Ordering::Relaxed);
        });
        let now = Instant::now();
        let result = run_at(
            Path::new("sh"),
            &["-c", "exec sleep 30"],
            &c,
            Duration::from_secs(10),
        );
        thread.join().unwrap();
        assert_eq!(result.unwrap_err(), "Cancelled");
        assert!(now.elapsed() < Duration::from_secs(3));
    }
    #[test]
    #[ignore = "requires an explicitly selected authorized test phone"]
    fn phone_roundtrip_preserves_names_and_existing_files() {
        let device = std::env::var("SMARTCONNECT_TEST_ADB_DEVICE").expect("choose a test device");
        let cancel = Arc::new(AtomicBool::new(false));
        let dir = std::env::temp_dir().join(token());
        fs::create_dir(&dir).unwrap();
        let remote = format!("/sdcard/Download/smartconnect-test-{}", token());
        shell(&device, &format!("mkdir {}", quote(&remote)), &cancel).unwrap();
        let name = "传输 测试'$(false).txt";
        let source = dir.join(name);
        fs::write(&source, b"Smart Connect ADB roundtrip\n").unwrap();
        let result = (|| -> Result<()> {
            upload(&device, &source, &remote, &cancel)?;
            let entries = list(&device, &remote, &cancel)?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name(), name);
            fs::write(&source, b"should not overwrite").unwrap();
            assert!(upload(&device, &source, &remote, &cancel).is_err());
            let output = dir.join("output");
            fs::create_dir(&output).unwrap();
            download(&device, &entries[0].path, &output, &cancel)?;
            assert_eq!(
                fs::read(output.join(name)).unwrap(),
                b"Smart Connect ADB roundtrip\n"
            );
            assert!(download(&device, &entries[0].path, &output, &cancel).is_err());
            assert_eq!(
                fs::read(output.join(name)).unwrap(),
                b"Smart Connect ADB roundtrip\n"
            );
            assert_eq!(list(&device, &remote, &cancel)?.len(), 1);
            Ok(())
        })();
        let _ = shell(
            &device,
            &format!(
                "rm -f {}; rmdir {}",
                quote(&format!("{remote}/{name}")),
                quote(&remote)
            ),
            &cancel,
        );
        let _ = fs::remove_dir_all(dir);
        result.unwrap();
    }
}
