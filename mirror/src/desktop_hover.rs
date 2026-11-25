//! Supplemental native desktop hover for scrcpy's mirrored-display input path.
//! Only the top edge arms forwarding; leaving its caption band disarms it.
#[derive(Default)]
pub struct Hover {
    active: bool,
    last: Option<(u32, u32)>,
}
/// Map X11 client pixels into the centered, aspect-fitted video; reject black bars/outside.
pub fn video_point(point: (i32, i32), window: (u32, u32), video: (u32, u32)) -> Option<(u32, u32)> {
    if window.0 == 0 || window.1 == 0 || video.0 == 0 || video.1 == 0 {
        return None;
    }
    let scale = (window.0 as f64 / video.0 as f64).min(window.1 as f64 / video.1 as f64);
    let left = (window.0 as f64 - video.0 as f64 * scale) / 2.0;
    let top = (window.1 as f64 - video.1 as f64 * scale) / 2.0;
    let x = (point.0 as f64 - left) / scale;
    let y = (point.1 as f64 - top) / scale;
    (x >= 0.0 && y >= 0.0 && x < video.0 as f64 && y < video.1 as f64)
        .then_some((x as u32, y as u32))
}
impl Hover {
    pub fn observe(&mut self, point: Option<(u32, u32)>, height: u32) -> Option<(u32, u32)> {
        let Some(point) = point else {
            self.active = false;
            self.last = None;
            return None;
        };
        if !self.active && point.1 > 3 {
            return None;
        }
        self.active = point.1 <= (height / 8).clamp(96, 256);
        if self.last == Some(point) {
            return None;
        }
        self.last = Some(point);
        // Forward the first move below the band too, allowing the phone to hide its caption.
        Some(point)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_scaling_and_letterboxing_without_triggering_on_black_bars() {
        assert_eq!(
            video_point((960, 0), (1920, 1080), (2880, 1920)),
            Some((1440, 0))
        );
        assert_eq!(video_point((100, 10), (1920, 1080), (2880, 1920)), None);
        assert_eq!(video_point((960, 0), (1920, 1200), (1920, 1080)), None);
        assert_eq!(
            video_point((960, 60), (1920, 1200), (1920, 1080)),
            Some((960, 0))
        );
        assert_eq!(video_point((-1, 0), (1920, 1080), (1920, 1080)), None);
        assert_eq!(video_point((1920, 0), (1920, 1080), (1920, 1080)), None);
        assert_eq!(video_point((0, 0), (0, 1080), (1920, 1080)), None);
    }
    #[test]
    fn forwards_only_an_edge_visit_and_its_exit_not_normal_app_motion() {
        let mut hover = Hover::default();
        assert_eq!(hover.observe(Some((500, 60)), 1920), None);
        assert_eq!(hover.observe(Some((500, 0)), 1920), Some((500, 0)));
        assert_eq!(hover.observe(Some((500, 0)), 1920), None);
        assert_eq!(hover.observe(Some((550, 45)), 1920), Some((550, 45)));
        assert_eq!(hover.observe(Some((550, 400)), 1920), Some((550, 400)));
        assert_eq!(hover.observe(Some((560, 450)), 1920), None);
        hover.observe(Some((500, 0)), 1920);
        assert_eq!(hover.observe(None, 1920), None);
        assert_eq!(hover.observe(Some((550, 45)), 1920), None);
    }
}
