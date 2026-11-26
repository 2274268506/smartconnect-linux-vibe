//! Phone-to-PC quality limits and physical-pixel app geometry.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Quality {
    pub bitrate_kbps: Option<u32>,
    pub fps: Option<u32>,
}
impl Quality {
    pub fn valid(self) -> bool {
        self.bitrate_kbps.is_none_or(|n| (300..=50000).contains(&n))
            && self.fps.is_none_or(|n| (1..=60).contains(&n))
    }
    pub fn custom(self) -> bool {
        self != Self::default()
    }
    pub fn from_request(v: &Value) -> Result<Self, &'static str> {
        let q = match v.get("quality") {
            None => Self::default(),
            Some(q) => serde_json::from_value(q.clone()).map_err(|_| "Invalid stream quality")?,
        };
        if q.valid() {
            Ok(q)
        } else {
            Err("Use bitrate 300–50000 kbit/s and frame rate 1–60")
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FitAxis {
    #[default]
    Manual,
    Height,
    Width,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layout {
    pub fixed: bool,
    pub ratio_width: u32,
    pub ratio_height: u32,
    pub fit: FitAxis,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            fixed: false,
            ratio_width: 9,
            ratio_height: 16,
            fit: FitAxis::Manual,
        }
    }
}
impl Layout {
    pub fn valid(self) -> bool {
        (1..=10000).contains(&self.ratio_width)
            && (1..=10000).contains(&self.ratio_height)
            && (self.fixed || self.fit == FitAxis::Manual)
    }
    pub fn from_request(v: &Value) -> Result<Self, &'static str> {
        let layout = match v.get("layout") {
            None => Self::default(),
            Some(v) => {
                serde_json::from_value::<Self>(v.clone()).map_err(|_| "Invalid app aspect ratio")?
            }
        };
        if layout.valid() {
            Ok(layout)
        } else {
            Err("Invalid app aspect ratio")
        }
    }
    /// Round the derived side to the closest even encoding size; never scale the target axis.
    pub fn width_for_height(self, h: u32) -> Option<u32> {
        even_ratio(h, self.ratio_width, self.ratio_height)
    }
    pub fn height_for_width(self, w: u32) -> Option<u32> {
        even_ratio(w, self.ratio_height, self.ratio_width)
    }
    /// A matching native fit needs real fullscreen so panels/work-area limits cannot shrink it.
    pub fn fills_monitor(self, screen: (u32, u32)) -> bool {
        self.fitted(screen).is_some_and(|size| size == screen)
    }
    pub fn fitted(self, screen: (u32, u32)) -> Option<(u32, u32)> {
        if !self.valid() || !self.fixed {
            return None;
        }
        match self.fit {
            FitAxis::Height => Some((self.width_for_height(screen.1 / 2 * 2)?, screen.1 / 2 * 2)),
            FitAxis::Width => Some((screen.0 / 2 * 2, self.height_for_width(screen.0 / 2 * 2)?)),
            FitAxis::Manual => None,
        }
    }
}
fn even_ratio(value: u32, numerator: u32, denominator: u32) -> Option<u32> {
    if denominator == 0 || numerator == 0 {
        return None;
    }
    let scaled = u64::from(value).checked_mul(u64::from(numerator))?;
    u32::try_from(((scaled + u64::from(denominator)) / (2 * u64::from(denominator))) * 2).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_fit_keeps_selected_axis_and_reports_oversized_results() {
        let l = Layout {
            fixed: true,
            fit: FitAxis::Height,
            ..Layout::default()
        };
        assert_eq!(l.fitted((2880, 1920)), Some((1080, 1920)));
        assert_eq!(
            Layout {
                fit: FitAxis::Width,
                ..l
            }
            .fitted((2880, 1920)),
            Some((2880, 5120))
        );
        assert_eq!(
            Layout {
                ratio_width: 16,
                ratio_height: 9,
                ..l
            }
            .fitted((2880, 1920)),
            Some((3414, 1920))
        );
        assert!(
            Layout {
                ratio_height: 0,
                ..l
            }
            .fitted((2880, 1920))
            .is_none()
        );
    }
    #[test]
    fn matching_native_fit_is_fullscreen_but_manual_and_other_ratios_are_not() {
        let layout = Layout { fixed: true, ratio_width: 3, ratio_height: 2, fit: FitAxis::Width };
        assert!(layout.fills_monitor((2880, 1920)));
        assert!(Layout { fit: FitAxis::Height, ..layout }.fills_monitor((2880, 1920)));
        assert!(!layout.fills_monitor((1920, 1080)));
        assert!(!Layout { fit: FitAxis::Manual, ..layout }.fills_monitor((2880, 1920)));
        assert!(!Layout { fixed: false, ..layout }.fills_monitor((2880, 1920)));
    }
    #[test]
    fn rejects_bad_limits_and_preserves_independent_automatic_options() {
        assert!(
            Quality::from_request(&serde_json::json!({"quality":{"fps":15}}))
                .unwrap()
                .valid()
        );
        for v in [
            serde_json::json!({"quality":{"fps":0}}),
            serde_json::json!({"quality":{"bitrateKbps":299}}),
            serde_json::json!({"quality":{"fps":61}}),
        ] {
            assert!(Quality::from_request(&v).is_err());
        }
    }
}
