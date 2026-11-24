//! The phone's "views" (service 1 sessions): the whole-screen mirror, Ready For desktop on a
//! virtual display, and the virtual phone. Each runs as a service-15 stream whose package is the
//! view name instead of an app.
//!
//! Started on the PC: svc 1 cmd 1 `{view, reply: 0}`. Started on the phone (Screen share tile =
//! view 1, Mobile desktop tile = view 2): the phone sends svc 1 cmd 0 `{view, resolution?}` and waits
//! 20 s for our cmd 1; it takes the view from our cmd 1 (phone app, `service.q` + `zc7`), then
//! invites the stream with svc 15 cmd 2 `{pkg: "mirror"|"desktop"|"mobile"}`.

use serde_json::{Value, json};

/// A landscape desktop size supported by the settings UI; the phone may impose a lower limit.
pub fn desktop_size(width: i64, height: i64) -> anyhow::Result<(i64, i64)> {
    anyhow::ensure!(
        (640..=3840).contains(&width)
            && (360..=2160).contains(&height)
            && width >= height
            && width % 2 == 0
            && height % 2 == 0,
        "desktop size must be landscape, even pixels, width 640–3840 and height 360–2160"
    );
    Ok((width, height))
}

/// A missing size preserves the original phone/default behavior; partial and malformed sizes fail.
pub fn requested_desktop_size(request: &Value) -> anyhow::Result<Option<(i64, i64)>> {
    if request.get("width").is_none() && request.get("height").is_none() {
        return Ok(None);
    }
    let width = request
        .get("width")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow::anyhow!("desktop needs integer width and height"))?;
    let height = request
        .get("height")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow::anyhow!("desktop needs integer width and height"))?;
    desktop_size(width, height).map(Some)
}

pub fn desktop_cli_options(args: &[String]) -> anyhow::Result<(Option<(i64, i64)>, bool)> {
    let (mut size, mut borderless) = (None, false);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--borderless" if !borderless => borderless = true,
            "--size" if size.is_none() => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--size needs WIDTHxHEIGHT"))?;
                let (w, h) = value
                    .split_once('x')
                    .ok_or_else(|| anyhow::anyhow!("use WIDTHxHEIGHT, e.g. 1920x1080"))?;
                size = Some(desktop_size(w.parse()?, h.parse()?)?);
            }
            _ => anyhow::bail!("usage: --desktop [--size WIDTHxHEIGHT] [--borderless]"),
        }
    }
    Ok((size, borderless))
}

/// Android dp scaling, independent of encoded pixel dimensions.
pub fn requested_desktop_scale(request: &Value) -> anyhow::Result<u32> {
    match request.get("scale") {
        None => Ok(100),
        Some(value) => value.as_u64().filter(|v| (100..=300).contains(v))
            .map(|v| v as u32).ok_or_else(|| anyhow::anyhow!("desktop scale must be an integer from 100 to 300")),
    }
}

pub fn desktop_display((width, height): (i64, i64), scale: u32) -> Value {
    let density = 160.0 * f64::from(scale) / 100.0;
    let mm = |pixels: i64| (pixels as f64 * 25.4 / density).round().max(1.0) as i64;
    json!({"phyWidth":mm(width), "phyHeight":mm(height),
        "displayDensity":density.round() as i64, "displayScale":f64::from(scale)/100.0})
}

pub fn desktop_resolution((width, height): (i64, i64)) -> Value {
    json!({ "ver": "1.0.0", "cmd": 33, "resolution": { "width": width, "height": height } })
}

/// Service-15 packages of view sessions (`appstream.a.y()` on the phone).
pub fn is_view_pkg(pkg: &str) -> bool {
    matches!(pkg, "mirror" | "desktop" | "mobile")
}

/// Wire `view` (the phone's display mode + 1) → service-15 package.
pub fn view_pkg(view: i64) -> Option<&'static str> {
    match view {
        1 => Some("mirror"),
        2 => Some("desktop"),
        3 => Some("mobile"),
        _ => None,
    }
}

/// Window title of a view session.
pub fn title(pkg: &str) -> Option<&'static str> {
    match pkg {
        "mirror" => Some("Phone — Smart Connect"),
        "desktop" => Some("Mobile desktop — Smart Connect"),
        "mobile" => Some("Virtual phone — Smart Connect"),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
pub struct ViewInvite {
    pub view: i64,
    /// The desktop resolution the user picked on the phone (sent when our svc 1 version is ≥ 6).
    pub resolution: Option<(i64, i64)>,
}

/// Phone svc 1 cmd 0: the user started a view on the phone.
pub fn parse_invite(msg: &Value) -> Option<ViewInvite> {
    if msg.get("cmd").and_then(Value::as_i64) != Some(0) {
        return None;
    }
    let view = msg
        .get("view")
        .and_then(Value::as_i64)
        .filter(|v| view_pkg(*v).is_some())?;
    let resolution = msg
        .get("resolution")
        .and_then(|r| Some((r.get("width")?.as_i64()?, r.get("height")?.as_i64()?)))
        .filter(|(w, h)| *w > 0 && *h > 0);
    Some(ViewInvite { view, resolution })
}

/// Our svc 1 cmd 1 answering a phone invite. The phone reads `view` (and `drawerMode`) from it;
/// `reply: 1` makes a phone whose invite already timed out ignore it instead of treating it as a
/// new PC-initiated start (INFERRED from the phone's idle-state check).
pub fn accept(view: i64) -> Value {
    json!({ "ver": "1.0.0", "cmd": 1, "view": view, "reply": 1, "drawerMode": false })
}

/// A PC-initiated start (svc 1 cmd 1). `reply` must not be 1 here.
pub fn start(view: i64) -> Value {
    json!({ "ver": "1.0.0", "cmd": 1, "view": view, "reply": 0, "drawerMode": false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_changes_density_and_physical_size_without_changing_video_size() {
        let normal = desktop_display((2880,1920),100);
        let enlarged = desktop_display((2880,1920),200);
        assert_eq!(enlarged["displayDensity"],320);
        assert_eq!(enlarged["displayScale"],2.0);
        assert!((normal["phyWidth"].as_i64().unwrap()-2*enlarged["phyWidth"].as_i64().unwrap()).abs() <= 1);
        assert_eq!(requested_desktop_scale(&json!({})).unwrap(),100);
        for scale in [json!(0),json!(99),json!(301),json!(150.5),json!("200"),json!(null)] {
            assert!(requested_desktop_scale(&json!({"scale":scale})).is_err());
        }
    }

    #[test]
    fn desktop_size_validation_and_cli_preserve_default_and_reject_bad_input() {
        assert_eq!(requested_desktop_size(&json!({})).unwrap(), None);
        assert_eq!(
            requested_desktop_size(&json!({"width":1280,"height":720})).unwrap(),
            Some((1280, 720))
        );
        for request in [
            json!({"width":1280}),
            json!({"width":"1280","height":720}),
            json!({"width":1281,"height":720}),
            json!({"width":720,"height":1280}),
            json!({"width":0,"height":0}),
            json!({"width":7680,"height":4320}),
        ] {
            assert!(requested_desktop_size(&request).is_err(), "{request}");
        }
        assert_eq!(desktop_cli_options(&[]).unwrap(), (None, false));
        assert_eq!(
            desktop_cli_options(&["--borderless".into(), "--size".into(), "1920x1080".into()]).unwrap(),
            (Some((1920, 1080)), true)
        );
        assert!(desktop_cli_options(&["--borderless".into(), "--borderless".into()]).is_err());
        assert_eq!(
            desktop_cli_options(&["--size".into(), "2560x1440".into()]).unwrap(),
            (Some((2560, 1440)), false)
        );
        assert!(desktop_cli_options(&["--size".into()]).is_err());
        assert!(desktop_cli_options(&["--size".into(), "2561x1440".into()]).is_err());
        assert!(desktop_cli_options(&["--width".into(), "2560".into()]).is_err());
    }

    #[test]
    fn invite_parsing() {
        assert_eq!(
            parse_invite(&json!({ "ver": "1.0.0", "cmd": 0, "view": 1 })),
            Some(ViewInvite {
                view: 1,
                resolution: None
            })
        );
        assert_eq!(
            parse_invite(
                &json!({ "ver": "1.0.0", "cmd": 0, "view": 2, "resolution": { "width": 2560, "height": 1440 } })
            ),
            Some(ViewInvite {
                view: 2,
                resolution: Some((2560, 1440))
            })
        );
        assert_eq!(
            parse_invite(&json!({ "cmd": 0, "view": 2, "resolution": { "width": 0, "height": 0 } }))
                .unwrap()
                .resolution,
            None
        );
        assert!(parse_invite(&json!({ "cmd": 0, "view": 7 })).is_none(), "unknown view");
        assert!(
            parse_invite(&json!({ "cmd": 31, "view": 7 })).is_none(),
            "supported-views broadcast"
        );
        assert!(parse_invite(&json!({ "cmd": 11, "errorCode": 145 })).is_none());
    }

    #[test]
    fn answers() {
        let a = accept(2);
        assert_eq!(
            (a["cmd"].as_i64(), a["view"].as_i64(), a["reply"].as_i64()),
            (Some(1), Some(2), Some(1))
        );
        assert_eq!(a["ver"], "1.0.0");
        assert_eq!(a["drawerMode"], false);
        assert_eq!(start(1)["reply"], 0);
    }

    #[test]
    fn view_packages() {
        assert_eq!(view_pkg(1), Some("mirror"));
        assert_eq!(view_pkg(2), Some("desktop"));
        assert_eq!(view_pkg(3), Some("mobile"));
        assert_eq!(view_pkg(0), None);
        assert!(is_view_pkg("desktop") && !is_view_pkg("com.example"));
        assert_eq!(title("desktop"), Some("Mobile desktop — Smart Connect"));
        assert_eq!(title("com.example"), None);
    }
}
