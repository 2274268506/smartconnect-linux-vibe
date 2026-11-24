//! Shared storage, retention and redaction contract for the mirror writer and GTK viewer.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

pub const FILE_LIMIT: usize = 2 * 1024 * 1024;
pub const RETAINED_LIMIT: usize = 3 * FILE_LIMIT;
pub const VIEW_LIMIT: usize = 256 * 1024;

pub fn directory() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".local/state"))
                .unwrap_or_else(std::env::temp_dir)
        })
        .join("smartconnect-linux")
}
pub fn path() -> PathBuf {
    directory().join("mirror.log")
}

pub fn sanitize(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if [
        "ice-pwd",
        "ice-ufrag",
        "fingerprint",
        "candidate:",
        "\"sdp\"",
    ]
    .iter()
    .any(|key| lower.contains(key))
    {
        return "[transport credentials/details omitted]".into();
    }
    static SECRET: OnceLock<Regex> = OnceLock::new();
    static ADDRESS: OnceLock<Regex> = OnceLock::new();
    let secret = SECRET.get_or_init(|| Regex::new(r#"(?i)(["']?(?:password|passwd|token|secret|authorization|deviceId|realDeviceId|device_id|key)["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,;}]+)"#).unwrap());
    let address = ADDRESS.get_or_init(|| {
        Regex::new(r"(?i)\b(?:[0-9]{1,3}\.){3}[0-9]{1,3}\b|\b(?:[0-9a-f]{0,4}:){2,}[0-9a-f:.]*\b")
            .unwrap()
    });
    let text = secret.replace_all(message, "${1}<redacted>");
    address
        .replace_all(&text, |captures: &regex::Captures<'_>| {
            let value = &captures[0];
            let mac = value.split(':').count() == 6
                && value.split(':').all(|part| {
                    part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_hexdigit())
                });
            if value.parse::<std::net::IpAddr>().is_ok() || mac {
                "<address>".to_owned()
            } else {
                value.to_owned()
            }
        })
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\t')
        .take(4096)
        .collect()
}

fn lock(dir: &Path, exclusive: bool) -> io::Result<File> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(dir.join("mirror.lock"))?;
    // SAFETY: the owned file remains live for the lock's duration; closing it releases flock.
    if unsafe {
        libc::flock(
            file.as_raw_fd(),
            if exclusive {
                libc::LOCK_EX
            } else {
                libc::LOCK_SH
            },
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

pub fn append(dir: &Path, text: &str) -> io::Result<()> {
    append_limit(dir, text, FILE_LIMIT)
}
fn append_limit(dir: &Path, text: &str, limit: usize) -> io::Result<()> {
    let _lock = lock(dir, true)?;
    let path = dir.join("mirror.log");
    if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) + text.len() as u64 > limit as u64 {
        let older = dir.join("mirror.log.2");
        if older.exists() {
            fs::remove_file(&older)?;
        }
        let previous = dir.join("mirror.log.1");
        if previous.exists() {
            fs::rename(previous, older)?;
        }
        if path.exists() {
            fs::rename(&path, dir.join("mirror.log.1"))?;
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}

pub fn read_recent(limit: usize) -> io::Result<String> {
    read_from(&directory(), limit)
}
pub fn read_from(dir: &Path, limit: usize) -> io::Result<String> {
    if !dir.try_exists()? {
        return Ok(String::new());
    }
    let _lock = lock(dir, false)?;
    let mut chunks = Vec::new();
    let mut remaining = limit;
    for name in ["mirror.log", "mirror.log.1", "mirror.log.2"] {
        if remaining == 0 {
            break;
        }
        let mut file = match File::open(dir.join(name)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let length = file.metadata()?.len();
        let start = length.saturating_sub(remaining as u64);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(remaining as u64).read_to_end(&mut bytes)?;
        remaining = remaining.saturating_sub(bytes.len());
        if start > 0 {
            if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
                bytes.drain(..=end);
            }
        }
        chunks.push(String::from_utf8_lossy(&bytes).into_owned());
    }
    chunks.reverse();
    Ok(chunks
        .concat()
        .lines()
        .map(sanitize)
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn filter(text: &str, query: &str, warnings_only: bool) -> String {
    let query = query.to_lowercase();
    text.lines()
        .filter(|line| {
            (!warnings_only || line.contains("[ERROR]") || line.contains("[WARN]"))
                && (query.is_empty() || line.to_lowercase().contains(&query))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_credentials_addresses_and_identifiers_are_hidden() {
        assert!(!sanitize("caps a-ice-pwd=example-secret").contains("example-secret"));
        let clean = sanitize(
            r#"peer 192.0.2.1 2001:db8::1 token="example-token" deviceId="example-device""#,
        );
        for sensitive in [
            "192.0.2.1",
            "2001:db8::1",
            "example-token",
            "example-device",
        ] {
            assert!(!clean.contains(sensitive), "{clean}");
        }
        assert_eq!(
            sanitize("decoder=vah264dec 2880x1920"),
            "decoder=vah264dec 2880x1920"
        );
        assert_eq!(
            sanitize("2026-01-01T12:34:56Z [INFO] colorimetry=2:4:16:3"),
            "2026-01-01T12:34:56Z [INFO] colorimetry=2:4:16:3"
        );
    }
    #[test]
    fn rotation_keeps_order_and_latest_entries_with_private_permissions() {
        let dir = std::env::temp_dir().join(format!(
            "smartconnect-log-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        for number in 0..5 {
            append_limit(&dir, &format!("[INFO] entry {number}\n"), 20).unwrap();
        }
        let text = read_from(&dir, 1000).unwrap();
        assert!(!text.contains("entry 1"));
        assert!(text.find("entry 2").unwrap() < text.find("entry 4").unwrap());
        assert_eq!(
            fs::metadata(dir.join("mirror.log"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(!dir.join("mirror.log.3").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn filter_supports_unicode_case_and_error_levels() {
        let text = "[INFO] 桌面 started\n[WARN] Desktop delayed\n[ERROR] decoder failed";
        assert_eq!(filter(text, "DESKTOP", true), "[WARN] Desktop delayed");
        assert_eq!(filter(text, "桌面", false), "[INFO] 桌面 started");
    }
}
