//! Persistent mirror diagnostics, usable even while the phone or stream host is offline.
use crate::{i18n::tr, mirror_log};
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

thread_local! { static OPEN: RefCell<glib::WeakRef<adw::Window>> = RefCell::default(); }

struct Viewer {
    window: glib::WeakRef<adw::Window>,
    text: gtk::TextView,
    search: gtk::SearchEntry,
    warnings: gtk::CheckButton,
    pause: gtk::ToggleButton,
    follow: gtk::CheckButton,
    status: gtk::Label,
    raw: RefCell<String>,
    shown: RefCell<String>,
    loading: Cell<bool>,
    closed: Cell<bool>,
}

pub fn show(parent: &impl IsA<gtk::Window>) {
    if let Some(window) = OPEN.with(|slot| slot.borrow().upgrade()) {
        window.present();
        return;
    }
    let window = adw::Window::builder()
        .title(tr("Mirroring logs"))
        .transient_for(parent)
        .default_width(960)
        .default_height(640)
        .build();
    OPEN.with(|slot| *slot.borrow_mut() = window.downgrade());
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(tr("Mirroring logs"), "")));
    let refresh = gtk::Button::with_label(tr("Refresh"));
    let copy = gtk::Button::with_label(tr("Copy logs"));
    let export = gtk::Button::with_label(tr("Export logs…"));
    let folder = gtk::Button::from_icon_name("folder-open-symbolic");
    folder.set_tooltip_text(Some(tr("Open log folder")));
    for button in [&folder, &export, &copy, &refresh] {
        header.pack_end(button);
    }
    let search = gtk::SearchEntry::builder()
        .placeholder_text(tr("Search logs"))
        .hexpand(true)
        .build();
    let warnings = gtk::CheckButton::with_label(tr("Warnings and errors"));
    let follow = gtk::CheckButton::with_label(tr("Follow latest"));
    follow.set_active(true);
    let pause = gtk::ToggleButton::with_label(tr("Pause updates"));
    let controls = gtk::Box::builder()
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();
    controls.append(&search);
    controls.append(&warnings);
    controls.append(&follow);
    controls.append(&pause);
    let text = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .left_margin(12)
        .right_margin(12)
        .top_margin(12)
        .bottom_margin(12)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .child(&text)
        .vexpand(true)
        .hexpand(true)
        .build();
    let location = gtk::Label::builder()
        .label(mirror_log::path().to_string_lossy().as_ref())
        .selectable(true)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["dim-label", "caption"])
        .margin_start(12)
        .margin_end(12)
        .build();
    let status = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    body.append(&controls);
    body.append(&scroll);
    body.append(&location);
    body.append(&status);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));
    window.set_content(Some(&toolbar));
    let viewer = Rc::new(Viewer {
        window: window.downgrade(),
        text,
        search,
        warnings,
        pause,
        follow,
        status,
        raw: RefCell::default(),
        shown: RefCell::default(),
        loading: Cell::new(false),
        closed: Cell::new(false),
    });
    // The window keeps state alive; state only weakly references the window, avoiding a cycle.
    let model = viewer.clone();
    window.connect_close_request(move |_| {
        model.closed.set(true);
        OPEN.with(|slot| *slot.borrow_mut() = glib::WeakRef::new());
        glib::Propagation::Proceed
    });
    let weak = Rc::downgrade(&viewer);
    viewer.search.connect_search_changed(move |_| {
        if let Some(v) = weak.upgrade() {
            v.render();
        }
    });
    let weak = Rc::downgrade(&viewer);
    viewer.warnings.connect_toggled(move |_| {
        if let Some(v) = weak.upgrade() {
            v.render();
        }
    });
    let weak = Rc::downgrade(&viewer);
    viewer.follow.connect_toggled(move |_| {
        if let Some(v) = weak.upgrade() {
            v.scroll_to_end();
        }
    });
    let weak = Rc::downgrade(&viewer);
    viewer.pause.connect_toggled(move |button| {
        button.set_label(if button.is_active() {
            tr("Resume updates")
        } else {
            tr("Pause updates")
        });
        if let Some(v) = weak.upgrade() {
            if !button.is_active() {
                v.load(false);
            }
        }
    });
    let weak = Rc::downgrade(&viewer);
    refresh.connect_clicked(move |_| {
        if let Some(v) = weak.upgrade() {
            v.load(true);
        }
    });
    let weak = Rc::downgrade(&viewer);
    copy.connect_clicked(move |_| {
        if let Some(v) = weak.upgrade() {
            v.text.display().clipboard().set_text(&v.filtered());
            v.status.set_label(tr("Logs copied"));
        }
    });
    let weak = Rc::downgrade(&viewer);
    export.connect_clicked(move |_| {
        if let Some(v) = weak.upgrade() {
            v.export();
        }
    });
    let weak = Rc::downgrade(&viewer);
    folder.connect_clicked(move |_| {
        let Some(model) = weak.upgrade() else { return };
        let parent = model.window.upgrade();
        glib::spawn_future_local(async move {
            let created =
                gio::spawn_blocking(|| std::fs::create_dir_all(mirror_log::directory())).await;
            if !matches!(created, Ok(Ok(()))) {
                model.status.set_label(tr("Could not open log folder"));
                return;
            }
            let uri = gio::File::for_path(mirror_log::directory()).uri();
            if let Err(error) = gtk::UriLauncher::new(&uri)
                .launch_future(parent.as_ref())
                .await
            {
                model.status.set_label(&crate::tr_format!(
                    "Could not open log folder: {error}",
                    "无法打开日志文件夹：{error}"
                ));
            }
        });
    });
    let weak = Rc::downgrade(&viewer);
    glib::timeout_add_seconds_local(1, move || {
        let Some(v) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if v.closed.get() {
            return glib::ControlFlow::Break;
        }
        if !v.pause.is_active() {
            v.load(false);
        }
        glib::ControlFlow::Continue
    });
    viewer.load(false);
    window.present();
}

impl Viewer {
    fn filtered(&self) -> String {
        mirror_log::filter(
            &self.raw.borrow(),
            self.search.text().as_str(),
            self.warnings.is_active(),
        )
    }
    fn scroll_to_end(&self) {
        if !self.follow.is_active() {
            return;
        }
        let view = self.text.clone();
        glib::idle_add_local_once(move || {
            let mut end = view.buffer().end_iter();
            view.scroll_to_iter(&mut end, 0.0, false, 0.0, 1.0);
        });
    }
    fn render(&self) {
        let filtered = self.filtered();
        let display = if self.raw.borrow().is_empty() {
            tr("No logs yet. Start mirroring to record diagnostics.").into()
        } else {
            filtered.clone()
        };
        if *self.shown.borrow() != display {
            self.text.buffer().set_text(&display);
            *self.shown.borrow_mut() = display;
            self.scroll_to_end();
        }
        self.status.set_label(&crate::tr_format!(
            "{} visible lines · Export includes retained matches",
            "显示 {} 行 · 按当前筛选导出保留的日志",
            filtered.lines().count()
        ));
    }
    fn load(self: &Rc<Self>, force: bool) {
        if self.loading.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result =
                gio::spawn_blocking(|| mirror_log::read_recent(mirror_log::VIEW_LIMIT)).await;
            let Some(v) = weak.upgrade() else { return };
            v.loading.set(false);
            if v.closed.get() || v.pause.is_active() && !force {
                return;
            }
            match result {
                Ok(Ok(text)) => {
                    *v.raw.borrow_mut() = text;
                    v.render();
                }
                Ok(Err(error)) => v.status.set_label(&crate::tr_format!(
                    "Could not read logs: {error}",
                    "无法读取日志：{error}"
                )),
                Err(_) => v.status.set_label(tr("Could not read logs")),
            }
        });
    }
    fn export(self: &Rc<Self>) {
        let Some(parent) = self.window.upgrade() else {
            return;
        };
        let query = self.search.text().to_string();
        let warnings = self.warnings.is_active();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let dialog = gtk::FileDialog::builder()
                .title(tr("Export mirroring logs"))
                .initial_name("smartconnect-mirror.log")
                .build();
            let file = match dialog.save_future(Some(&parent)).await {
                Ok(file) => file,
                Err(_) => return,
            };
            let result = save_export(&file, query, warnings).await;
            if let Some(v) = weak.upgrade() {
                match result {
                    Ok(_) => v.status.set_label(tr("Logs exported")),
                    Err(error) => v.status.set_label(&crate::tr_format!(
                        "Could not export logs: {error}",
                        "无法导出日志：{error}"
                    )),
                }
            }
        });
    }
}

async fn save_export(file: &gio::File, query: String, warnings: bool) -> Result<(), String> {
    let text = gio::spawn_blocking(move || {
        mirror_log::read_recent(mirror_log::RETAINED_LIMIT)
            .map(|text| mirror_log::filter(&text, &query, warnings))
    })
    .await
    .map_err(|_| tr("Could not read logs").to_owned())?
    .map_err(|error| error.to_string())?;
    file.replace_contents_future(
        text.into_bytes(),
        None,
        false,
        gio::FileCreateFlags::PRIVATE,
    )
    .await
    .map(|_| ())
    .map_err(|(_, error)| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    fn find<T: IsA<gtk::Widget> + glib::types::StaticType + Clone>(
        widget: &gtk::Widget,
    ) -> Option<T> {
        if let Ok(found) = widget.clone().downcast::<T>() {
            return Some(found);
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(found) = find(&widget) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }
    fn spin_until(mut ready: impl FnMut() -> bool) {
        let context = glib::MainContext::default();
        let until = Instant::now() + Duration::from_secs(5);
        while !ready() && Instant::now() < until {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ready(), "log viewer did not update");
    }
    #[test]
    #[ignore = "requires desktop session and isolated XDG_STATE_HOME"]
    fn live_log_view_search_pause_resume_close_and_reopen() {
        adw::init().unwrap();
        mirror_log::append(
            &mirror_log::directory(),
            "2026-01-01T12:34:56Z [INFO] started\n2026-01-01T12:34:57Z [ERROR] decoder example\n",
        )
        .unwrap();
        let parent = adw::Window::new();
        show(&parent);
        let window = OPEN.with(|slot| slot.borrow().upgrade()).unwrap();
        let view = find::<gtk::TextView>(window.upcast_ref()).unwrap();
        let buffer = view.buffer();
        let text = || {
            buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string()
        };
        spin_until(|| text().contains("decoder example"));
        assert!(
            text().contains("12:34:56Z"),
            "timestamps must remain readable"
        );
        let search = find::<gtk::SearchEntry>(window.upcast_ref()).unwrap();
        search.set_text("decoder");
        spin_until(|| !text().contains("started"));
        search.set_text("");
        spin_until(|| text().contains("started"));
        let pause = find::<gtk::ToggleButton>(window.upcast_ref()).unwrap();
        pause.set_active(true);
        mirror_log::append(
            &mirror_log::directory(),
            "2026-01-01T12:34:58Z [WARN] newly appended\n",
        )
        .unwrap();
        let until = Instant::now() + Duration::from_millis(1300);
        while Instant::now() < until {
            glib::MainContext::default().iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!text().contains("newly appended"));
        pause.set_active(false);
        spin_until(|| text().contains("newly appended"));
        let target = mirror_log::directory().join("export-test.log");
        glib::MainContext::default()
            .block_on(save_export(
                &gio::File::for_path(&target),
                "decoder".into(),
                true,
            ))
            .unwrap();
        let exported = std::fs::read_to_string(&target).unwrap();
        assert!(exported.contains("decoder example"));
        assert!(!exported.contains("started"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o600
        );

        window.close();
        assert!(OPEN.with(|slot| slot.borrow().upgrade()).is_none());
        show(&parent);
        let reopened = OPEN.with(|slot| slot.borrow().upgrade()).unwrap();
        assert_ne!(reopened, window);
        reopened.close();
        parent.destroy();
    }
}
