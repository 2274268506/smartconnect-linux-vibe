//! File tools subpage. All ADB I/O runs on a worker; GTK only consumes bounded events.
use crate::{
    adb_transfer::{self as adb, Cancel, Entry},
    i18n::tr,
};
use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

enum Job {
    Devices,
    Connect(String),
    List(String, String),
    Upload(String, String, Vec<PathBuf>),
    Download(String, Vec<String>, PathBuf),
}
enum Event {
    Devices(Vec<String>),
    Listing(String, Vec<Entry>),
    Progress(usize, usize),
    Done(Result<(), String>),
}

pub struct AdbFilesPage {
    pub widget: gtk::Box,
    devices: gtk::DropDown,
    model: gtk::StringList,
    address: gtk::Entry,
    connect: gtk::Button,
    refresh: gtk::Button,
    path: gtk::Entry,
    up: gtk::Button,
    browse: gtk::Button,
    list: gtk::ListBox,
    upload: gtk::Button,
    download: gtk::Button,
    clear_selection: gtk::Button,
    checks: RefCell<Vec<(gtk::ListBoxRow, gtk::CheckButton)>>,
    progress: gtk::ProgressBar,
    cancel_button: gtk::Button,
    status: gtk::Label,
    entries: RefCell<Vec<Entry>>,
    folder: RefCell<String>,
    cancel: RefCell<Option<Cancel>>,
    busy: Cell<bool>,
    syncing: Cell<bool>,
}
impl AdbFilesPage {
    pub fn build() -> Rc<Self> {
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .build();
        let hint=gtk::Label::builder().label(tr("Enable USB or network debugging and authorize this computer on the phone. Only shared storage is accessible."))
            .wrap(true).xalign(0.0).build();
        widget.append(&hint);
        let model = gtk::StringList::new(&[]);
        let devices = gtk::DropDown::new(Some(model.clone()), None::<gtk::Expression>);
        devices.set_hexpand(true);
        let refresh = gtk::Button::with_label(&tr("Refresh devices"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&devices);
        row.append(&refresh);
        widget.append(&row);
        let address = gtk::Entry::builder()
            .placeholder_text(tr("ADB address (host:port)"))
            .hexpand(true)
            .build();
        let connect = gtk::Button::with_label(&tr("Connect"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&address);
        row.append(&connect);
        widget.append(&row);
        let path = gtk::Entry::builder().text("/sdcard").hexpand(true).build();
        let up = gtk::Button::with_label(&tr("Up"));
        let browse = gtk::Button::with_label(&tr("Open folder"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&up);
        row.append(&path);
        row.append(&browse);
        widget.append(&row);
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Multiple)
            .activate_on_single_click(false)
            .css_classes(["boxed-list"])
            .build();
        let clear_selection = gtk::Button::with_label(tr("Clear selection"));
        clear_selection.set_halign(gtk::Align::End);
        widget.append(&clear_selection);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .min_content_height(180)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        widget.append(&scroll);
        let upload = gtk::Button::with_label(&tr("Upload files…"));
        let download = gtk::Button::with_label(&tr("Download selected…"));
        let cancel_button = gtk::Button::with_label(&tr("Cancel"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&upload);
        row.append(&download);
        row.append(&cancel_button);
        widget.append(&row);
        let progress = gtk::ProgressBar::new();
        widget.append(&progress);
        let status=gtk::Label::builder().label(tr("Open a folder by double-clicking. Select files to download. Existing files are never overwritten."))
            .wrap(true).selectable(true).xalign(0.0).build();
        widget.append(&status);
        let page = Rc::new(Self {
            widget,
            devices,
            model,
            address,
            connect,
            refresh,
            path,
            up,
            browse,
            list,
            upload,
            download,
            clear_selection,
            checks: RefCell::new(Vec::new()),
            progress,
            cancel_button,
            status,
            entries: RefCell::new(Vec::new()),
            folder: RefCell::new("/sdcard".into()),
            cancel: RefCell::new(None),
            busy: Cell::new(false),
            syncing: Cell::new(false),
        });
        let w = Rc::downgrade(&page);
        page.refresh.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.refresh_devices()
            }
        });
        let w = Rc::downgrade(&page);
        page.connect.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.start(Job::Connect(p.address.text().trim().into()));
            }
        });
        let w = Rc::downgrade(&page);
        page.devices.connect_selected_notify(move |_| {
            if let Some(p) = w.upgrade() {
                if !p.syncing.get() {
                    p.clear();
                    p.browse_path("/sdcard".into());
                }
            }
        });
        let w = Rc::downgrade(&page);
        page.browse.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.browse_path(p.path.text().into())
            }
        });
        let w = Rc::downgrade(&page);
        page.path.connect_changed(move |_| {
            if let Some(p) = w.upgrade() {
                p.controls()
            }
        });
        let w = Rc::downgrade(&page);
        page.path.connect_activate(move |_| {
            if let Some(p) = w.upgrade() {
                p.browse_path(p.path.text().into())
            }
        });
        let w = Rc::downgrade(&page);
        page.up.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                let folder = p.folder.borrow().clone();
                let parent = folder
                    .rsplit_once('/')
                    .map(|v| v.0)
                    .filter(|v| v.starts_with("/sdcard"))
                    .unwrap_or("/sdcard");
                p.browse_path(parent.into());
            }
        });
        let w = Rc::downgrade(&page);
        page.list.connect_row_activated(move |_, row| {
            if let Some(p) = w.upgrade() {
                let entry = p.entries.borrow().get(row.index() as usize).cloned();
                if let Some(e) = entry.filter(|e| e.directory) {
                    p.browse_path(e.path);
                }
            }
        });
        let w = Rc::downgrade(&page);
        page.list.connect_selected_rows_changed(move |_| {
            if let Some(p) = w.upgrade() {
                p.controls()
            }
        });
        let w = Rc::downgrade(&page);
        page.clear_selection.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                if !p.busy.get() {
                    p.list.unselect_all();
                }
            }
        });
        let keys = gtk::EventControllerKey::new();
        let w = Rc::downgrade(&page);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(p) = w.upgrade() {
                    if !p.busy.get() && !p.list.selected_rows().is_empty() {
                        p.list.unselect_all();
                        return glib::Propagation::Stop;
                    }
                }
            }
            glib::Propagation::Proceed
        });
        page.list.add_controller(keys);
        let w = Rc::downgrade(&page);
        page.cancel_button.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.shutdown();
                p.cancel_button.set_sensitive(false);
                p.status.set_text(&tr("Cancelling…"));
            }
        });
        let w = Rc::downgrade(&page);
        page.upload.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.choose_upload()
            }
        });
        let w = Rc::downgrade(&page);
        page.download.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.choose_download()
            }
        });
        page.controls();
        page
    }
    fn device(&self) -> Option<String> {
        self.model
            .string(self.devices.selected())
            .map(|s| s.to_string())
    }
    fn clear(&self) {
        self.checks.borrow_mut().clear();
        self.entries.borrow_mut().clear();
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        self.controls();
    }
    fn show_listing(&self, path: String, entries: Vec<Entry>) {
        self.clear();
        self.path.set_text(&path);
        *self.folder.borrow_mut() = path;
        for e in &entries {
            let row = adw::ActionRow::builder()
                .title(e.name())
                .use_markup(false)
                .activatable(true)
                .selectable(!e.directory)
                .build();
            if !e.directory {
                let check = gtk::CheckButton::builder()
                    .valign(gtk::Align::Center)
                    .tooltip_text(e.name())
                    .build();
                check.update_property(&[gtk::accessible::Property::Label(e.name())]);
                let weak_list = self.list.downgrade();
                let weak_row = row.downgrade();
                check.connect_toggled(move |check| {
                    if let (Some(list), Some(row)) = (weak_list.upgrade(), weak_row.upgrade()) {
                        if check.is_active() != row.is_selected() {
                            if check.is_active() {
                                list.select_row(Some(&row));
                            } else {
                                list.unselect_row(&row);
                            }
                        }
                    }
                });
                row.add_prefix(&check);
                self.checks.borrow_mut().push((row.clone().upcast(), check));
            }
            row.add_prefix(&gtk::Image::from_icon_name(if e.directory {
                "folder-symbolic"
            } else {
                "text-x-generic-symbolic"
            }));
            self.list.append(&row);
        }
        *self.entries.borrow_mut() = entries;
        self.controls();
    }
    fn controls(&self) {
        let idle = !self.busy.get();
        let device = self.device().is_some();
        self.devices.set_sensitive(idle);
        self.refresh.set_sensitive(idle);
        self.address.set_sensitive(idle);
        self.connect.set_sensitive(idle);
        self.path.set_sensitive(idle && device);
        self.browse.set_sensitive(idle && device);
        self.up.set_sensitive(idle && device);
        self.list.set_sensitive(idle && device);
        self.upload.set_sensitive(
            idle && device && self.path.text().as_str() == self.folder.borrow().as_str(),
        );
        self.download
            .set_sensitive(idle && device && !self.selected().is_empty());
        self.clear_selection
            .set_sensitive(idle && !self.list.selected_rows().is_empty());
        // Keep native Ctrl/Shift and keyboard selection in sync with the explicit checkboxes.
        for (row, check) in self.checks.borrow().iter() {
            check.set_active(row.is_selected());
        }
        self.cancel_button.set_sensitive(!idle);
        self.progress.set_visible(!idle);
    }
    fn selected(&self) -> Vec<String> {
        let entries = self.entries.borrow();
        self.list
            .selected_rows()
            .iter()
            .filter_map(|r| entries.get(r.index() as usize))
            .filter(|e| !e.directory)
            .map(|e| e.path.clone())
            .collect()
    }
    pub fn refresh_devices(self: &Rc<Self>) {
        if !self.busy.get() {
            self.start(Job::Devices)
        }
    }
    fn browse_path(self: &Rc<Self>, path: String) {
        if let Some(device) = self.device() {
            self.start(Job::List(device, path));
        }
    }
    pub fn shutdown(&self) {
        if let Some(c) = self.cancel.borrow().as_ref() {
            c.store(true, Ordering::Relaxed);
        }
    }
    fn start(self: &Rc<Self>, job: Job) {
        if self.busy.replace(true) {
            return;
        }
        let transfer = matches!(job, Job::Upload(..) | Job::Download(..));
        let scan = matches!(job, Job::Devices | Job::Connect(..));
        let cancel = Arc::new(AtomicBool::new(false));
        *self.cancel.borrow_mut() = Some(cancel.clone());
        self.status.set_text(&tr("Working…"));
        self.progress.set_fraction(0.0);
        self.controls();
        let (tx, rx) = async_channel::bounded(16);
        let pulse_cancel = cancel.clone();
        std::thread::spawn(move || {
            let result = (|| -> adb::Result<()> {
                match job {
                    Job::Devices => {
                        let ids = adb::devices(&cancel)?;
                        let _ = tx.send_blocking(Event::Devices(ids));
                    }
                    Job::Connect(address) => {
                        adb::connect(&address, &cancel)?;
                        let ids = adb::devices(&cancel)?;
                        let _ = tx.send_blocking(Event::Devices(ids));
                    }
                    Job::List(device, path) => {
                        let path = adb::remote_path(&path)?;
                        let entries = adb::list(&device, &path, &cancel)?;
                        let _ = tx.send_blocking(Event::Listing(path, entries));
                    }
                    Job::Upload(device, path, files) => {
                        let total = files.len();
                        for (i, file) in files.iter().enumerate() {
                            let _ = tx.send_blocking(Event::Progress(i, total));
                            adb::upload(&device, file, &path, &cancel)?;
                            let _ = tx.send_blocking(Event::Progress(i + 1, total));
                        }
                    }
                    Job::Download(device, files, path) => {
                        let total = files.len();
                        for (i, file) in files.iter().enumerate() {
                            let _ = tx.send_blocking(Event::Progress(i, total));
                            adb::download(&device, file, &path, &cancel)?;
                            let _ = tx.send_blocking(Event::Progress(i + 1, total));
                        }
                    }
                }
                Ok(())
            })();
            let _ = tx.send_blocking(Event::Done(result));
        });
        let w = Rc::downgrade(self);
        glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
            let Some(p) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !p
                .cancel
                .borrow()
                .as_ref()
                .is_some_and(|c| Arc::ptr_eq(c, &pulse_cancel))
            {
                return glib::ControlFlow::Break;
            }
            p.progress.pulse();
            glib::ControlFlow::Continue
        });
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(event) = rx.recv().await {
                let Some(p) = w.upgrade() else { break };
                match event {
                    Event::Devices(ids) => {
                        let previous = p.device();
                        p.syncing.set(true);
                        p.model.splice(
                            0,
                            p.model.n_items(),
                            &ids.iter().map(String::as_str).collect::<Vec<_>>(),
                        );
                        let selected = previous
                            .and_then(|id| ids.iter().position(|v| v == &id))
                            .or_else(|| (ids.len() == 1).then_some(0));
                        p.devices.set_selected(
                            selected
                                .map(|i| i as u32)
                                .unwrap_or(gtk::INVALID_LIST_POSITION),
                        );
                        p.syncing.set(false);
                        p.clear();
                    }
                    Event::Listing(path, entries) => {
                        p.show_listing(path, entries);
                    }
                    Event::Progress(done, total) => {
                        p.progress.set_fraction(done as f64 / total.max(1) as f64);
                        p.status
                            .set_text(&format!("{} {done}/{total}", tr("Files completed:")));
                    }
                    Event::Done(result) => {
                        p.busy.set(false);
                        p.cancel.borrow_mut().take();
                        p.controls();
                        match result {
                            Ok(()) => {
                                p.status.set_text(&tr(if scan&&p.model.n_items()==0{"No authorized ADB device. Unlock the phone and accept the debugging prompt, then refresh."}else if scan&&p.device().is_none(){"Select an ADB device."}else{"Completed"}));
                                if (scan || transfer) && p.device().is_some() {
                                    let path = if scan {
                                        "/sdcard".into()
                                    } else {
                                        p.folder.borrow().clone()
                                    };
                                    p.browse_path(path);
                                }
                            }
                            Err(e) => {
                                if e == "Cancelled" {
                                    p.status.set_text(tr("Cancelled"));
                                } else {
                                    p.status.set_text(&format!(
                                        "{}: {}",
                                        tr("Failed"),
                                        tr(e.strip_prefix("ADB: ").unwrap_or(&e))
                                    ));
                                }
                            }
                        }
                        break;
                    }
                }
            }
        });
    }
    fn choose_upload(self: &Rc<Self>) {
        let Some(device) = self.device() else { return };
        let path = self.folder.borrow().clone();
        let parent = self.widget.root().and_downcast::<gtk::Window>();
        let w = Rc::downgrade(self);
        gtk::FileDialog::builder()
            .title(tr("Upload files…"))
            .build()
            .open_multiple(parent.as_ref(), None::<&gio::Cancellable>, move |result| {
                let Some(p) = w.upgrade() else { return };
                if p.busy.get()
                    || p.device().as_ref() != Some(&device)
                    || *p.folder.borrow() != path
                {
                    return;
                }
                if let Ok(model) = result {
                    let files = (0..model.n_items())
                        .filter_map(|i| {
                            model
                                .item(i)
                                .and_downcast::<gio::File>()
                                .and_then(|f| f.path())
                        })
                        .collect::<Vec<_>>();
                    if !files.is_empty() {
                        p.start(Job::Upload(device, path, files));
                    }
                }
            });
    }
    fn choose_download(self: &Rc<Self>) {
        let Some(device) = self.device() else { return };
        let files = self.selected();
        if files.is_empty() {
            return;
        }
        let parent = self.widget.root().and_downcast::<gtk::Window>();
        let w = Rc::downgrade(self);
        gtk::FileDialog::builder()
            .title(tr("Download selected…"))
            .build()
            .select_folder(parent.as_ref(), None::<&gio::Cancellable>, move |result| {
                let Some(p) = w.upgrade() else { return };
                if p.busy.get() || p.device().as_ref() != Some(&device) {
                    return;
                }
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        p.start(Job::Download(device, files, path));
                    }
                }
            });
    }
}
impl Drop for AdbFilesPage {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a desktop session"]
    fn idle_page_and_cancellation_controls() {
        adw::init().unwrap();
        let page = AdbFilesPage::build();
        assert!(!page.busy.get());
        assert!(page.cancel.borrow().is_none());
        assert!(!page.upload.is_sensitive());
        assert!(!page.download.is_sensitive());
        assert!(!page.cancel_button.is_sensitive());
        assert!(page.refresh.is_sensitive());
        // Inject a selected authorized device without launching ADB.
        page.syncing.set(true);
        page.model.append("test-device");
        page.devices.set_selected(0);
        page.syncing.set(false);
        page.controls();
        assert!(page.upload.is_sensitive());
        page.show_listing(
            "/sdcard".into(),
            vec![
                Entry {
                    path: "/sdcard/first.txt".into(),
                    directory: false,
                },
                Entry {
                    path: "/sdcard/second.txt".into(),
                    directory: false,
                },
                Entry {
                    path: "/sdcard/Download".into(),
                    directory: true,
                },
            ],
        );
        assert!(!page.list.activates_on_single_click());
        let first = page.checks.borrow()[0].1.clone();
        let second = page.checks.borrow()[1].1.clone();
        first.set_active(true);
        second.set_active(true);
        assert_eq!(page.selected().len(), 2);
        assert!(page.download.is_sensitive());
        first.set_active(false);
        assert_eq!(page.selected(), vec!["/sdcard/second.txt"]);
        // Native selection must also update the checkboxes.
        page.list.unselect_all();
        assert!(!second.is_active());
        let row = page.list.row_at_index(0).unwrap();
        page.list.select_row(Some(&row));
        assert!(first.is_active());
        assert!(page.clear_selection.is_sensitive());
        page.clear_selection.emit_clicked();
        assert!(page.selected().is_empty());
        assert!(!first.is_active());
        assert!(!page.download.is_sensitive());
        assert!(!page.clear_selection.is_sensitive());
        page.busy.set(true);
        let cancel = Arc::new(AtomicBool::new(false));
        *page.cancel.borrow_mut() = Some(cancel.clone());
        page.controls();
        assert!(!page.devices.is_sensitive());
        assert!(!page.upload.is_sensitive());
        assert!(page.cancel_button.is_sensitive());
        page.cancel_button.emit_clicked();
        assert!(adb::cancelled(&cancel));
        assert!(!page.cancel_button.is_sensitive());
    }
}
