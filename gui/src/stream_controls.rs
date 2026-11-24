//! Reusable, independent automatic/manual quality controls for phone-to-PC streams.
use crate::{i18n::tr, stream_settings::Quality};
use adw::prelude::*;
use std::rc::Rc;

pub struct QualityRows {
    bitrate_auto: adw::SwitchRow,
    bitrate: adw::SpinRow,
    fps_auto: adw::SwitchRow,
    fps: adw::SpinRow,
}
impl QualityRows {
    pub fn new(q: Quality) -> Self {
        let bitrate_auto = adw::SwitchRow::builder()
            .title(tr("Automatic bitrate"))
            .active(q.bitrate_kbps.is_none())
            .build();
        let bitrate = spin(
            tr("Bitrate limit (kbit/s)"),
            300.,
            50000.,
            q.bitrate_kbps.unwrap_or(8000),
            100.,
        );
        bitrate.set_subtitle(tr(
            "Upper limit; actual bitrate depends on content and connection",
        ));
        let fps_auto = adw::SwitchRow::builder()
            .title(tr("Automatic frame rate"))
            .active(q.fps.is_none())
            .build();
        let fps = spin(
            tr("Frame rate limit (FPS)"),
            1.,
            60.,
            q.fps.unwrap_or(60),
            1.,
        );
        fps.set_subtitle(tr(
            "Maximum display frame rate; the phone may send fewer frames",
        ));
        let rows = Self {
            bitrate_auto,
            bitrate,
            fps_auto,
            fps,
        };
        rows.refresh();
        rows
    }
    pub fn add_to(&self, group: &adw::PreferencesGroup) {
        group.add(&self.bitrate_auto);
        group.add(&self.bitrate);
        group.add(&self.fps_auto);
        group.add(&self.fps);
    }
    pub fn selection(&self) -> Quality {
        Quality {
            bitrate_kbps: (!self.bitrate_auto.is_active()).then(|| self.bitrate.value() as u32),
            fps: (!self.fps_auto.is_active()).then(|| self.fps.value() as u32),
        }
    }
    pub fn refresh(&self) {
        self.bitrate.set_sensitive(!self.bitrate_auto.is_active());
        self.fps.set_sensitive(!self.fps_auto.is_active());
    }
    pub fn set_encoder_mode(&self,external:bool) {
        self.fps.set_subtitle(tr(if external{"Requested maximum capture frame rate for scrcpy"}else{"Maximum display frame rate; the phone may send fewer frames"}));
        self.bitrate.set_subtitle(tr(if external{"scrcpy encoder target; actual bitrate depends on content and connection"}else{"Upper limit; actual bitrate depends on content and connection"}));
    }
    #[cfg(test)]
    pub(crate) fn set_selection(&self, quality: Quality) {
        if let Some(value) = quality.bitrate_kbps {
            self.bitrate.set_value(f64::from(value));
        }
        if let Some(value) = quality.fps {
            self.fps.set_value(f64::from(value));
        }
        self.bitrate_auto.set_active(quality.bitrate_kbps.is_none());
        self.fps_auto.set_active(quality.fps.is_none());
        self.refresh();
    }
    pub fn connect_changed(&self, action: impl Fn() + 'static) {
        let action: Rc<dyn Fn()> = Rc::new(action);
        for row in [&self.bitrate_auto, &self.fps_auto] {
            let action = action.clone();
            row.connect_active_notify(move |_| action());
        }
        for row in [&self.bitrate, &self.fps] {
            let action = action.clone();
            row.connect_value_notify(move |_| action());
        }
    }
}
pub fn spin(title: &str, min: f64, max: f64, value: u32, step: f64) -> adw::SpinRow {
    adw::SpinRow::builder()
        .title(title)
        .adjustment(&gtk::Adjustment::new(
            f64::from(value),
            min,
            max,
            step,
            step * 10.,
            0.,
        ))
        .digits(0)
        .numeric(true)
        .build()
}
