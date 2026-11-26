//! Desktop presentation choice; contains no phone protocol fields.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Renderer {
    #[default]
    Builtin,
    Scrcpy,
}
impl Renderer {
    pub fn from_request(v: &Value) -> Result<Self, &'static str> {
        match v.get("renderer") {
            None => Ok(Self::Builtin),
            Some(Value::String(s)) if s == "builtin" => Ok(Self::Builtin),
            Some(Value::String(s)) if s == "scrcpy" => Ok(Self::Scrcpy),
            _ => Err("Desktop renderer must be builtin or scrcpy"),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    #[default]
    H264,
    H265,
    Av1,
}
impl VideoCodec {
    pub fn name(self) -> &'static str {
        match self { Self::H264 => "h264", Self::H265 => "h265", Self::Av1 => "av1" }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ScrcpyOptions {
    /// Empty: prefer the paired phone's IP, otherwise the sole authorized ADB device.
    pub adb_device: String,
    pub adb_port: u16,
    pub software_decode: bool,
    pub video_codec: VideoCodec,
    /// Empty selects the phone default encoder for the chosen codec.
    pub video_encoder: String,
    /// Forward phone output to the PC (not microphone capture).
    pub audio: bool,
    /// Capture app playback instead of volume-dependent device output (Android 13+).
    pub independent_audio: bool,
    /// Attempt a normal wake/dismiss before creating the Smart Connect display.
    pub wake_unlock: bool,
    /// Use a source-bound UHID mouse where Android/scrcpy support it.
    pub fold_mouse: bool,
    /// Relative mouse sensitivity, scoped to the scrcpy child.
    pub mouse_speed_percent: u16,
    /// Prefer a source-bound physical keyboard for the phone IME.
    pub phone_ime: bool,
    /// Compose in the computer IME and commit Unicode through Smart Connect.
    pub computer_ime: bool,
}
impl Default for ScrcpyOptions {
    fn default() -> Self {
        Self {
            adb_device: String::new(),
            adb_port: 5555,
            software_decode: false,
            video_codec: VideoCodec::default(),
            video_encoder: String::new(),
            audio: true,
            independent_audio: false,
            wake_unlock: false,
            fold_mouse: true,
            mouse_speed_percent: 100,
            phone_ime: true,
            computer_ime: false,
        }
    }
}
impl ScrcpyOptions {
    pub fn valid(&self) -> bool {
        self.video_encoder.len() <= 255
            && self.video_encoder.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            && (25..=400).contains(&self.mouse_speed_percent)
            && self.adb_port > 0
            && self.adb_device.len() <= 255
            && self
                .adb_device
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-:%[]".contains(&b))
    }
    pub fn from_request(v: &Value) -> Result<Self, &'static str> {
        let options = match v.get("scrcpy") {
            None => Self::default(),
            Some(v) => serde_json::from_value::<Self>(v.clone())
                .map_err(|_| "Invalid scrcpy ADB settings")?,
        };
        if options.valid() {
            Ok(options)
        } else {
            Err("Invalid scrcpy ADB settings")
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_encoding_migrates_and_rejects_unknown_or_unsafe_values() {
        let old = ScrcpyOptions::from_request(&serde_json::json!({})).unwrap();
        assert_eq!(old.video_codec, VideoCodec::H264);
        assert!(old.video_encoder.is_empty());
        for codec in ["h264", "h265", "av1"] {
            let options = ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{
                "videoCodec":codec,"videoEncoder":"c2.vendor.encoder.avc"}})).unwrap();
            assert_eq!(options.video_codec.name(), codec);
            assert_eq!(serde_json::to_value(options).unwrap()["videoEncoder"], "c2.vendor.encoder.avc");
        }
        for value in [serde_json::json!("unknown"), serde_json::json!(null), serde_json::json!(1)] {
            assert!(ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"videoCodec":value}})).is_err());
        }
        for encoder in ["bad name", "bad\nname", "bad;name"] {
            assert!(ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"videoEncoder":encoder}})).is_err());
        }
        assert!(!ScrcpyOptions { video_encoder:"x".repeat(256), ..Default::default() }.valid());
    }

    #[test]
    fn input_settings_migrate_and_reject_invalid_speed() {
        let old=ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"foldMouse":false}})).unwrap();
        assert_eq!(old.mouse_speed_percent,100);
        assert!(old.phone_ime);
        assert!(old.audio);
        assert!(!old.independent_audio);
        assert!(ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"independentAudio":true}})).unwrap().independent_audio);
        assert!(ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"independentAudio":1}})).is_err());
        assert!(!ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"audio":false}})).unwrap().audio);
        assert!(ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"audio":"yes"}})).is_err());
        assert!(!old.computer_ime);
        let computer=ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"computerIme":true}})).unwrap();
        assert!(computer.computer_ime && computer.phone_ime); // Computer mode takes precedence without losing the saved phone choice.
        assert!(ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"computerIme":"yes"}})).is_err());
        for speed in [25,100,175,400] {
            let request=serde_json::json!({"scrcpy":{"mouseSpeedPercent":speed,"phoneIme":false}});
            let parsed=ScrcpyOptions::from_request(&request).unwrap();
            assert_eq!(parsed.mouse_speed_percent,speed);
            assert!(!parsed.phone_ime);
            assert_eq!(serde_json::to_value(parsed).unwrap()["mouseSpeedPercent"],speed);
        }
        for speed in [serde_json::json!(0),serde_json::json!(401),serde_json::json!(-1),serde_json::json!(100.5),serde_json::json!("100")] {
            assert!(ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"mouseSpeedPercent":speed}})).is_err());
        }
    }

    #[test]
    fn old_settings_default_to_builtin_and_targets_are_validated() {
        assert_eq!(
            Renderer::from_request(&serde_json::json!({})).unwrap(),
            Renderer::Builtin
        );
        assert!(Renderer::from_request(&serde_json::json!({"renderer":"unknown"})).is_err());
        assert!(ScrcpyOptions::default().valid());
        assert!(
            ScrcpyOptions::from_request(&serde_json::json!({}))
                .unwrap()
                .fold_mouse
        );
        assert!(
            !ScrcpyOptions::from_request(&serde_json::json!({"scrcpy":{"foldMouse":false}}))
                .unwrap()
                .fold_mouse
        );
        assert!(
            ScrcpyOptions {
                adb_device: "192.168.1.20:5555".into(),
                ..Default::default()
            }
            .valid()
        );
        assert!(
            !ScrcpyOptions {
                adb_device: "device\nextra".into(),
                ..Default::default()
            }
            .valid()
        );
        assert!(
            !ScrcpyOptions {
                adb_port: 0,
                ..Default::default()
            }
            .valid()
        );
    }
}
