//! Read-only, session-private phone-side thumbnails; originals are only pulled on explicit Save.
use crate::adb_transfer::{self as adb, Cancel};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
const DEX: &[u8] = include_bytes!("../android/gallery.dex");
pub fn is_image(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "heic" | "heif" | "avif"
        )
    })
}
fn decode_batch(bytes: &[u8], count: usize) -> adb::Result<Vec<adb::Result<Vec<u8>>>> {
    let invalid = || "Invalid gallery thumbnail response".to_owned();
    if count == 0
        || count > 24
        || bytes.len() < 8
        || &bytes[..4] != b"SCG1"
        || u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize != count
    {
        return Err(invalid());
    }
    let mut offset = 8;
    let mut frames = Vec::with_capacity(count);
    for _ in 0..count {
        let length = bytes.get(offset..offset + 4).ok_or_else(invalid)?;
        let length = u32::from_be_bytes(length.try_into().unwrap()) as usize;
        offset += 4;
        if length > 1024 * 1024 {
            return Err(invalid());
        }
        let frame = bytes.get(offset..offset + length).ok_or_else(invalid)?;
        offset += length;
        frames.push(if frame.starts_with(&[0xff, 0xd8, 0xff]) {
            Ok(frame.to_vec())
        } else {
            Err("Image preview unavailable".into())
        });
    }
    if offset != bytes.len() {
        return Err(invalid());
    }
    Ok(frames)
}
pub struct Thumbnails {
    device: String,
    local: PathBuf,
    remote: String,
    remote_created: bool,
}
impl Thumbnails {
    pub fn new(device: &str, cancel: &Cancel) -> adb::Result<Self> {
        let token = adb::token();
        let local = std::env::temp_dir().join(format!("smartconnect-gallery-{token}"));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&local)
            .map_err(|e| e.to_string())?;
        let mut session = Self {
            device: device.into(),
            local,
            remote: format!("/data/local/tmp/smartconnect-gallery-{token}"),
            remote_created: false,
        };
        let asset = session.local.join("gallery.dex");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&asset)
            .and_then(|mut file| file.write_all(DEX))
            .map_err(|e| e.to_string())?;
        let script = format!("umask 077; mkdir -- {}", adb::quote(&session.remote));
        adb::run(
            &["-s", device, "shell", "-T", &script],
            cancel,
            Duration::from_secs(10),
        )?;
        session.remote_created = true;
        let remote_asset = format!("{}/gallery.dex", session.remote);
        adb::run(
            &[
                "-s",
                device,
                "push",
                asset.to_str().ok_or("Invalid temporary path")?,
                &remote_asset,
            ],
            cancel,
            Duration::from_secs(20),
        )?;
        let script = format!("chmod 400 -- {}", adb::quote(&remote_asset));
        adb::run(
            &["-s", device, "shell", "-T", &script],
            cancel,
            Duration::from_secs(10),
        )?;
        Ok(session)
    }
    pub fn batch(
        &self,
        paths: &[String],
        cancel: &Cancel,
    ) -> adb::Result<Vec<adb::Result<Vec<u8>>>> {
        if paths.is_empty() || paths.len() > 24 {
            return Err("Invalid gallery thumbnail request".into());
        }
        let mut script = format!(
            "CLASSPATH={} app_process / smartconnect.gallery.Gallery --batch",
            adb::quote(&format!("{}/gallery.dex", self.remote))
        );
        for path in paths {
            let path = adb::remote_path(path)?;
            if !is_image(&path) {
                return Err("Unsupported image".into());
            }
            script.push(' ');
            script.push_str(&adb::quote(&path));
        }
        let bytes = adb::run(
            &["-s", &self.device, "exec-out", &script],
            cancel,
            Duration::from_secs(60),
        )?;
        decode_batch(&bytes, paths.len())
    }
    pub fn image(&self, path: &str, edge: u32, cancel: &Cancel) -> adb::Result<Vec<u8>> {
        let path = adb::remote_path(path)?;
        if !is_image(&path) || !matches!(edge, 256 | 2048) {
            return Err("Unsupported image".into());
        }
        let script = format!(
            "CLASSPATH={} app_process / smartconnect.gallery.Gallery {} {edge}",
            adb::quote(&format!("{}/gallery.dex", self.remote)),
            adb::quote(&path)
        );
        let bytes = adb::run(
            &["-s", &self.device, "exec-out", &script],
            cancel,
            Duration::from_secs(30),
        )?;
        if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return Err("Image preview unavailable".into());
        }
        Ok(bytes)
    }
}
impl Drop for Thumbnails {
    fn drop(&mut self) {
        // Only this operation's exact files/directory; never remove another session's assets.
        let script = format!(
            "rm -f -- {asset}; rmdir -- {dir}",
            asset = adb::quote(&format!("{}/gallery.dex", self.remote)),
            dir = adb::quote(&self.remote)
        );
        let clean = Arc::new(AtomicBool::new(false));
        if self.remote_created {
            let _ = adb::run(
                &["-s", &self.device, "shell", "-T", &script],
                &clean,
                Duration::from_secs(3),
            );
        }
        let _ = fs::remove_file(self.local.join("gallery.dex"));
        let _ = fs::remove_dir(&self.local);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_frames_preserve_failed_images_and_reject_truncated_or_trailing_data() {
        let mut bytes = b"SCG1".to_vec();
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&3u32.to_be_bytes());
        bytes.extend_from_slice(&[0xff, 0xd8, 0xff]);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let frames = decode_batch(&bytes, 2).unwrap();
        assert!(frames[0].is_ok());
        assert!(frames[1].is_err());
        assert!(decode_batch(&bytes[..bytes.len() - 1], 2).is_err());
        assert!(decode_batch(&bytes, 1).is_err());
        bytes.push(0);
        assert!(decode_batch(&bytes, 2).is_err());
    }
    #[test]
    #[ignore = "requires authorized ADB and an explicitly created synthetic gallery fixture"]
    fn phone_thumbnail_preview_and_owned_asset_cleanup() {
        let device = std::env::var("SMARTCONNECT_GALLERY_TEST_DEVICE")
            .expect("explicit authorized fixture device required");
        let image = std::env::var("SMARTCONNECT_GALLERY_TEST_IMAGE")
            .expect("synthetic fixture path required");
        assert!(image.starts_with("/sdcard/Download/smartconnect-gallery-test-"));
        let cancel = Arc::new(AtomicBool::new(false));
        let session = Thumbnails::new(&device, &cancel).unwrap();
        let remote = session.remote.clone();
        let local = session.local.clone();
        for edge in [256, 2048] {
            let bytes = session.image(&image, edge, &cancel).unwrap();
            let pixels = gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(bytes)).unwrap();
            assert!(
                pixels.width() > 0 && pixels.height() > pixels.width(),
                "EXIF portrait rotation was not applied"
            );
            assert!(pixels.width() <= edge as i32 && pixels.height() <= edge as i32);
        }
        let paths = vec![
            image.clone(),
            format!("{}/missing.jpg", image.rsplit_once('/').unwrap().0),
            image.clone(),
        ];
        let frames = session.batch(&paths, &cancel).unwrap();
        assert!(frames[0].is_ok() && frames[1].is_err() && frames[2].is_ok());
        if std::env::var_os("SMARTCONNECT_GALLERY_BENCHMARK").is_some() {
            let start = std::time::Instant::now();
            for _ in 0..24 {
                session.image(&image, 256, &cancel).unwrap();
            }
            let serial = start.elapsed();
            let start = std::time::Instant::now();
            let frames = session.batch(&vec![image.clone(); 24], &cancel).unwrap();
            assert!(frames.iter().all(Result::is_ok));
            println!(
                "Gallery 24-image fixture: serial={}ms batch={}ms",
                serial.as_millis(),
                start.elapsed().as_millis()
            );
        }
        assert!(session
            .image("/sdcard/../secret.jpg", 256, &cancel)
            .is_err());
        drop(session);
        assert!(!local.exists());
        let script = format!("[ ! -e {} ]", adb::quote(&remote));
        adb::run(
            &["-s", &device, "shell", "-T", &script],
            &cancel,
            Duration::from_secs(5),
        )
        .unwrap();
    }
    #[test]
    fn accepts_photo_formats_without_mistaking_videos_or_extensionless_paths() {
        for path in [
            "/sdcard/DCIM/相片.JPG",
            "/sdcard/Pictures/a.heic",
            "/sdcard/a.avif",
            "/sdcard/a.webp",
        ] {
            assert!(is_image(path));
        }
        for path in ["/sdcard/a.mp4", "/sdcard/DCIM", "/sdcard/a.jpg.exe"] {
            assert!(!is_image(path));
        }
    }
}
