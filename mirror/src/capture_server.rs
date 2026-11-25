//! Session-private launcher around the installed, unmodified scrcpy server.
//! The archive contains only an original bootstrap DEX and the user's installed server asset.
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const LAUNCHER: &[u8] = include_bytes!("../android/capture-launcher.dex");
const MAX_SERVER: u64 = 64 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);
/// Native view packages are separate modes; only the desktop and actual app packages opt in.
pub fn supports_package(pkg: &str) -> bool {
    if pkg == "desktop" || pkg == "android" {
        return true;
    }
    let parts: Vec<_> = pkg.split('.').collect();
    parts.len() > 1
        && parts.iter().all(|part| {
            let mut bytes = part.bytes();
            bytes
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        })
}
pub struct Artifact {
    directory: PathBuf,
    pub path: PathBuf,
}
fn installed_server(scrcpy: &Path) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SCRCPY_SERVER_PATH").filter(|v| !v.is_empty()) {
        return Some(path.into());
    }
    let executable = if scrcpy.components().count() > 1 {
        Some(scrcpy.to_path_buf())
    } else {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(scrcpy))
                .find(|p| p.is_file())
        })
    };
    let executable = executable.and_then(|p| fs::canonicalize(p).ok());
    if let Some(dir) = executable.as_deref().and_then(Path::parent) {
        for candidate in [
            dir.join("scrcpy-server"),
            dir.join("../share/scrcpy/scrcpy-server"),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let system = PathBuf::from("/usr/share/scrcpy/scrcpy-server");
    system.is_file().then_some(system)
}
impl Artifact {
    pub fn prepare(scrcpy: &Path) -> Result<Self, String> {
        let server =
            installed_server(scrcpy).ok_or("Could not locate the installed scrcpy server")?;
        let metadata =
            fs::metadata(&server).map_err(|_| "Could not read the installed scrcpy server")?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SERVER {
            return Err("Invalid scrcpy server asset".into());
        }
        let upstream =
            fs::read(server).map_err(|_| "Could not read the installed scrcpy server")?;
        Self::from_bytes(&upstream)
    }
    fn from_bytes(upstream: &[u8]) -> Result<Self, String> {
        let directory = std::env::temp_dir().join(format!(
            "smartconnect-capture-server-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|_| "Could not create private capture launcher directory")?;
        let artifact = Self {
            path: directory.join("server.jar"),
            directory,
        };
        let bytes = stored_zip(&[("classes.dex", LAUNCHER), ("upstream.jar", upstream)]);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&artifact.path)
            .map_err(|_| "Could not prepare capture launcher")?;
        file.write_all(&bytes)
            .map_err(|_| "Could not write capture launcher")?;
        Ok(artifact)
    }
}
impl Drop for Artifact {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}
// Only two fixed entries, STORE compression and bounded assets: ZIP64, readers and user paths
// are deliberately unnecessary. CRC32 is the ZIP checksum, not a trust/authentication check.
fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    fn word(out: &mut Vec<u8>, n: u16) {
        out.extend_from_slice(&n.to_le_bytes());
    }
    fn dword(out: &mut Vec<u8>, n: u32) {
        out.extend_from_slice(&n.to_le_bytes());
    }
    let mut out = Vec::new();
    let mut index = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        let size = data.len() as u32;
        dword(&mut out, 0x04034b50);
        word(&mut out, 20);
        word(&mut out, 0);
        word(&mut out, 0);
        word(&mut out, 0);
        word(&mut out, 0x21);
        dword(&mut out, crc);
        dword(&mut out, size);
        dword(&mut out, size);
        word(&mut out, name.len() as u16);
        word(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        dword(&mut index, 0x02014b50);
        word(&mut index, 20);
        word(&mut index, 20);
        word(&mut index, 0);
        word(&mut index, 0);
        word(&mut index, 0);
        word(&mut index, 0x21);
        dword(&mut index, crc);
        dword(&mut index, size);
        dword(&mut index, size);
        word(&mut index, name.len() as u16);
        word(&mut index, 0);
        word(&mut index, 0);
        word(&mut index, 0);
        word(&mut index, 0);
        dword(&mut index, 0);
        dword(&mut index, offset);
        index.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    let length = index.len() as u32;
    out.extend_from_slice(&index);
    dword(&mut out, 0x06054b50);
    word(&mut out, 0);
    word(&mut out, 0);
    word(&mut out, entries.len() as u16);
    word(&mut out, entries.len() as u16);
    dword(&mut out, length);
    dword(&mut out, start);
    word(&mut out, 0);
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_apps_and_desktop_opt_in_but_phone_views_do_not() {
        for pkg in [
            "desktop",
            "android",
            "com.example.notes",
            "org.example_app.Player",
        ] {
            assert!(supports_package(pkg), "{pkg}");
        }
        for pkg in [
            "mirror",
            "mobile",
            "",
            "desktop-other",
            "com..example",
            "com.2invalid",
            "com.example/activity",
        ] {
            assert!(!supports_package(pkg), "{pkg}");
        }
    }
    #[test]
    fn private_artifact_is_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let artifact = Artifact::from_bytes(b"installed server fixture").unwrap();
        let directory = artifact.directory.clone();
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&artifact.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(artifact);
        assert!(!directory.exists());
    }
    #[test]
    #[ignore = "requires Python 3 for an independent ZIP reader"]
    fn independent_reader_checks_both_entries_crc_and_unchanged_server() {
        let artifact = Artifact::from_bytes(b"unchanged upstream fixture\0binary").unwrap();
        let status=std::process::Command::new("python3").args(["-c", "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1]); assert z.namelist()==['classes.dex','upstream.jar']; assert z.testzip() is None; assert z.read('classes.dex').startswith(b'dex\\n'); assert z.read('upstream.jar')==b'unchanged upstream fixture\\0binary'"])
            .arg(&artifact.path).status().unwrap();
        assert!(status.success());
    }
}
