//! Defaults for newly started standalone app streams.
use crate::stream_settings::{FitAxis, Layout};
use crate::{app_display::AppDisplay, i18n::tr, prefs};
use adw::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

const PRESETS: [(u32, u32); 4] = [(720, 1280), (1080, 1920), (1280, 720), (1920, 1080)];
struct Form {
    custom: adw::SwitchRow,
    renderer: adw::ComboRow,
    adb_device: adw::EntryRow,
    adb_port: adw::SpinRow,
    software_decode: adw::SwitchRow,
    audio: adw::SwitchRow,
    independent_audio: adw::SwitchRow,
    wake_unlock: adw::SwitchRow,
    fold_mouse: adw::SwitchRow,
    mouse_speed: adw::SpinRow,
    phone_ime: adw::SwitchRow,
    computer_ime: adw::SwitchRow,
    preset: adw::ComboRow,
    width: adw::SpinRow,
    height: adw::SpinRow,
    density: adw::SpinRow,
    fixed: adw::SwitchRow,
    ratio_width: adw::SpinRow,
    ratio_height: adw::SpinRow,
    ratio_preset: adw::ComboRow,
    fit: adw::ComboRow,
    preview: gtk::Label,
    quality: crate::stream_controls::QualityRows,
    save: gtk::Button,
    error: gtk::Label,
    syncing: Cell<bool>,
}
impl Form {
    fn selection(&self) -> prefs::AppStreamingPrefs {
        let mut display = AppDisplay {
            width: self.width.value() as u32,
            height: self.height.value() as u32,
            density: self.density.value() as u32,
        };
        if !self.custom.is_active() && !display.valid() {
            display = AppDisplay::default();
        }
        prefs::AppStreamingPrefs {
            custom: self.custom.is_active(),
            display,
            quality: self.quality.selection(),
            layout: self.layout(),
            renderer: if self.renderer.selected() == 1 {
                crate::desktop_renderer::Renderer::Scrcpy
            } else {
                crate::desktop_renderer::Renderer::Builtin
            },
            scrcpy: crate::desktop_renderer::ScrcpyOptions {
                adb_device: self.adb_device.text().trim().into(),
                adb_port: self.adb_port.value() as u16,
                software_decode: self.software_decode.is_active(),
                audio: self.audio.is_active(),
                independent_audio: self.independent_audio.is_active(),
                wake_unlock: self.wake_unlock.is_active(),
                fold_mouse: self.fold_mouse.is_active(),
                mouse_speed_percent: self.mouse_speed.value() as u16,
                phone_ime: self.phone_ime.is_active(),
                computer_ime: self.computer_ime.is_active(),
            },
        }
    }
    fn layout(&self) -> Layout {
        Layout {
            fixed: self.fixed.is_active(),
            ratio_width: self.ratio_width.value() as u32,
            ratio_height: self.ratio_height.value() as u32,
            fit: match self.fit.selected() {
                1 => FitAxis::Height,
                2 => FitAxis::Width,
                _ => FitAxis::Manual,
            },
        }
    }
    fn recalculate(&self, width_changed: bool) {
        if self.syncing.replace(true) {
            return;
        }
        let l = self.layout();
        if l.fixed {
            let size = if l.fit != FitAxis::Manual {
                crate::mobile_desktop::native_monitor_size(&self.width)
                    .and_then(|screen| l.fitted(screen))
            } else if width_changed {
                l.height_for_width(self.width.value() as u32)
                    .map(|h| (self.width.value() as u32, h))
            } else {
                l.width_for_height(self.height.value() as u32)
                    .map(|w| (w, self.height.value() as u32))
            };
            if let Some((w, h)) = size {
                // Keep out-of-range calculated values visible instead of silently clamping/scaling.
                self.preset.set_selected(4);
                self.width.adjustment().set_lower(f64::from(w.min(320)));
                self.height.adjustment().set_lower(f64::from(h.min(320)));
                self.width.adjustment().set_upper(f64::from(w.max(3840)));
                self.height.adjustment().set_upper(f64::from(h.max(3840)));
                self.width.set_value(f64::from(w));
                self.height.set_value(f64::from(h));
            }
        }
        self.syncing.set(false);
    }
    fn refresh(&self) {
        let custom = self.custom.is_active();
        let l = self.layout();
        let selection = self.selection();
        let screen = crate::mobile_desktop::native_monitor_size(&self.width);
        let valid = selection.valid() && (l.fit == FitAxis::Manual || screen.is_some());
        self.preset.set_sensitive(custom && !l.fixed);
        self.width.set_sensitive(custom && l.fit == FitAxis::Manual);
        self.height
            .set_sensitive(custom && l.fit == FitAxis::Manual);
        self.density.set_sensitive(custom);
        self.ratio_width.set_sensitive(l.fixed);
        self.ratio_height.set_sensitive(l.fixed);
        self.ratio_preset.set_sensitive(l.fixed);
        self.fit.set_sensitive(l.fixed);
        let external = self.renderer.selected() == 1;
        self.adb_device.set_visible(external || self.wake_unlock.is_active());
        self.adb_port.set_visible(external || self.wake_unlock.is_active());
        self.software_decode.set_visible(external);
        self.audio.set_visible(external);
        self.independent_audio.set_visible(external);self.independent_audio.set_sensitive(self.audio.is_active());
        self.fold_mouse.set_visible(external);
        self.mouse_speed.set_visible(external);
        self.mouse_speed.set_sensitive(self.fold_mouse.is_active());
        self.phone_ime.set_visible(external);
        self.phone_ime.set_sensitive(!self.computer_ime.is_active());
        self.computer_ime.set_visible(external);
        self.quality.refresh();
        self.quality.set_encoder_mode(external);
        self.preview.set_label(&if let Some((sw, sh)) = screen {
            let (w, h) = (selection.display.width, selection.display.height);
            if l.fixed && l.fit != FitAxis::Manual {
                crate::tr_format!(
                    "Screen: {} × {} physical pixels; stream: {} × {} (1:1)",
                    "屏幕物理像素：{} × {}；投屏分辨率：{} × {}（点对点）",
                    sw,
                    sh,
                    w,
                    h
                )
            } else {
                crate::tr_format!(
                    "Screen: {} × {} physical pixels",
                    "屏幕物理像素：{} × {}",
                    sw,
                    sh
                )
            }
        } else {
            tr("Could not determine the monitor's physical resolution").into()
        });
        self.error.set_visible(!valid);
        self.error
            .set_label(tr(if (external || self.wake_unlock.is_active()) && !selection.scrcpy.valid() {
                "Enter a valid ADB device and port"
            } else {
                "Use even dimensions 320–3840, short side at most 2160, and density 120–640."
            }));
        self.save.set_sensitive(valid);
    }
}
fn dimension(title: &str, min: f64, max: f64, value: u32, step: f64) -> adw::SpinRow {
    adw::SpinRow::builder()
        .title(title)
        .adjustment(&gtk::Adjustment::new(
            f64::from(value),
            min,
            max,
            step,
            20.0,
            0.0,
        ))
        .digits(0)
        .numeric(true)
        .build()
}
fn build() -> (adw::PreferencesDialog, Rc<Form>) {
    let settings = prefs::load().apps.streaming.sanitized();
    let renderer = adw::ComboRow::builder()
        .title(tr("Display method"))
        .model(&gtk::StringList::new(&[
            tr("Built-in mirroring"),
            "scrcpy (ADB)",
        ]))
        .selected(
            if settings.renderer == crate::desktop_renderer::Renderer::Scrcpy {
                1
            } else {
                0
            },
        )
        .build();
    let adb_device = adw::EntryRow::builder()
        .title(tr("ADB device (empty for automatic)"))
        .text(&settings.scrcpy.adb_device)
        .build();
    let adb_port = crate::stream_controls::spin(
        tr("Wireless ADB port"),
        1.,
        65535.,
        u32::from(settings.scrcpy.adb_port),
        1.,
    );
    let software_decode = adw::SwitchRow::builder()
        .title(tr("Software decoding for scrcpy"))
        .active(settings.scrcpy.software_decode)
        .build();
    let audio=adw::SwitchRow::builder().title(tr("Play phone audio"))
        .subtitle(tr("Forward phone sound to the computer. Applies after restarting the stream; audio may include other phone apps."))
        .active(settings.scrcpy.audio).build();
    let independent_audio=adw::SwitchRow::builder().title(tr("Independent of phone volume"))
        .subtitle(tr("Capture app playback on Android 13+. Control loudness on the computer; some apps prohibit capture. Restart the stream to apply."))
        .active(settings.scrcpy.independent_audio).build();
    let wake_unlock=adw::SwitchRow::builder().title(tr("Wake and unlock with ADB before starting"))
        .subtitle(tr("Wakes the phone and dismisses an unprotected lock screen. Secure locks require authentication on the phone."))
        .active(settings.scrcpy.wake_unlock).build();
    let fold_mouse=adw::SwitchRow::builder().title(tr("Fold-compatible mouse"))
        .subtitle(tr("Uses relative mouse on Android 15+ with scrcpy 4+. Press Left Alt or Super to release."))
        .active(settings.scrcpy.fold_mouse).build();
    let mouse_speed=crate::stream_controls::spin(tr("Mouse speed (%)"),25.,400.,u32::from(settings.scrcpy.mouse_speed_percent),25.);
    mouse_speed.set_subtitle(tr("100% is normal speed. Applies to the compatibility mouse after restarting the stream."));
    let computer_ime=adw::SwitchRow::builder().title(tr("Use computer input method"))
        .subtitle(tr("Compose Chinese on the computer and send confirmed text to the phone. Requires scrcpy 5+. Restart the stream to apply."))
        .active(settings.scrcpy.computer_ime).build();
    let phone_ime=adw::SwitchRow::builder().title(tr("Use phone input method"))
        .subtitle(tr("Uses a physical keyboard on Android 15+ with scrcpy 4+. Select Chinese in the phone input method; Super+K opens keyboard settings."))
        .active(settings.scrcpy.phone_ime).build();
    let method=adw::PreferencesGroup::builder().title(tr("Display method"))
        .description(tr("scrcpy captures the app display created by Smart Connect; ADB authorization is required.")).build();
    method.add(&renderer);
    method.add(&adb_device);
    method.add(&adb_port);
    method.add(&software_decode);
    method.add(&audio);
    method.add(&independent_audio);
    method.add(&wake_unlock);
    method.add(&fold_mouse);
    method.add(&mouse_speed);
    method.add(&phone_ime);
    method.add(&computer_ime);
    let custom = adw::SwitchRow::builder()
        .title(tr("Custom app resolution"))
        .subtitle(tr("Off uses the phone's automatic app size"))
        .active(settings.custom)
        .build();
    let preset = adw::ComboRow::builder()
        .title(tr("Resolution preset"))
        .model(&gtk::StringList::new(&[
            "720 × 1280",
            "1080 × 1920",
            "1280 × 720",
            "1920 × 1080",
            tr("Custom"),
        ]))
        .selected(
            PRESETS
                .iter()
                .position(|size| *size == (settings.display.width, settings.display.height))
                .map_or(4, |i| i as u32),
        )
        .build();
    let width = dimension(tr("Width"), 320.0, 3840.0, settings.display.width, 2.0);
    let height = dimension(tr("Height"), 320.0, 3840.0, settings.display.height, 2.0);
    let density = dimension(
        tr("Display density (DPI)"),
        120.0,
        640.0,
        settings.display.density,
        20.0,
    );
    density.set_subtitle(tr("Higher values make app controls and text larger"));
    let fixed = adw::SwitchRow::builder()
        .title(tr("Fixed aspect ratio"))
        .subtitle(tr("Derived dimensions are rounded to even pixels; the other side may extend beyond the screen"))
        .active(settings.layout.fixed)
        .build();
    let ratio_width = dimension(
        tr("Aspect ratio width"),
        1.,
        10000.,
        settings.layout.ratio_width,
        1.,
    );
    let ratio_height = dimension(
        tr("Aspect ratio height"),
        1.,
        10000.,
        settings.layout.ratio_height,
        1.,
    );
    let ratio_preset = adw::ComboRow::builder()
        .title(tr("Aspect ratio preset"))
        .model(&gtk::StringList::new(&[
            "9:16",
            "16:9",
            "3:2",
            "4:3",
            "1:1",
            tr("Custom"),
        ]))
        .selected(
            [(9, 16), (16, 9), (3, 2), (4, 3), (1, 1)]
                .iter()
                .position(|p| *p == (settings.layout.ratio_width, settings.layout.ratio_height))
                .map_or(5, |n| n as u32),
        )
        .build();
    let fit = adw::ComboRow::builder()
        .title(tr("Size at fixed aspect ratio"))
        .subtitle(tr(
            "Fill uses a borderless window at one source pixel per screen pixel",
        ))
        .model(&gtk::StringList::new(&[
            tr("Manual dimensions"),
            tr("Fill screen height (1:1)"),
            tr("Fill screen width (1:1)"),
        ]))
        .selected(match settings.layout.fit {
            FitAxis::Manual => 0,
            FitAxis::Height => 1,
            FitAxis::Width => 2,
        })
        .build();
    let preview = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .css_classes(["dim-label"])
        .build();
    let quality = crate::stream_controls::QualityRows::new(settings.quality);
    let quality_group = adw::PreferencesGroup::builder()
        .title(tr("Stream quality"))
        .build();
    quality.add_to(&quality_group);
    let save = gtk::Button::builder()
        .label(tr("Save settings"))
        .css_classes(["suggested-action"])
        .valign(gtk::Align::Center)
        .build();
    let save_row = adw::ActionRow::builder()
        .title(tr("Use for newly opened app windows"))
        .build();
    save_row.add_suffix(&save);
    let group=adw::PreferencesGroup::builder().description(tr("Close an existing app window before reopening it with a new resolution. The phone may limit the size or adjust orientation.")).build();
    for row in [
        custom.upcast_ref::<gtk::Widget>(),
        preset.upcast_ref(),
        width.upcast_ref(),
        height.upcast_ref(),
        density.upcast_ref(),
        fixed.upcast_ref(),
        ratio_preset.upcast_ref(),
        ratio_width.upcast_ref(),
        ratio_height.upcast_ref(),
        fit.upcast_ref(),
        preview.upcast_ref(),
    ] {
        group.add(row);
    }
    let error = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .css_classes(["error"])
        .build();
    group.add(&error);
    let actions = adw::PreferencesGroup::new();
    actions.add(&save_row);
    let page = adw::PreferencesPage::new();
    page.add(&method);
    page.add(&group);
    page.add(&quality_group);
    page.add(&actions);
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title(tr("App streaming settings"));
    dialog.set_content_width(560);
    dialog.set_content_height(650);
    dialog.add(&page);
    let form = Rc::new(Form {
        custom,
        renderer,
        adb_device,
        adb_port,
        software_decode,
        audio,
        independent_audio,
        wake_unlock,
        fold_mouse,
        mouse_speed,
        phone_ime,
        computer_ime,
        preset,
        width,
        height,
        density,
        fixed,
        ratio_preset,
        ratio_width,
        ratio_height,
        fit,
        preview,
        quality,
        save,
        error,
        syncing: Cell::new(false),
    });
    let weak = Rc::downgrade(&form);
    form.renderer.connect_selected_notify(move |_| {
        if let Some(f) = weak.upgrade() {
            f.refresh();
        }
    });
    let weak = Rc::downgrade(&form);
    form.adb_device.connect_changed(move |_| {
        if let Some(f) = weak.upgrade() {
            f.refresh();
        }
    });
    let weak = Rc::downgrade(&form);
    form.adb_port.connect_value_notify(move |_| {
        if let Some(f) = weak.upgrade() {
            f.refresh();
        }
    });
    let weak=Rc::downgrade(&form);
    form.mouse_speed.connect_value_notify(move |_| {if let Some(f)=weak.upgrade(){f.refresh();}});
    let weak=Rc::downgrade(&form);
    form.independent_audio.connect_active_notify(move |_| {if let Some(f)=weak.upgrade(){f.refresh();}});
    let weak=Rc::downgrade(&form);
    form.audio.connect_active_notify(move |_| {if let Some(f)=weak.upgrade(){f.refresh();}});
    let weak=Rc::downgrade(&form);
    form.computer_ime.connect_active_notify(move |_| {if let Some(f)=weak.upgrade(){f.refresh();}});
    let weak=Rc::downgrade(&form);
    form.phone_ime.connect_active_notify(move |_| {if let Some(f)=weak.upgrade(){f.refresh();}});
    let weak = Rc::downgrade(&form);
    form.fold_mouse.connect_active_notify(move |_| {
        if let Some(f)=weak.upgrade(){f.refresh();}
    });
    let weak=Rc::downgrade(&form);
    form.wake_unlock.connect_active_notify(move |_| {
        if let Some(f)=weak.upgrade(){f.refresh();}
    });
    let weak=Rc::downgrade(&form);
    form.software_decode.connect_active_notify(move |_| {
        if let Some(f) = weak.upgrade() {
            f.refresh();
        }
    });
    let weak = Rc::downgrade(&form);
    form.custom.connect_active_notify(move |_| {
        if let Some(form) = weak.upgrade() {
            if !form.custom.is_active() {
                form.fixed.set_active(false);
                form.fit.set_selected(0);
            }
            form.refresh();
        }
    });
    for (row, width_changed) in [(&form.width, true), (&form.height, false)] {
        let weak = Rc::downgrade(&form);
        row.connect_value_notify(move |_| {
            if let Some(form) = weak.upgrade() {
                if form.syncing.get() {
                    return;
                }
                form.recalculate(width_changed);
                let size = (form.width.value() as u32, form.height.value() as u32);
                form.syncing.set(true);
                form.preset.set_selected(
                    PRESETS
                        .iter()
                        .position(|v| *v == size)
                        .map_or(4, |i| i as u32),
                );
                form.syncing.set(false);
                form.refresh();
            }
        });
    }
    let weak = Rc::downgrade(&form);
    form.preset.connect_selected_notify(move |row| {
        if let Some(form) = weak.upgrade() {
            if form.syncing.replace(true) {
                return;
            }
            if let Some((width, height)) = PRESETS.get(row.selected() as usize) {
                form.width.set_value(f64::from(*width));
                form.height.set_value(f64::from(*height));
            }
            form.syncing.set(false);
            form.recalculate(false);
            form.refresh();
        }
    });
    let weak = Rc::downgrade(&form);
    form.fixed.connect_active_notify(move |row| {
        if let Some(f) = weak.upgrade() {
            if row.is_active() {
                f.custom.set_active(true);
            } else {
                f.fit.set_selected(0);
            }
            f.recalculate(false);
            f.refresh();
        }
    });
    for row in [&form.ratio_width, &form.ratio_height] {
        let weak = Rc::downgrade(&form);
        row.connect_value_notify(move |_| {
            if let Some(f) = weak.upgrade() {
                if f.syncing.get() {
                    return;
                }
                f.syncing.set(true);
                f.ratio_preset.set_selected(5);
                f.syncing.set(false);
                f.recalculate(false);
                f.refresh();
            }
        });
    }
    let weak = Rc::downgrade(&form);
    form.ratio_preset.connect_selected_notify(move |row| {
        if let Some(f) = weak.upgrade() {
            if f.syncing.replace(true) {
                return;
            }
            if let Some((w, h)) =
                [(9, 16), (16, 9), (3, 2), (4, 3), (1, 1)].get(row.selected() as usize)
            {
                f.ratio_width.set_value(*w as f64);
                f.ratio_height.set_value(*h as f64);
            }
            f.syncing.set(false);
            f.recalculate(false);
            f.refresh();
        }
    });
    let weak = Rc::downgrade(&form);
    form.fit.connect_selected_notify(move |_| {
        if let Some(f) = weak.upgrade() {
            f.recalculate(false);
            f.refresh();
        }
    });
    let weak = Rc::downgrade(&form);
    form.density.connect_value_notify(move |_| {
        if let Some(f) = weak.upgrade() {
            f.refresh();
        }
    });
    let weak = Rc::downgrade(&form);
    form.quality.connect_changed(move || {
        if let Some(f) = weak.upgrade() {
            f.refresh();
        }
    });
    // Dialog lifetime owns the form; individual widget callbacks only retain weak references.
    let keep = form.clone();
    dialog.connect_closed(move |_| {
        let _ = &keep;
    });
    let state = Rc::downgrade(&form);
    let weak = dialog.downgrade();
    form.save.connect_clicked(move |_| {
        let Some(state) = state.upgrade() else { return };
        match prefs::save_app_streaming(state.selection()) {
            Ok(()) => {
                if let Some(dialog) = weak.upgrade() {
                    dialog.close();
                }
            }
            Err(error) => {
                state.error.set_label(&crate::tr_format!(
                    "Could not save app settings: {error}",
                    "无法保存应用设置：{error}"
                ));
                state.error.set_visible(true);
            }
        }
    });
    form.recalculate(false);
    form.refresh();
    (dialog, form)
}
pub fn show(parent: &impl IsA<gtk::Widget>) {
    let (dialog, _) = build();
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires desktop session and isolated XDG_CONFIG_HOME"]
    fn custom_app_geometry_validates_and_saves_without_changing_other_modes() {
        adw::init().unwrap();
        let before = prefs::load();
        let parent = adw::Window::new();
        parent.present();
        let (dialog, form) = build();
        dialog.present(Some(&parent));
        assert!(!form.selection().custom);
        form.custom.set_active(true);
        form.preset.set_selected(0);
        assert_eq!(
            form.selection().display,
            AppDisplay {
                width: 720,
                height: 1280,
                density: 320
            }
        );
        form.fixed.set_active(true);
        form.fit.set_selected(1);
        let screen = crate::mobile_desktop::native_monitor_size(&form.width).unwrap();
        assert_eq!(form.selection().display.height, screen.1);
        assert_eq!(
            form.selection().display.width,
            Layout {
                fixed: true,
                ..Layout::default()
            }
            .width_for_height(screen.1)
            .unwrap()
        );
        form.ratio_width.set_value(1.);
        form.ratio_height.set_value(100.);
        form.fit.set_selected(2);
        assert!(!form.save.is_sensitive());
        form.fixed.set_active(false);
        form.preset.set_selected(0);
        form.width.set_value(721.0);
        assert!(!form.save.is_sensitive());
        form.width.set_value(720.0);
        form.density.set_value(240.0);
        assert!(form.save.is_sensitive());
        form.quality.set_selection(crate::stream_settings::Quality {
            bitrate_kbps: Some(2500),
            fps: Some(24),
        });
        form.renderer.set_selected(1);
        assert!(form.adb_device.is_visible());
        form.adb_device.set_text("invalid device");
        assert!(!form.save.is_sensitive());
        form.adb_device.set_text("");
        assert!(form.save.is_sensitive());
        form.wake_unlock.set_active(true);
        assert!(form.fold_mouse.is_active());
        assert!(form.audio.is_active());assert!(!form.independent_audio.is_active());
        form.independent_audio.set_active(true);form.audio.set_active(false);assert!(!form.independent_audio.is_sensitive());
        assert!(form.phone_ime.is_active());
        assert!(!form.computer_ime.is_active());
        form.computer_ime.set_active(true);assert!(!form.phone_ime.is_sensitive());
        form.mouse_speed.set_value(175.);form.phone_ime.set_active(false);
        form.fold_mouse.set_active(false);assert!(!form.mouse_speed.is_sensitive());
        form.save.emit_clicked();
        let saved = prefs::load();
        assert!(saved.apps.streaming.scrcpy.wake_unlock);
        assert_eq!(saved.apps.streaming.scrcpy.mouse_speed_percent,175);
        assert!(!saved.apps.streaming.scrcpy.phone_ime);
        assert!(saved.apps.streaming.scrcpy.computer_ime);
        assert!(!saved.apps.streaming.scrcpy.audio);assert!(saved.apps.streaming.scrcpy.independent_audio);
        assert!(!saved.apps.streaming.scrcpy.fold_mouse);
        assert_eq!(
            saved.apps.streaming.renderer,
            crate::desktop_renderer::Renderer::Scrcpy
        );
        assert_eq!(saved.apps.streaming.display.density, 240);
        assert_eq!(saved.apps.streaming.quality.fps, Some(24));
        assert_eq!(saved.apps.streaming.quality.bitrate_kbps, Some(2500));
        assert_eq!(saved.desktop, before.desktop);
        assert_eq!(saved.display, before.display);
        assert_eq!(saved.language, before.language);
        assert_eq!(saved.apps.recent, before.apps.recent);
        parent.destroy();
    }
}
