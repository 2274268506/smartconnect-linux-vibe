//! Validated app-stream display geometry, shared by the host and GUI preferences.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppDisplay {
    pub width: u32,
    pub height: u32,
    pub density: u32,
}
impl Default for AppDisplay {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 1920,
            density: 320,
        }
    }
}
impl AppDisplay {
    pub fn valid(self) -> bool {
        (320..=3840).contains(&self.width)
            && (320..=3840).contains(&self.height)
            && self.width.min(self.height) <= 2160
            && self.width % 2 == 0
            && self.height % 2 == 0
            && (120..=640).contains(&self.density)
    }
    pub fn wire(self) -> Value {
        let mm = |pixels: u32| {
            (f64::from(pixels) * 25.4 / f64::from(self.density))
                .round()
                .max(1.0) as u32
        };
        json!({"width":self.width,"height":self.height,"displayWidth":self.width,"displayHeight":self.height,
            "displayDensity":self.density,"displayScale":1.0,"phyWidth":mm(self.width),"phyHeight":mm(self.height),
            "supportCustomDisplay":true})
    }
}
pub const INVALID: &str =
    "App resolution needs even dimensions 320–3840, short side at most 2160, and density 120–640";
pub fn from_request(value: &Value) -> Result<Option<AppDisplay>, &'static str> {
    if ["width", "height", "density"]
        .iter()
        .all(|key| value.get(key).is_none())
    {
        return Ok(None);
    }
    let number = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(INVALID)
    };
    let profile = AppDisplay {
        width: number("width")?,
        height: number("height")?,
        density: if value.get("density").is_some() {
            number("density")?
        } else {
            320
        },
    };
    if profile.valid() {
        Ok(Some(profile))
    } else {
        Err(INVALID)
    }
}
pub fn cli_options(args: &[String]) -> Result<(Option<String>, Option<AppDisplay>), &'static str> {
    let mut args = args.iter().peekable();
    let component = if args.peek().is_some_and(|arg| !arg.starts_with("--")) {
        args.next().cloned()
    } else {
        None
    };
    let (mut size, mut density) = (None, None);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--size" if size.is_none() => {
                let value = args.next().ok_or(INVALID)?;
                let (width, height) = value.split_once('x').ok_or(INVALID)?;
                size = Some((
                    width.parse::<u32>().map_err(|_| INVALID)?,
                    height.parse::<u32>().map_err(|_| INVALID)?,
                ));
            }
            "--dpi" if density.is_none() => {
                density = Some(
                    args.next()
                        .ok_or(INVALID)?
                        .parse::<u32>()
                        .map_err(|_| INVALID)?,
                )
            }
            _ => return Err("usage: --app PKG [CMP] [--size WIDTHxHEIGHT] [--dpi DPI]"),
        }
    }
    let profile = match size {
        Some((width, height)) => {
            from_request(&json!({"width":width,"height":height,"density":density.unwrap_or(320)}))?
        }
        None if density.is_some() => return Err("--dpi requires --size"),
        None => None,
    };
    Ok((component, profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_custom_geometry_without_restricting_orientation() {
        for (w, h) in [(720, 1280), (1920, 1080), (2160, 3840)] {
            assert!(
                from_request(&json!({"width":w,"height":h}))
                    .unwrap()
                    .unwrap()
                    .valid()
            );
        }
        for value in [
            json!({"width":720}),
            json!({"density":320}),
            json!({"width":721,"height":1280}),
            json!({"width":3000,"height":3000}),
            json!({"width":0,"height":720}),
            json!({"width":720,"height":1280,"density":641}),
            json!({"width":"720","height":1280}),
        ] {
            assert!(from_request(&value).is_err());
        }
        assert_eq!(from_request(&json!({})), Ok(None));
        let wire = AppDisplay::default().wire();
        assert_eq!(wire["supportCustomDisplay"], true);
        assert_eq!(wire["displayDensity"], 320);
    }
    #[test]
    fn cli_retains_component_and_requires_dimensions_for_density() {
        let strings = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (component, display) = cli_options(&strings(&[
            "com.example.Main",
            "--size",
            "720x1280",
            "--dpi",
            "240",
        ]))
        .unwrap();
        assert_eq!(component.as_deref(), Some("com.example.Main"));
        assert_eq!(
            display.unwrap(),
            AppDisplay {
                width: 720,
                height: 1280,
                density: 240
            }
        );
        assert_eq!(cli_options(&[]).unwrap(), (None, None));
        assert!(cli_options(&strings(&["--dpi", "240"])).is_err());
        assert!(cli_options(&strings(&["--size", "720x1280", "--size", "1080x1920"])).is_err());
    }
}
