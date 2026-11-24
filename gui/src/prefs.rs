//! GUI preferences.
//!
//! - `$XDG_CONFIG_HOME/smartconnect-linux/gui.json` (usually `~/.config/…`): everything the app
//!   remembers, including the resolution chosen per display mode.
//! - `$XDG_CONFIG_HOME/smartconnect-linux/display.json`: the phone-as-display choices shared with
//!   `smartconnect-display` (so a start from the phone uses them) and with the Noctalia plugin.
//!   It is read on top of `gui.json`, so a change made elsewhere wins, and written on every change:
//!   `{"mode":"extend"|"mirror","size":"native"|"requested"|"WxH","displayScale":2.0,"oskHeight":45,
//!     "output":null|"NAME","audio":false,"followRotation":false,"bitrate":8000,"fps":60}`
//!
//! Unknown or missing fields fall back to defaults, so both files survive other versions.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub language: crate::i18n::Language,
    pub display: DisplayPrefs,
    pub files: FilesPrefs,
    pub apps: AppsPrefs,
    pub desktop: DesktopPrefs,
}

/// Phone desktop shown on the PC; separate from PC-to-phone Display preferences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopPrefs {
    pub width: u32,
    pub height: u32,
    // Older saved sizes were explicit choices; only an absent desktop section defaults to auto.
    #[serde(rename = "followComputer", default)]
    pub follow_computer: bool,
    pub borderless: bool,
    #[serde(rename = "scalePercent")]
    pub scale_percent: Option<u32>,
    pub quality: crate::stream_settings::Quality,
    pub renderer: crate::desktop_renderer::Renderer,
    pub scrcpy: crate::desktop_renderer::ScrcpyOptions,
}

impl Default for DesktopPrefs {
    fn default() -> Self { Self { width: 1920, height: 1080, follow_computer: true, borderless: false, scale_percent: None, quality: Default::default(), renderer: Default::default(), scrcpy: Default::default() } }
}

impl DesktopPrefs {
    pub fn fixed(width: u32, height: u32) -> Self {
        Self { width, height, follow_computer: false, ..Self::default() }
    }
    pub fn valid(&self) -> bool {
        (640..=3840).contains(&self.width) && (360..=2160).contains(&self.height)
            && self.width >= self.height && self.width % 2 == 0 && self.height % 2 == 0
            && self.scale_percent.is_none_or(|scale| (100..=300).contains(&scale)) && self.quality.valid() && ((self.renderer!=crate::desktop_renderer::Renderer::Scrcpy && !self.scrcpy.wake_unlock) || self.scrcpy.valid())
    }

    pub fn sanitized(self) -> Self { if self.valid() { self } else { Self::default() } }
}

/// Choices on the Files page (the phone's storage as a FUSE mount).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FilesPrefs {
    /// Where to mount the phone's storage. Empty means the default `~/Phone`; a `~/…` path is
    /// expanded against `$HOME`.
    pub mount_dir: String,
    /// Mount the phone's storage automatically once the phone connects.
    pub auto_mount: bool,
    /// Share a PC folder with the phone (`smartconnect-files serve`) while the phone is connected.
    pub share: bool,
    /// The folder shared with the phone. Empty means the default, your home folder; `~/…` is expanded.
    pub share_dir: String,
    /// Also show hidden (dot) files and folders to the phone.
    pub share_hidden: bool,
    /// Refuse deletions from the phone (by default they move the files to the Trash).
    pub share_read_only: bool,
}

/// Remembered choices on the Apps page.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppsPrefs {
    /// Packages opened most recently, newest first (for the Recent row). Capped on save.
    pub recent: Vec<String>,
    pub streaming: AppStreamingPrefs,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppStreamingPrefs {
    pub custom: bool,
    pub display: crate::app_display::AppDisplay,
    pub quality: crate::stream_settings::Quality,
    pub layout: crate::stream_settings::Layout,
    pub renderer: crate::desktop_renderer::Renderer,
    pub scrcpy: crate::desktop_renderer::ScrcpyOptions,
}
impl AppStreamingPrefs {
    pub fn valid(&self)->bool {self.display.valid() && ((self.renderer!=crate::desktop_renderer::Renderer::Scrcpy && !self.scrcpy.wake_unlock) || self.scrcpy.valid()) && self.quality.valid() && self.layout.valid() && (!self.layout.fixed || (self.custom && self.layout.width_for_height(self.display.height).is_some_and(|w|w.abs_diff(self.display.width)<=2)))}
    pub fn sanitized(self)->Self { if self.valid() {self} else {Self::default()} }
    pub fn requested(&self)->Option<crate::app_display::AppDisplay> { if self.custom {Some(self.display)} else {None} }
}


impl FilesPrefs {
    /// The folder to mount at: the configured one (with a leading `~/` expanded), else `~/Phone`.
    pub fn mount_path(&self) -> PathBuf {
        let dir = self.mount_dir.trim();
        if dir.is_empty() {
            default_mount_dir()
        } else {
            expand_tilde(dir)
        }
    }

    /// The folder shared with the phone: the configured one (`~/` expanded), else your home folder.
    pub fn share_path(&self) -> PathBuf {
        let dir = self.share_dir.trim();
        if dir.is_empty() {
            home_dir()
        } else {
            expand_tilde(dir)
        }
    }
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// `~/Phone`.
pub fn default_mount_dir() -> PathBuf {
    home_dir().join("Phone")
}

/// Expands a leading `~/` (or a bare `~`) against `$HOME`; other paths are taken as-is.
fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        home_dir()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// Choices on the Display page (phone as a screen for this PC).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DisplayPrefs {
    /// `extend` or `mirror` (`smartconnect-display --mode`; `mirror` is shown as Duplicate).
    pub mode: String,
    /// Resolution per mode: `native` (the phone's panel), `phone` (what the phone asks for,
    /// usually 1920x1080) or `custom` (`custom_width` x `custom_height`).
    pub extend_resolution: String,
    pub mirror_resolution: String,
    pub custom_width: u32,
    pub custom_height: u32,
    /// Extend only: scale of the phone's virtual monitor in percent (`--display-scale`).
    pub scale: u32,
    /// Extend only: output to switch on instead of the first `Virtual-*` one (`--output`).
    /// Empty means automatic.
    pub output: String,
    /// Height of the on-screen keyboard the phone's keyboard button opens, in percent of the
    /// shared screen (`--osk-height`).
    pub osk_height: u32,
    /// Experimental: send the PC's audio to the phone.
    pub audio: bool,
    /// Experimental: turn the phone's monitor when the phone is turned.
    pub follow_rotation: bool,
    /// Stream quality: video bitrate in kbit/s (`--bitrate`).
    pub bitrate: u32,
    /// Most frames per second sent (`--fps`).
    pub fps: u32,
}

/// Range offered for a custom stream size (either orientation).
pub const MIN_SIDE: u32 = 360;
pub const MAX_SIDE: u32 = 3840;
/// `--osk-height` range and default (percent of the screen).
pub const OSK_MIN: u32 = 15;
pub const OSK_MAX: u32 = 80;
pub const OSK_DEFAULT: u32 = 45;
/// `--bitrate` range and default (kbit/s) and `--fps` range and default.
pub const BITRATE_MIN: u32 = 500;
pub const BITRATE_MAX: u32 = 50_000;
pub const BITRATE_DEFAULT: u32 = 8000;
pub const FPS_MIN: u32 = 10;
pub const FPS_MAX: u32 = 60;
pub const FPS_DEFAULT: u32 = 60;

impl Default for DisplayPrefs {
    fn default() -> Self {
        Self {
            mode: "extend".into(),
            extend_resolution: "native".into(),
            mirror_resolution: "phone".into(),
            custom_width: 1920,
            custom_height: 1080,
            scale: 200,
            output: String::new(),
            osk_height: OSK_DEFAULT,
            audio: false,
            follow_rotation: false,
            bitrate: BITRATE_DEFAULT,
            fps: FPS_DEFAULT,
        }
    }
}

impl DisplayPrefs {
    /// Replaces values an older or hand-edited file may hold that the page cannot show.
    fn sanitized(mut self) -> Self {
        let d = Self::default();
        if !matches!(self.mode.as_str(), "extend" | "mirror") {
            self.mode = d.mode;
        }
        for (res, dflt) in [(&mut self.extend_resolution, d.extend_resolution), (&mut self.mirror_resolution, d.mirror_resolution)] {
            if !matches!(res.as_str(), "native" | "phone" | "custom") {
                *res = dflt;
            }
        }
        self.custom_width = self.custom_width.clamp(MIN_SIDE, MAX_SIDE);
        self.custom_height = self.custom_height.clamp(MIN_SIDE, MAX_SIDE);
        // smartconnect-display accepts 50–400 %.
        if !(50..=400).contains(&self.scale) {
            self.scale = d.scale;
        }
        self.output = self.output.trim().to_owned();
        if !(OSK_MIN..=OSK_MAX).contains(&self.osk_height) {
            self.osk_height = d.osk_height;
        }
        if !(BITRATE_MIN..=BITRATE_MAX).contains(&self.bitrate) {
            self.bitrate = d.bitrate;
        }
        if !(FPS_MIN..=FPS_MAX).contains(&self.fps) {
            self.fps = d.fps;
        }
        self
    }

    /// The resolution choice of the current mode.
    pub fn resolution(&self) -> &str {
        if self.mode == "mirror" { &self.mirror_resolution } else { &self.extend_resolution }
    }

    fn resolution_mut(&mut self) -> &mut String {
        if self.mode == "mirror" { &mut self.mirror_resolution } else { &mut self.extend_resolution }
    }

    /// The shared `display.json` form.
    fn shared(&self) -> Value {
        let size = match self.resolution() {
            "native" => "native".to_owned(),
            "custom" => format!("{}x{}", self.custom_width, self.custom_height),
            _ => "requested".to_owned(),
        };
        json!({
            "mode": self.mode,
            "size": size,
            "displayScale": f64::from(self.scale) / 100.0,
            "oskHeight": self.osk_height,
            "output": if self.output.is_empty() { Value::Null } else { Value::from(self.output.clone()) },
            "audio": self.audio,
            "followRotation": self.follow_rotation,
            "bitrate": self.bitrate,
            "fps": self.fps,
        })
    }

    /// Takes over every valid field of a `display.json` written by any front-end.
    fn apply_shared(&mut self, v: &Value) {
        if let Some(mode) = v.get("mode").and_then(Value::as_str).filter(|m| matches!(*m, "extend" | "mirror")) {
            self.mode = mode.to_owned();
        }
        match v.get("size").and_then(Value::as_str) {
            Some("native") => *self.resolution_mut() = "native".into(),
            Some("requested") => *self.resolution_mut() = "phone".into(),
            Some(wxh) => {
                if let Some((w, h)) = wxh.split_once('x').and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?))) {
                    *self.resolution_mut() = "custom".into();
                    (self.custom_width, self.custom_height) = (w, h);
                }
            }
            None => {}
        }
        if let Some(s) = v.get("displayScale").and_then(Value::as_f64).filter(|s| s.is_finite()) {
            self.scale = (s * 100.0).round() as u32;
        }
        if let Some(h) = v.get("oskHeight").and_then(Value::as_u64) {
            self.osk_height = u32::try_from(h).unwrap_or(u32::MAX);
        }
        match v.get("output") {
            Some(Value::Null) => self.output.clear(),
            Some(Value::String(o)) => self.output = o.clone(),
            _ => {}
        }
        if let Some(a) = v.get("audio").and_then(Value::as_bool) {
            self.audio = a;
        }
        if let Some(r) = v.get("followRotation").and_then(Value::as_bool) {
            self.follow_rotation = r;
        }
        if let Some(b) = v.get("bitrate").and_then(Value::as_u64) {
            self.bitrate = u32::try_from(b).unwrap_or(u32::MAX);
        }
        if let Some(f) = v.get("fps").and_then(Value::as_u64) {
            self.fps = u32::try_from(f).unwrap_or(u32::MAX);
        }
    }
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("smartconnect-linux")
}

pub fn path() -> PathBuf {
    config_dir().join("gui.json")
}

pub fn display_path() -> PathBuf {
    config_dir().join("display.json")
}

pub fn load() -> Prefs {
    let mut prefs: Prefs = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    reload_shared(&mut prefs.display);
    prefs
}

/// Re-reads `display.json` (another front-end may have changed it). Returns whether anything changed.
pub fn reload_shared(display: &mut DisplayPrefs) -> bool {
    let before = display.clone();
    if let Some(v) = std::fs::read_to_string(display_path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        display.apply_shared(&v);
    }
    *display = std::mem::take(display).sanitized();
    *display != before
}

fn write_atomic(path: &Path, text: String) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text + "\n")?;
    std::fs::rename(tmp, path)
}

/// Writes `gui.json` and the shared `display.json`.
pub fn save(prefs: &Prefs) -> std::io::Result<()> {
    write_atomic(&path(), serde_json::to_string_pretty(prefs).map_err(std::io::Error::other)?)?;
    save_shared(&prefs.display)
}

/// Changing the interface language must not rewrite shared display settings.
pub fn save_language(language: crate::i18n::Language) -> std::io::Result<()> {
    save_language_at(&path(), language)
}

fn save_language_at(path: &Path, language: crate::i18n::Language) -> std::io::Result<()> {
    save_gui_field(path, "language", serde_json::to_value(language).map_err(std::io::Error::other)?)
}

pub fn save_desktop(settings: DesktopPrefs) -> std::io::Result<()> {
    if !settings.valid() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid desktop resolution"));
    }
    save_gui_field(&path(), "desktop", serde_json::to_value(settings).map_err(std::io::Error::other)?)
}

pub fn save_app_streaming(settings:AppStreamingPrefs)->std::io::Result<()> {
    save_app_streaming_at(&path(), settings)
}
fn save_app_streaming_at(path:&Path,settings:AppStreamingPrefs)->std::io::Result<()> {
    if !settings.valid() {return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,"invalid app resolution"));}
    let mut value=match std::fs::read_to_string(path) {
        Ok(text)=>serde_json::from_str::<Value>(&text).map_err(std::io::Error::other)?,
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>json!({}),
        Err(error)=>return Err(error),
    };
    let object=value.as_object_mut().ok_or_else(||std::io::Error::other("GUI settings must be an object"))?;
    let apps=object.entry("apps").or_insert(json!({})).as_object_mut().ok_or_else(||std::io::Error::other("apps settings must be an object"))?;
    apps.insert("streaming".into(),serde_json::to_value(settings).map_err(std::io::Error::other)?);
    write_atomic(path,serde_json::to_string_pretty(&value).map_err(std::io::Error::other)?)
}

fn save_gui_field(path: &Path, key: &str, setting: Value) -> std::io::Result<()> {
    let mut value = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e),
    };
    let object = value.as_object_mut().ok_or_else(||
        std::io::Error::new(std::io::ErrorKind::InvalidData, "GUI settings must be a JSON object"))?;
    object.insert(key.into(), setting);
    write_atomic(path, serde_json::to_string_pretty(&value).map_err(std::io::Error::other)?)
}

pub fn save_shared(display: &DisplayPrefs) -> std::io::Result<()> {
    write_atomic(&display_path(), serde_json::to_string_pretty(&display.shared()).map_err(std::io::Error::other)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_streaming_settings_preserve_recents_and_other_preferences() {
        let dir=std::env::temp_dir().join(format!("smartconnect-app-prefs-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();let path=dir.join("gui.json");
        let original=json!({"language":"zh-CN","desktop":{"width":2880,"height":1920},
            "apps":{"recent":["com.example.one"],"futureField":true},"otherFuture":{"enabled":true}});
        std::fs::write(&path,original.to_string()).unwrap();
        let mut settings=AppStreamingPrefs::default();settings.custom=true;settings.display.width=720;settings.display.height=1280;
        save_app_streaming_at(&path,settings.clone()).unwrap();
        let saved:Value=serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["apps"]["recent"],original["apps"]["recent"]);assert_eq!(saved["apps"]["futureField"],true);
        assert_eq!(saved["desktop"],original["desktop"]);assert_eq!(saved["language"],original["language"]);
        assert_eq!(saved["otherFuture"],original["otherFuture"]);
        assert_eq!(saved["apps"]["streaming"]["display"]["width"],720);
        settings.display.width=721;assert!(save_app_streaming_at(&path,settings).is_err());
        let old:Prefs=serde_json::from_value(json!({"apps":{"recent":[]}})).unwrap();assert!(!old.apps.streaming.custom);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn desktop_settings_default_round_trip_and_reject_invalid_dimensions() {
        let old: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(old.desktop, DesktopPrefs::default());
        assert!(old.desktop.follow_computer);
        let legacy: Prefs = serde_json::from_value(json!({"desktop":{"width":1280,"height":720}})).unwrap();
        assert!(!legacy.desktop.follow_computer, "keep an older explicit size");
        assert!(!legacy.desktop.borderless);
        assert_eq!(legacy.desktop.scale_percent,None);
        let scaled: Prefs = serde_json::from_value(json!({"desktop":{"width":1920,"height":1080,"scalePercent":200}})).unwrap();
        assert!(scaled.desktop.valid());
        assert_eq!(scaled.desktop.scale_percent,Some(200));
        let custom = DesktopPrefs::fixed(2560, 1440);
        assert!(custom.valid());
        let prefs: Prefs = serde_json::from_value(json!({"desktop":custom,"language":"zh-CN"})).unwrap();
        assert_eq!(prefs.desktop, custom);
        assert_eq!(prefs.language, crate::i18n::Language::Chinese);
        for dimensions in [(1281,720),(0,0),(720,1280),(7680,4320)] {
            let settings = DesktopPrefs::fixed(dimensions.0, dimensions.1);
            assert!(!settings.valid());
            assert_eq!(settings.sanitized(), DesktopPrefs::default());
        }
    }

    #[test]
    fn changing_language_preserves_other_settings_and_rejects_corrupt_files() {
        let dir = std::env::temp_dir().join(format!("smartconnect-language-test-{}-{}",
            std::process::id(), crate::lastdevice::now_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gui.json");
        let shared = dir.join("display.json");
        std::fs::write(&shared, "shared-display-sentinel").unwrap();
        let original = json!({"files":{"mountDir":"/tmp/phone"},"apps":{"recent":["example.app"]},
            "display":{"fps":30},"futureSetting":{"keep":true}});
        std::fs::write(&path, original.to_string()).unwrap();
        save_language_at(&path, crate::i18n::Language::Chinese).unwrap();
        let changed: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut expected = original;
        expected["language"] = json!("zh-CN");
        assert_eq!(changed, expected);
        assert_eq!(std::fs::read_to_string(&shared).unwrap(), "shared-display-sentinel");
        let prefs: Prefs = serde_json::from_value(changed).unwrap();
        assert_eq!(prefs.language, crate::i18n::Language::Chinese);
        assert_eq!(prefs.display.fps, 30);
        std::fs::write(&path, "broken JSON").unwrap();
        assert!(save_language_at(&path, crate::i18n::Language::English).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken JSON");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_and_bad_fields_fall_back_to_defaults() {
        let p: Prefs = serde_json::from_str(r#"{"display":{"mode":"sideways","scale":5000,"customWidth":12,"oskHeight":3}}"#).unwrap();
        let d = p.display.sanitized();
        assert_eq!(d.mode, "extend");
        assert_eq!(d.scale, 200);
        assert_eq!(d.custom_width, MIN_SIDE);
        assert_eq!(d.osk_height, OSK_DEFAULT);
        assert_eq!(d.extend_resolution, "native");
        assert_eq!(d.mirror_resolution, "phone");
    }

    #[test]
    fn files_mount_path_defaults_and_expands() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        // Empty → ~/Phone.
        assert_eq!(FilesPrefs::default().mount_path(), home.join("Phone"));
        // A `~/…` path is expanded against $HOME; an absolute path is taken as-is.
        assert_eq!(FilesPrefs { mount_dir: "~/Storage/Phone".into(), ..Default::default() }.mount_path(), home.join("Storage/Phone"));
        assert_eq!(FilesPrefs { mount_dir: "/mnt/phone".into(), ..Default::default() }.mount_path(), PathBuf::from("/mnt/phone"));
        assert_eq!(FilesPrefs::default().share_path(), home);
        assert_eq!(FilesPrefs { share_dir: "~/Public".into(), ..Default::default() }.share_path(), home.join("Public"));
    }

    #[test]
    fn gui_json_round_trips() {
        let mut p = Prefs::default();
        p.display.mode = "mirror".into();
        p.display.output = "HDMI-A-1".into();
        p.display.audio = true;
        let back: Prefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn shared_file_matches_the_contract() {
        let d = DisplayPrefs::default();
        assert_eq!(
            d.shared(),
            json!({"mode":"extend","size":"native","displayScale":2.0,"oskHeight":45,"output":null,"audio":false,"followRotation":false,"bitrate":8000,"fps":60})
        );
        let mut d = DisplayPrefs { mode: "mirror".into(), output: "HDMI-A-1".into(), scale: 150, ..DisplayPrefs::default() };
        assert_eq!(d.shared()["size"], "requested");
        assert_eq!(d.shared()["displayScale"], 1.5);
        assert_eq!(d.shared()["output"], "HDMI-A-1");
        d.mirror_resolution = "custom".into();
        (d.custom_width, d.custom_height) = (1280, 720);
        assert_eq!(d.shared()["size"], "1280x720");
    }

    #[test]
    fn shared_file_is_taken_over() {
        let mut d = DisplayPrefs::default();
        d.apply_shared(&json!({"mode":"mirror","size":"1280x720","displayScale":1.75,"oskHeight":60,"output":"HDMI-A-1","audio":true,"followRotation":true}));
        assert_eq!((d.mode.as_str(), d.mirror_resolution.as_str(), d.custom_width, d.custom_height), ("mirror", "custom", 1280, 720));
        assert_eq!((d.scale, d.osk_height, d.output.as_str(), d.audio, d.follow_rotation), (175, 60, "HDMI-A-1", true, true));
        // The extend choice is kept for when the mode switches back.
        assert_eq!(d.extend_resolution, "native");
        // Partial or odd files only change what they validly hold.
        d.apply_shared(&json!({"mode":"sideways","size":"requested","output":null}));
        assert_eq!((d.mode.as_str(), d.mirror_resolution.as_str(), d.output.as_str()), ("mirror", "phone", ""));
    }
}
