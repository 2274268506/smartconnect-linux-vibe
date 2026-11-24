//! Phone desktop controls. This page never changes the PC-to-phone Display configuration.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::apps::{self, Session};
use crate::i18n::tr;
use crate::prefs::{self, DesktopPrefs};

const PRESETS: [(u32, u32); 4] = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
const SCALES: [u32; 8] = [100, 125, 150, 175, 200, 225, 250, 300];
const START_TIMEOUT: Duration = Duration::from_secs(25);
const CUSTOM: u32 = PRESETS.len() as u32 + 1;

pub struct DesktopPage {
    resolution: adw::ComboRow,
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
    quality: crate::stream_controls::QualityRows,
    scale: adw::ComboRow,
    width: adw::SpinRow,
    height: adw::SpinRow,
    validation: gtk::Label,
    status: gtk::Label,
    actual: adw::ActionRow,
    borderless: adw::SwitchRow,
    start: gtk::Button,
    apply: gtk::Button,
    cancel: gtk::Button,
    start_control: RefCell<Option<std::sync::Arc<apps::StartControl>>>,
    generation: Cell<u64>,
    cancelling: Cell<bool>,
    toasts: adw::ToastOverlay,
    connected: Cell<bool>,
    visible: Cell<bool>,
    busy: Cell<bool>,
    polling: Cell<bool>,
    syncing: Cell<bool>,
    pending: Cell<Option<Instant>>,
    session: RefCell<Option<Session>>,
    error: RefCell<Option<String>>,
    query_error: RefCell<Option<String>>,
}

impl DesktopPage {
    pub fn build(toasts: &adw::ToastOverlay) -> (Rc<Self>, gtk::Widget) {
        let settings = prefs::load().desktop.sanitized();
        let title = gtk::Label::builder()
            .label(tr("Desktop mode"))
            .xalign(0.0)
            .css_classes(["title-2"])
            .build();
        let description = gtk::Label::builder()
            .label(tr("Use your phone's mobile desktop in a window on this PC"))
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label"])
            .build();
        let status = gtk::Label::builder().xalign(0.0).wrap(true).build();
        let start = gtk::Button::builder()
            .label(tr("Start desktop mode"))
            .css_classes(["suggested-action"])
            .build();
        let apply = gtk::Button::builder()
            .label(tr("Apply and restart desktop"))
            .visible(false)
            .build();
        let buttons = gtk::Box::builder().spacing(12).build();
        buttons.append(&start);
        buttons.append(&apply);
        let cancel=gtk::Button::builder().label(tr("Cancel")).visible(false).build();
        buttons.append(&cancel);
        let logs=gtk::Button::with_label(tr("View mirroring logs"));
        logs.connect_clicked(|button| { if let Some(window)=button.root().and_downcast::<gtk::Window>() { crate::logs::show(&window); } });
        buttons.append(&logs);
        let renderer=adw::ComboRow::builder().title(tr("Display method"))
            .subtitle(tr("scrcpy uses authorized ADB and keeps the complete phone desktop"))
            .model(&gtk::StringList::new(&[tr("Built-in mirroring"),"scrcpy (ADB)"]))
            .selected(if settings.renderer==crate::desktop_renderer::Renderer::Scrcpy{1}else{0}).build();
        let adb_device=adw::EntryRow::builder().title(tr("ADB device (empty for automatic)"))
            .text(&settings.scrcpy.adb_device).show_apply_button(true).build();
        let adb_port=crate::stream_controls::spin(tr("Wireless ADB port"),1.,65535.,u32::from(settings.scrcpy.adb_port),1.);
        adb_port.set_subtitle(tr("Used when automatically connecting to the paired phone"));
        let software_decode=adw::SwitchRow::builder().title(tr("Software decoding for scrcpy"))
            .subtitle(tr("Enable if hardware decoding causes a crash or black screen"))
            .active(settings.scrcpy.software_decode).build();
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
            .description(tr("Install scrcpy and adb first. Wireless ADB must already be enabled and authorized on the phone.")).build();
        method.add(&renderer);method.add(&adb_device);method.add(&adb_port);method.add(&software_decode);method.add(&audio);method.add(&independent_audio);method.add(&wake_unlock);method.add(&fold_mouse);method.add(&mouse_speed);method.add(&phone_ime);method.add(&computer_ime);
        let resolution = adw::ComboRow::builder()
            .title(tr("Desktop resolution"))
            .model(&gtk::StringList::new(&[
                tr("Use computer resolution"),
                "1280 × 720 · HD",
                "1920 × 1080 · Full HD",
                "2560 × 1440 · QHD",
                "3840 × 2160 · 4K",
                tr("Custom"),
            ]))
            .selected(if settings.follow_computer {
                0
            } else {
                PRESETS
                    .iter()
                    .position(|&(w, h)| (w, h) == (settings.width, settings.height))
                    .map_or(CUSTOM, |index| index as u32 + 1)
            })
            .build();
        let width = dimension_row(tr("Width"), 640.0, 3840.0, settings.width);
        let height = dimension_row(tr("Height"), 360.0, 2160.0, settings.height);
        let custom = resolution.selected() == CUSTOM;
        width.set_visible(custom);
        height.set_visible(custom);
        let actual = adw::ActionRow::builder()
            .title(tr("Actual resolution"))
            .subtitle("—")
            .build();
        let group = adw::PreferencesGroup::builder()
            .title(tr("Desktop resolution"))
            .description(tr(
                "The phone may limit the requested size. Its reported resolution appears below.",
            ))
            .build();
        group.add(&resolution);
        group.add(&width);
        group.add(&height);
        group.add(&actual);
        let scale = adw::ComboRow::builder()
            .title(tr("App and text scaling"))
            .subtitle(tr("Changes app controls and text after restarting desktop mode"))
            .model(&gtk::StringList::new(&[
                tr("Follow computer scaling"), "100%", "125%", "150%", "175%", "200%", "225%", "250%", "300%",
            ]))
            .selected(settings.scale_percent.and_then(|value| SCALES.iter().position(|v| *v == value))
                .map_or(0, |index| index as u32 + 1))
            .build();
        group.add(&scale);
        let borderless = adw::SwitchRow::builder()
            .title(tr("Borderless mode"))
            .subtitle(tr("Hide the desktop window's title bar and borders"))
            .active(settings.borderless)
            .build();
        group.add(&borderless);
        let quality = crate::stream_controls::QualityRows::new(settings.quality);
        let quality_group = adw::PreferencesGroup::builder().title(tr("Stream quality")).build();
        quality.add_to(&quality_group);
        let validation = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["error"])
            .visible(false)
            .build();
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();
        for widget in [
            title.upcast_ref::<gtk::Widget>(),
            description.upcast_ref(),
            method.upcast_ref(),
            group.upcast_ref(),
            quality_group.upcast_ref(),
            validation.upcast_ref(),
            status.upcast_ref(),
            buttons.upcast_ref(),
        ] {
            column.append(widget);
        }
        let clamp = adw::Clamp::builder().maximum_size(900).child(&column).build();
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&clamp)
            .build();
        let page = Rc::new(Self {
            renderer,adb_device,adb_port,software_decode,audio,independent_audio,wake_unlock,fold_mouse,mouse_speed,phone_ime,computer_ime,
            quality,
            resolution,
            scale,
            width,
            height,
            validation,
            status,
            actual,
            borderless,
            start,
            apply,
            toasts: toasts.clone(),
            connected: Cell::new(false),
            visible: Cell::new(false),
            busy: Cell::new(false),
            polling: Cell::new(false),
            syncing: Cell::new(false),
            cancel, start_control: RefCell::new(None),generation:Cell::new(0),cancelling:Cell::new(false),
            pending: Cell::new(None),
            session: RefCell::new(None),
            error: RefCell::new(None),
            query_error: RefCell::new(None),
        });
        let weak=Rc::downgrade(&page);page.renderer.connect_selected_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.adb_device.connect_changed(move |_|{if let Some(page)=weak.upgrade(){page.refresh();}});
        let weak=Rc::downgrade(&page);page.adb_device.connect_apply(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.adb_port.connect_value_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.mouse_speed.connect_value_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.independent_audio.connect_active_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.audio.connect_active_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.computer_ime.connect_active_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.phone_ime.connect_active_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.fold_mouse.connect_active_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.wake_unlock.connect_active_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak=Rc::downgrade(&page);page.software_decode.connect_active_notify(move |_|{if let Some(page)=weak.upgrade(){page.selection_changed();}});
        let weak = Rc::downgrade(&page);
        page.resolution.connect_selected_notify(move |_| {
            if let Some(page) = weak.upgrade() {
                page.selection_changed();
            }
        });
        let weak = Rc::downgrade(&page);
        page.scale.connect_selected_notify(move |_| {
            if let Some(page) = weak.upgrade() { page.selection_changed(); }
        });
        for row in [&page.width, &page.height] {
            let weak = Rc::downgrade(&page);
            row.connect_value_notify(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.selection_changed();
                }
            });
        }
        let weak = Rc::downgrade(&page);
        page.borderless.connect_active_notify(move |_| {
            if let Some(page) = weak.upgrade() {
                page.borderless_changed();
            }
        });
        let weak = Rc::downgrade(&page);
        page.cancel.connect_clicked(move |_| {
            if let Some(page)=weak.upgrade(){page.cancel_start();}
        });
        let weak = Rc::downgrade(&page);
        page.start.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.operate(false);
            }
        });
        let weak = Rc::downgrade(&page);
        page.apply.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.operate(true);
            }
        });
        let weak = Rc::downgrade(&page);
        glib::timeout_add_seconds_local(1, move || {
            let Some(page) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            page.poll();
            glib::ControlFlow::Continue
        });
        let weak = Rc::downgrade(&page);
        page.quality.connect_changed(move || { if let Some(page)=weak.upgrade() {page.quality.refresh();page.selection_changed();} });
        page.refresh();
        (page, root.upcast())
    }

    fn selection(&self) -> DesktopPrefs {
        let mut settings = if self.resolution.selected() == 0 {
            let (width, height) = computer_resolution(&self.resolution).unwrap_or((1920, 1080));
            DesktopPrefs {
                width,
                height,
                ..DesktopPrefs::default()
            }
        } else {
            PRESETS.get(self.resolution.selected() as usize - 1).map_or_else(
                || DesktopPrefs::fixed(self.width.value() as u32, self.height.value() as u32),
                |&(width, height)| DesktopPrefs::fixed(width, height),
            )
        };
        settings.renderer=if self.renderer.selected()==1{crate::desktop_renderer::Renderer::Scrcpy}else{crate::desktop_renderer::Renderer::Builtin};
        settings.scrcpy=crate::desktop_renderer::ScrcpyOptions{adb_device:self.adb_device.text().trim().into(),adb_port:self.adb_port.value() as u16,software_decode:self.software_decode.is_active(),audio:self.audio.is_active(),independent_audio:self.independent_audio.is_active(),wake_unlock:self.wake_unlock.is_active(),fold_mouse:self.fold_mouse.is_active(),mouse_speed_percent:self.mouse_speed.value() as u16,phone_ime:self.phone_ime.is_active(),computer_ime:self.computer_ime.is_active()};
        settings.quality = self.quality.selection();
        settings.borderless = self.borderless.is_active();
        settings.scale_percent = self.scale.selected().checked_sub(1).and_then(|index| SCALES.get(index as usize).copied());
        settings
    }

    fn selected_scale(&self) -> u32 {
        self.selection().scale_percent.unwrap_or_else(|| {
            monitor_for_widget(&self.scale).map(|monitor| (monitor.scale() * 100.0).round() as u32)
                .unwrap_or(100).clamp(100, 300)
        })
    }

    fn selection_changed(&self) {
        if self.syncing.replace(true) {
            return;
        }
        if let Some(&(width, height)) = self
            .resolution
            .selected()
            .checked_sub(1)
            .and_then(|i| PRESETS.get(i as usize))
        {
            self.width.set_value(f64::from(width));
            self.height.set_value(f64::from(height));
        }
        let custom = self.resolution.selected() == CUSTOM;
        self.width.set_visible(custom);
        self.height.set_visible(custom);
        self.syncing.set(false);
        let settings = self.selection();
        if settings.valid() {
            match prefs::save_desktop(settings) {
                Ok(()) => {
                    self.error.borrow_mut().take();
                }
                Err(error) => {
                    *self.error.borrow_mut() = Some(crate::tr_format!(
                        "Could not save desktop settings: {error}",
                        "无法保存桌面设置：{error}"
                    ));
                }
            }
        }
        self.refresh();
    }

    fn borderless_changed(self: &Rc<Self>) {
        if self.syncing.get() {
            return;
        }
        let wanted = self.borderless.is_active();
        self.selection_changed();
        if self.session.borrow().as_ref().is_some_and(|s|s.renderer==crate::desktop_renderer::Renderer::Scrcpy){return;}
        if self.session.borrow().is_none() || self.busy.get() {
            return;
        }
        self.busy.set(true);
        self.refresh();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || apps::set_desktop_borderless(wanted))
                .await
                .unwrap_or_else(|_| Err(tr("Could not change the desktop window").into()));
            let Some(page) = weak.upgrade() else { return };
            page.busy.set(false);
            match result {
                Ok(_) => {
                    if let Some(session) = page.session.borrow_mut().as_mut() {
                        session.borderless = Some(wanted);
                    }
                }
                Err(error) => {
                    page.syncing.set(true);
                    page.borderless.set_active(!wanted);
                    page.syncing.set(false);
                    page.selection_changed();
                    page.toasts.add_toast(adw::Toast::new(&error));
                    *page.error.borrow_mut() = Some(error);
                }
            }
            page.refresh();
            page.poll();
        });
    }

    pub fn set_connected(self: &Rc<Self>, connected: bool) {
        if self.connected.replace(connected) != connected {
            if !connected {
                self.pending.set(None);
            }
            self.poll();
        }
        self.refresh();
    }

    pub fn set_visible(self: &Rc<Self>, visible: bool) {
        self.visible.set(visible);
        if visible {
            self.poll();
        }
    }

    fn poll(self: &Rc<Self>) {
        if !self.visible.get() && self.pending.get().is_none() && self.session.borrow().is_none() {
            return;
        }
        if self.polling.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);let generation=self.generation.get();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(apps::desktop_status)
                .await
                .unwrap_or_else(|_| Err(tr("Could not query desktop mode").into()));
            let Some(page) = weak.upgrade() else { return };
            page.polling.set(false);
            if page.generation.get()!=generation {return;}
            match result {
                Ok((session,host_pending)) => {
                    page.query_error.borrow_mut().take();
                    if host_pending && page.pending.get().is_none() && !page.busy.get() {page.pending.set(Some(Instant::now()));}
                    let starting=session.as_ref().is_some_and(|s|s.renderer==crate::desktop_renderer::Renderer::Scrcpy&&s.scrcpy_state.as_deref()!=Some("running"));
                    if !host_pending&&!starting&&!page.busy.get() {page.pending.set(None);page.start_control.borrow_mut().take();}
                    *page.session.borrow_mut() = session;
                }
                Err(error) => {
                    page.pending.set(None);page.session.borrow_mut().take();
                    *page.query_error.borrow_mut() = Some(error);
                }
            }
            if page.pending.get().is_some_and(|since| since.elapsed() >= if page.renderer.selected()==1{Duration::from_secs(45)}else{START_TIMEOUT}) {
                page.cancel_start();
                *page.error.borrow_mut() =
                    Some(tr("The phone did not start desktop mode. Unlock it and try again.").into());
            }
            page.refresh();
        });
    }

    fn cancel_start(self:&Rc<Self>) {
        if self.cancelling.replace(true) {return;}
        self.generation.set(self.generation.get()+1);
        let control=self.start_control.borrow().clone();
        if let Some(control)=&control {control.cancel();}
        self.busy.set(true);self.error.borrow_mut().take();self.refresh();
        let weak=Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result=gio::spawn_blocking(move ||apps::cancel_start("desktop",control.as_deref())).await.unwrap_or_else(|_|Err(tr("Could not cancel startup").into()));
            let Some(page)=weak.upgrade()else{return;};
            page.cancelling.set(false);page.busy.set(false);page.pending.set(None);
            page.session.borrow_mut().take();page.start_control.borrow_mut().take();
            if let Err(error)=result {*page.error.borrow_mut()=Some(error);}
            page.refresh();page.poll();
        });
    }

    fn operate(self: &Rc<Self>, restart: bool) {
        if self.busy.get() {return;}
        let stop = if restart {
            None
        } else {
            self.session.borrow().as_ref().map(|session| session.session_id)
        };
        if stop.is_none()&&self.pending.get().is_some(){return;}
        let mut settings = self.selection();
        if stop.is_none()&&settings.valid() {
            if let Err(error)=prefs::save_desktop(settings.clone()) {
                self.toasts.add_toast(adw::Toast::new(&error.to_string()));return;
            }
        }
        settings.scale_percent = Some(self.selected_scale());
        if stop.is_none() && (!self.connected.get() || !settings.valid()) {
            return;
        }
        self.busy.set(true);
        let control=std::sync::Arc::new(apps::StartControl::default());
        if stop.is_none() {*self.start_control.borrow_mut()=Some(control.clone());}
        self.generation.set(self.generation.get()+1);let generation=self.generation.get();
        self.error.borrow_mut().take();
        self.refresh();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || match stop {
                Some(id) => apps::close(id),
                None => apps::start_desktop(settings, restart,&control),
            })
            .await
            .unwrap_or_else(|_| Err(tr("Desktop mode request failed").into()));
            let Some(page) = weak.upgrade() else { return };
            if page.generation.get()!=generation {return;}
            page.busy.set(false);
            match result {
                Ok(_) => {
                    page.session.borrow_mut().take();
                    page.pending
                        .set(if stop.is_none() { Some(Instant::now()) } else { None });
                }
                Err(error) => {
                    page.toasts.add_toast(adw::Toast::new(&error));
                    *page.error.borrow_mut() = Some(error);
                }
            }
            page.refresh();
            page.poll();
        });
    }

    fn refresh(&self) {
        let settings = self.selection();
        let valid = settings.valid();
        self.resolution.set_subtitle(&if settings.follow_computer {
            crate::tr_format!(
                "Computer screen: {} × {}",
                "电脑屏幕：{} × {}",
                settings.width,
                settings.height
            )
        } else {
            String::new()
        });
        self.validation.set_visible(!valid);
        self.validation.set_label(tr(if (settings.renderer==crate::desktop_renderer::Renderer::Scrcpy||settings.scrcpy.wake_unlock)&&!settings.scrcpy.valid(){"Enter a valid ADB device and port"}else{
            "Use landscape dimensions with even pixels: width 640–3840, height 360–2160."
        }));
        let session = self.session.borrow();
        let active = session.is_some();
        let starting=session.as_ref().is_some_and(|s|s.renderer==crate::desktop_renderer::Renderer::Scrcpy&&s.scrcpy_state.as_deref()!=Some("running"));
        let waiting = self.busy.get() || self.pending.get().is_some() || starting;
        self.start.set_label(if self.busy.get() {
            tr("Working…")
        } else if active {
            tr("Stop desktop mode")
        } else {
            tr("Start desktop mode")
        });
        self.cancel.set_visible(self.start_control.borrow().is_some() || self.pending.get().is_some() || starting);
        self.cancel.set_sensitive(!self.cancelling.get());
        self.cancel.set_label(tr(if self.cancelling.get(){"Cancelling…"}else{"Cancel"}));
        self.start
            .set_sensitive(!self.busy.get() && (active || !waiting && self.connected.get() && valid));
        let changed = session.as_ref().is_some_and(|s| {
            s.requested_width.zip(s.requested_height) != Some((settings.width, settings.height))
                || s.borderless.unwrap_or(false) != settings.borderless
                || s.requested_scale.unwrap_or(100) != self.selected_scale()
                || s.requested_quality != settings.quality
                || s.renderer!=settings.renderer
                || s.fullscreen!=(settings.follow_computer&&settings.renderer==crate::desktop_renderer::Renderer::Scrcpy)
                || (s.renderer==crate::desktop_renderer::Renderer::Scrcpy&&s.requested_scrcpy!=settings.scrcpy)
        });
        self.apply.set_visible(active);
        self.apply
            .set_sensitive(active && changed && valid && !waiting && self.connected.get());
        self.renderer.set_sensitive(!waiting);
        let external=settings.renderer==crate::desktop_renderer::Renderer::Scrcpy;
        self.renderer.set_subtitle(tr(if external&&settings.follow_computer{"Desktop fills the screen when following computer resolution"}else{"scrcpy uses authorized ADB and keeps the complete phone desktop"}));
        self.adb_device.set_visible(external||settings.scrcpy.wake_unlock);self.adb_port.set_visible(external||settings.scrcpy.wake_unlock);self.software_decode.set_visible(external);self.fold_mouse.set_visible(external);
        self.audio.set_visible(external);self.audio.set_sensitive(!waiting);
        self.independent_audio.set_visible(external);self.independent_audio.set_sensitive(!waiting && self.audio.is_active());
        self.fold_mouse.set_sensitive(!waiting);
        self.mouse_speed.set_visible(external);
        self.mouse_speed.set_sensitive(!waiting && self.fold_mouse.is_active());
        self.phone_ime.set_visible(external);
        self.phone_ime.set_sensitive(!waiting && !self.computer_ime.is_active());
        self.computer_ime.set_visible(external);self.computer_ime.set_sensitive(!waiting);
        self.wake_unlock.set_sensitive(!waiting);
        self.adb_device.set_sensitive(!waiting);self.adb_port.set_sensitive(!waiting);self.software_decode.set_sensitive(!waiting);
        self.quality.set_encoder_mode(external);
        self.resolution.set_sensitive(!waiting);
        self.scale.set_sensitive(!waiting);
        self.width.set_sensitive(!waiting);
        self.height.set_sensitive(!waiting);
        self.borderless.set_sensitive(!waiting);
        let actual = session
            .as_ref()
            .and_then(|s| s.width.zip(s.height))
            .map_or_else(|| "—".into(), |(w, h)| {
                session.as_ref().and_then(|s| s.density).map_or_else(|| format!("{w} × {h}"),
                    |dpi| format!("{w} × {h} · {dpi} DPI"))
            });
        self.actual.set_subtitle(&actual);
        let error = self.error.borrow();
        let query_error = self.query_error.borrow();
        self.status.set_label(if let Some(error) = error.as_deref() {
            error
        } else if let Some(error) = query_error.as_deref() {
            error
        } else if !self.connected.get() {
            tr("Connect the phone to use desktop mode")
        } else if self.cancelling.get() {
            tr("Cancelling…")
        } else if starting {
            tr("Connecting ADB and starting scrcpy…")
        } else if waiting {
            tr("Waiting for the phone to start desktop mode…")
        } else if active && session.as_ref().is_some_and(|s|s.renderer==crate::desktop_renderer::Renderer::Scrcpy) {
            tr("Complete desktop is running in scrcpy")
        } else if active {
            tr("Desktop mode is running in its own window")
        } else {
            tr("Choose a resolution, then start desktop mode")
        });
    }
}

// GDK geometry is in logical pixels; fractional monitor scale converts it to actual pixels.
fn monitor_for_widget(widget: &impl IsA<gtk::Widget>) -> Option<gdk::Monitor> {
    let display = widget.as_ref().display();
    let monitor = widget
        .as_ref()
        .native()
        .and_then(|native| native.surface())
        .and_then(|surface| display.monitor_at_surface(&surface))
        .or_else(|| display.monitors().item(0)?.downcast::<gdk::Monitor>().ok())?;
    Some(monitor)
}

pub(crate) fn native_monitor_size(widget: &impl IsA<gtk::Widget>) -> Option<(u32,u32)> {
    let monitor=monitor_for_widget(widget)?;let rect=monitor.geometry();let scale=monitor.scale();
    if rect.width()<=0 || rect.height()<=0 || !scale.is_finite() || scale<=0. {return None;}
    Some((((f64::from(rect.width())*scale).round() as u32)/2*2,((f64::from(rect.height())*scale).round() as u32)/2*2))
}

fn computer_resolution(widget: &impl IsA<gtk::Widget>) -> Option<(u32, u32)> {
    let monitor = monitor_for_widget(widget)?;
    let geometry = monitor.geometry();
    fit_computer_resolution(geometry.width(), geometry.height(), monitor.scale())
}

fn fit_computer_resolution(width: i32, height: i32, scale: f64) -> Option<(u32, u32)> {
    if width <= 0 || height <= 0 || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let (width, height) = (
        f64::from(width.max(height)) * scale,
        f64::from(width.min(height)) * scale,
    );
    let factor = (3840.0 / width).min(2160.0 / height).min(1.0);
    let even = |value: f64, min: u32| ((value.floor() as u32) / 2 * 2).max(min);
    Some((even(width * factor, 640), even(height * factor, 360)))
}

fn dimension_row(title: &str, min: f64, max: f64, value: u32) -> adw::SpinRow {
    adw::SpinRow::builder()
        .title(title)
        .subtitle(tr("Pixels"))
        .adjustment(&gtk::Adjustment::new(f64::from(value), min, max, 2.0, 20.0, 0.0))
        .digits(0)
        .numeric(true)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a desktop session and an isolated XDG_CONFIG_HOME"]
    fn app_scaling_saves_separately_and_requires_desktop_restart() {
        adw::init().unwrap();
        let before = prefs::load().display;
        let (page, _root) = DesktopPage::build(&adw::ToastOverlay::new());
        assert_eq!(page.selection().scale_percent,None);
        page.set_connected(true);
        let selected = page.selection();
        *page.session.borrow_mut() = Some(Session {
            requested_width:Some(selected.width), requested_height:Some(selected.height),
            requested_scale:Some(100), borderless:Some(false), ..Session::default()
        });
        page.scale.set_selected(5);
        assert_eq!(page.selected_scale(),200);
        assert_eq!(prefs::load().desktop.scale_percent,Some(200));
        assert!(page.apply.is_sensitive(), "new app density needs a new virtual display");
        page.scale.set_selected(2);
        assert_eq!(page.selected_scale(),125);
        page.scale.set_selected(0);
        assert_eq!(prefs::load().desktop.scale_percent,None);
        assert!((100..=300).contains(&page.selected_scale()));
        assert_eq!(prefs::load().display,before);
    }

    #[test]
    #[ignore = "requires a desktop session and isolated XDG_CONFIG_HOME/XDG_RUNTIME_DIR"]
    fn startup_cancel_is_available_before_a_session_and_clears_async_state() {
        adw::init().unwrap();
        assert!(!std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap()).join("smartconnect-mirror.sock").exists());
        let (page,_root)=DesktopPage::build(&adw::ToastOverlay::new());
        page.connected.set(true);page.busy.set(true);
        let control=std::sync::Arc::new(apps::StartControl::default());
        *page.start_control.borrow_mut()=Some(control.clone());
        page.refresh();assert!(page.cancel.is_visible());assert!(page.cancel.is_sensitive());
        page.cancel.emit_clicked();assert!(control.cancelled());assert!(!page.cancel.is_sensitive());
        let deadline=Instant::now()+Duration::from_secs(3);
        while page.cancelling.get() && Instant::now()<deadline {
            while glib::MainContext::default().pending(){glib::MainContext::default().iteration(false);}
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!page.cancelling.get());assert!(!page.busy.get());assert!(page.pending.get().is_none());
        assert!(!page.cancel.is_visible());assert!(page.start.is_sensitive());
    }

    #[test]
    fn computer_size_uses_fractional_scale_and_fits_codec_bounds() {
        assert_eq!(fit_computer_resolution(1920, 1280, 1.5), Some((2880, 1920)));
        assert_eq!(fit_computer_resolution(5120, 2880, 1.0), Some((3840, 2160)));
        assert_eq!(fit_computer_resolution(1080, 1920, 1.0), Some((1920, 1080)));
        assert_eq!(fit_computer_resolution(1365, 767, 1.0), Some((1364, 766)));
        assert_eq!(fit_computer_resolution(0, 720, 1.0), None);
        assert_eq!(fit_computer_resolution(1920, 1080, f64::NAN), None);
    }

    #[test]
    #[ignore = "requires a desktop session and an isolated XDG_CONFIG_HOME"]
    fn desktop_presets_and_custom_resolution_save_without_touching_display() {
        adw::init().unwrap();
        prefs::save_language(crate::i18n::Language::Chinese).unwrap();
        let display = prefs::load().display;
        let (page, _root) = DesktopPage::build(&adw::ToastOverlay::new());
        assert_eq!(page.resolution.selected(), 0);
        assert!(page.selection().follow_computer);
        assert!(page.selection().valid());
        page.set_connected(true);
        page.resolution.set_selected(1);
        assert_eq!(prefs::load().desktop, DesktopPrefs::fixed(1280, 720));
        assert!(!page.width.is_visible());
        assert!(page.start.is_sensitive());
        page.resolution.set_selected(CUSTOM);
        assert!(page.width.is_visible());
        page.width.set_value(1281.0);
        assert!(page.validation.is_visible());
        assert!(!page.start.is_sensitive());
        assert_eq!(prefs::load().desktop, DesktopPrefs::fixed(1280, 720));
        page.width.set_value(2560.0);
        page.height.set_value(1440.0);
        assert!(!page.validation.is_visible());
        assert!(page.start.is_sensitive());
        assert_eq!(prefs::load().desktop, DesktopPrefs::fixed(2560, 1440));
        assert_eq!(prefs::load().display, display);
        assert_eq!(prefs::load().language, crate::i18n::Language::Chinese);
        let (reopened, _root) = DesktopPage::build(&adw::ToastOverlay::new());
        assert_eq!(reopened.selection(), DesktopPrefs::fixed(2560, 1440));
        assert_eq!(reopened.resolution.selected(), 3);
        reopened.resolution.set_selected(0);
        reopened.borderless.set_active(true);
        let saved = prefs::load().desktop;
        assert!(saved.follow_computer);
        assert!(saved.borderless);
        let (automatic, _root) = DesktopPage::build(&adw::ToastOverlay::new());
        assert_eq!(automatic.resolution.selected(), 0);
        assert!(automatic.borderless.is_active());
        assert_eq!(automatic.selection(), saved);
        automatic.quality.set_selection(crate::stream_settings::Quality{bitrate_kbps:Some(3000),fps:Some(30)});
        let quality_saved=prefs::load().desktop;
        assert_eq!(quality_saved.quality.fps,Some(30));assert_eq!(quality_saved.quality.bitrate_kbps,Some(3000));
        assert_eq!(prefs::load().display,display);
        automatic.renderer.set_selected(1);
        assert!(automatic.adb_device.is_visible());
        automatic.adb_device.set_text("unit-device");automatic.adb_port.set_value(5557.);automatic.software_decode.set_active(true);
        assert!(automatic.fold_mouse.is_active());assert!(automatic.phone_ime.is_active());
        assert!(automatic.audio.is_active());assert!(!automatic.independent_audio.is_active());
        automatic.independent_audio.set_active(true);automatic.audio.set_active(false);assert!(!automatic.independent_audio.is_sensitive());
        automatic.computer_ime.set_active(true);assert!(!automatic.phone_ime.is_sensitive());
        automatic.mouse_speed.set_value(250.);automatic.phone_ime.set_active(false);
        automatic.fold_mouse.set_active(false);assert!(!automatic.mouse_speed.is_sensitive());
        let saved=prefs::load().desktop;assert_eq!(saved.renderer,crate::desktop_renderer::Renderer::Scrcpy);
        assert_eq!(saved.scrcpy.adb_port,5557);assert_eq!(saved.scrcpy.adb_device,"unit-device");assert!(saved.scrcpy.software_decode);assert!(!saved.scrcpy.fold_mouse);
        assert_eq!(saved.scrcpy.mouse_speed_percent,250);assert!(!saved.scrcpy.phone_ime);assert!(saved.scrcpy.computer_ime);assert!(!saved.scrcpy.audio);assert!(saved.scrcpy.independent_audio);
        automatic.adb_device.set_text("invalid device");assert!(!automatic.selection().valid());assert_eq!(prefs::load().desktop,saved);
        automatic.adb_device.set_text("unit-device");
        let (external,_root)=DesktopPage::build(&adw::ToastOverlay::new());assert_eq!(external.renderer.selected(),1);assert_eq!(external.selection(),saved);
        external.renderer.set_selected(0);assert!(!external.adb_device.is_visible());
        external.wake_unlock.set_active(true);assert!(external.adb_device.is_visible());
        assert!(prefs::load().desktop.scrcpy.wake_unlock);assert!(external.selection().valid());
        external.adb_device.set_text("invalid device");assert!(!external.selection().valid());
        external.adb_device.set_text("unit-device");external.adb_device.emit_by_name::<()>("apply",&[]);
        assert_eq!(prefs::load().display,display);
    }
}
