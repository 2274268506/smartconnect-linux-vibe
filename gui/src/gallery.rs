//! Paged ADB photo browser. No startup scan, persistent image cache, or phone-media writes.
use crate::{
    adb_transfer::{self as adb, Cancel, Entry},
    gallery_transfer::Thumbnails,
    i18n::tr,
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
const PAGE: usize = 24;
fn photos(entries: &[Entry]) -> Vec<Entry> {
    let mut out: Vec<_> = entries
        .iter()
        .filter(|e| !e.directory && crate::gallery_transfer::is_image(&e.path))
        .cloned()
        .collect();
    out.sort_by(|a, b| b.name().cmp(a.name()));
    out
}
fn page_range(total: usize, page: usize) -> std::ops::Range<usize> {
    let start = page.min(total.saturating_sub(1) / PAGE) * PAGE;
    start..(start + PAGE).min(total)
}
fn selected_device(
    ids: &[String],
    previous: Option<&str>,
    automatic: Option<&str>,
    preferred: Option<&str>,
) -> Option<usize> {
    let primary = if previous.is_none() || previous == automatic {
        preferred
    } else {
        previous
    };
    primary
        .and_then(|id| ids.iter().position(|value| value == id))
        .or_else(|| preferred.and_then(|id| ids.iter().position(|value| value == id)))
        .or_else(|| (ids.len() == 1).then_some(0))
}
enum Job {
    Devices,
    List(String, String, usize),
    Preview(String, String),
    Save(String, String, std::path::PathBuf),
}
enum Event {
    Devices(Vec<String>, Option<String>),
    Listing(Vec<Entry>, usize),
    Thumbnail(usize, adb::Result<Vec<u8>>),
    Preview(String, Vec<u8>),
    Done(adb::Result<()>),
}
pub struct GalleryPage {
    pub widget: gtk::Box,
    devices: gtk::DropDown,
    model: gtk::StringList,
    refresh: gtk::Button,
    path: gtk::Entry,
    open: gtk::Button,
    up: gtk::Button,
    folders: gtk::Box,
    grid: gtk::FlowBox,
    previous: gtk::Button,
    next: gtk::Button,
    counter: gtk::Label,
    status: gtk::Label,
    cancel_button: gtk::Button,
    images: RefCell<Vec<(gtk::Picture, gtk::Image)>>,
    cache: Arc<Mutex<crate::gallery_cache::ThumbnailCache>>,
    current: Cell<usize>,
    total: Cell<usize>,
    generation: Cell<u64>,
    busy: Cell<bool>,
    unavailable: Cell<usize>,
    syncing: Cell<bool>,
    cancel: RefCell<Option<Cancel>>,
    last_peer: RefCell<Option<String>>,
    automatic_device: RefCell<Option<String>>,
    pending_refresh: Cell<bool>,
    dialogs: RefCell<Vec<adw::Dialog>>,
}
impl GalleryPage {
    pub fn build() -> Rc<Self> {
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .build();
        let model = gtk::StringList::new(&[]);
        let devices = gtk::DropDown::new(Some(model.clone()), None::<gtk::Expression>);
        devices.set_hexpand(true);
        let refresh = gtk::Button::with_label(tr("Refresh devices"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&devices);
        row.append(&refresh);
        widget.append(&row);
        let shortcuts = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let mut albums = Vec::new();
        for (name, path) in [
            ("Camera", "/sdcard/DCIM"),
            ("Pictures", "/sdcard/Pictures"),
            ("Downloads", "/sdcard/Download"),
        ] {
            let button = gtk::Button::with_label(tr(name));
            shortcuts.append(&button);
            albums.push((button, path));
        }
        widget.append(&shortcuts);
        let path = gtk::Entry::builder()
            .text("/sdcard/DCIM")
            .hexpand(true)
            .build();
        let open = gtk::Button::with_label(tr("Open folder"));
        let up = gtk::Button::with_label(tr("Up"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&up);
        row.append(&path);
        row.append(&open);
        widget.append(&row);
        let folders = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let grid = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .column_spacing(8)
            .row_spacing(8)
            .min_children_per_line(1)
            .max_children_per_line(6)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.append(&folders);
        content.append(&grid);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .min_content_height(200)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        widget.append(&scroll);
        let previous = gtk::Button::with_label(tr("Previous"));
        let next = gtk::Button::with_label(tr("Next"));
        let counter = gtk::Label::builder().hexpand(true).build();
        let cancel_button = gtk::Button::with_label(tr("Cancel"));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        for button in [&previous, &next] {
            row.append(button);
        }
        row.append(&counter);
        row.append(&cancel_button);
        widget.append(&row);
        let status = gtk::Label::builder()
            .label(tr("Open Gallery to browse photos using authorized ADB."))
            .wrap(true)
            .xalign(0.)
            .build();
        widget.append(&status);
        let page = Rc::new(Self {
            widget,
            devices,
            model,
            refresh,
            path,
            open,
            up,
            folders,
            grid,
            previous,
            next,
            counter,
            status,
            cancel_button,
            images: RefCell::default(),
            cache: Arc::default(),
            current: Cell::new(0),
            total: Cell::new(0),
            generation: Cell::new(0),
            busy: Cell::new(false),
            unavailable: Cell::new(0),
            syncing: Cell::new(false),
            cancel: RefCell::default(),
            last_peer: RefCell::default(),
            automatic_device: RefCell::default(),
            pending_refresh: Cell::new(false),
            dialogs: RefCell::default(),
        });
        let w = Rc::downgrade(&page);
        page.widget.connect_map(move |_| {
            if let Some(p) = w.upgrade() {
                p.refresh_devices();
            }
        });
        let w = Rc::downgrade(&page);
        page.widget.connect_unmap(move |_| {
            if let Some(p) = w.upgrade() {
                p.shutdown();
            }
        });
        let w = Rc::downgrade(&page);
        page.refresh.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.cache.lock().unwrap().clear();
                p.refresh_devices();
            }
        });
        let w = Rc::downgrade(&page);
        page.devices.connect_selected_notify(move |_| {
            if let Some(p) = w.upgrade() {
                if !p.syncing.get() {
                    p.automatic_device.borrow_mut().take();
                    p.load(0);
                }
            }
        });
        let w = Rc::downgrade(&page);
        page.open.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.load(0);
            }
        });
        let w = Rc::downgrade(&page);
        page.path.connect_activate(move |_| {
            if let Some(p) = w.upgrade() {
                p.load(0);
            }
        });
        let w = Rc::downgrade(&page);
        page.up.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                let path = p.path.text();
                if let Ok(path) = adb::remote_path(&path) {
                    p.path.set_text(
                        path.rsplit_once('/')
                            .map(|(d, _)| if d.is_empty() { "/sdcard" } else { d })
                            .filter(|d| d.starts_with("/sdcard"))
                            .unwrap_or("/sdcard"),
                    );
                    p.load(0);
                }
            }
        });
        for (button, path) in albums {
            let w = Rc::downgrade(&page);
            button.connect_clicked(move |_| {
                if let Some(p) = w.upgrade() {
                    p.path.set_text(path);
                    p.load(0);
                }
            });
        }
        let w = Rc::downgrade(&page);
        page.previous.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.load(p.current.get().saturating_sub(1));
            }
        });
        let w = Rc::downgrade(&page);
        page.next.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.load(p.current.get() + 1);
            }
        });
        let w = Rc::downgrade(&page);
        page.cancel_button.connect_clicked(move |_| {
            if let Some(p) = w.upgrade() {
                p.shutdown();
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
    fn controls(&self) {
        let idle = !self.busy.get();
        let connected = self.device().is_some();
        self.devices.set_sensitive(idle);
        self.path.set_sensitive(idle);
        self.refresh.set_sensitive(idle);
        self.open.set_sensitive(idle && connected);
        self.up.set_sensitive(idle && connected);
        self.previous.set_sensitive(idle && self.current.get() > 0);
        self.next
            .set_sensitive(idle && (self.current.get() + 1) * PAGE < self.total.get());
        self.cancel_button.set_sensitive(!idle);
    }
    fn clear(&self) {
        while let Some(child) = self.grid.first_child() {
            self.grid.remove(&child);
        }
        while let Some(child) = self.folders.first_child() {
            self.folders.remove(&child);
        }
        self.images.borrow_mut().clear();
        self.total.set(0);
        self.counter.set_label("");
    }
    pub fn refresh_devices(self: &Rc<Self>) {
        self.start(Job::Devices);
    }
    fn load(self: &Rc<Self>, page: usize) {
        if let Some(device) = self.device() {
            self.start(Job::List(device, self.path.text().into(), page));
        }
    }
    pub fn peer_changed(self: &Rc<Self>, status: &crate::daemon::Status) {
        let peer = if status.connected && status.paired {
            status.phone_address.clone()
        } else {
            None
        };
        if *self.last_peer.borrow() == peer {
            return;
        }
        *self.last_peer.borrow_mut() = peer.clone();
        self.cache.lock().unwrap().clear();
        if peer.is_some() && self.widget.is_mapped() {
            if self.busy.get() {
                self.pending_refresh.set(true);
            } else {
                self.refresh_devices();
            }
        }
    }
    pub fn shutdown(&self) {
        self.pending_refresh.set(false);
        if let Some(cancel) = self.cancel.borrow().as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    fn show_entries(self: &Rc<Self>, entries: Vec<Entry>, page: usize) {
        self.clear();
        let images = photos(&entries);
        let range = page_range(images.len(), page);
        self.current.set(range.start / PAGE);
        self.total.set(images.len());
        self.counter.set_label(&format!(
            "{}: {} · {}/{}",
            tr("Photos"),
            images.len(),
            self.current.get() + 1,
            images.len().div_ceil(PAGE).max(1)
        ));
        for folder in entries.iter().filter(|e| e.directory) {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&gtk::Image::from_icon_name("folder-symbolic"));
            row.append(
                &gtk::Label::builder()
                    .label(folder.name())
                    .xalign(0.)
                    .build(),
            );
            let button = gtk::Button::builder()
                .child(&row)
                .halign(gtk::Align::Fill)
                .build();
            let path = folder.path.clone();
            let w = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(p) = w.upgrade() {
                    p.path.set_text(&path);
                    p.load(0);
                }
            });
            self.folders.append(&button);
        }
        for image in &images[range] {
            let picture = gtk::Picture::builder()
                .width_request(144)
                .height_request(120)
                .can_shrink(true)
                .content_fit(gtk::ContentFit::Contain)
                .build();
            let placeholder = gtk::Image::builder()
                .icon_name("image-x-generic-symbolic")
                .pixel_size(40)
                .build();
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&picture));
            overlay.add_overlay(&placeholder);
            let label = gtk::Label::builder()
                .label(image.name())
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .max_width_chars(18)
                .build();
            let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
            content.append(&overlay);
            content.append(&label);
            let button = gtk::Button::builder()
                .child(&content)
                .tooltip_text(image.name())
                .build();
            let path = image.path.clone();
            let w = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(p) = w.upgrade() {
                    if let Some(device) = p.device() {
                        p.start(Job::Preview(device, path.clone()));
                    }
                }
            });
            self.grid.insert(&button, -1);
            self.images.borrow_mut().push((picture, placeholder));
        }
    }
    fn start(self: &Rc<Self>, job: Job) {
        self.shutdown();
        self.generation.set(self.generation.get() + 1);
        let generation = self.generation.get();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.cancel.borrow_mut() = Some(cancel.clone());
        self.busy.set(true);
        self.unavailable.set(0);
        self.controls();
        self.status.set_label(tr("Loading photos…"));
        if matches!(job, Job::List(..) | Job::Devices) {
            self.clear();
        }
        let (tx, rx) = async_channel::bounded(8);
        let cache = self.cache.clone();
        let epoch = cache.lock().unwrap().epoch();
        std::thread::spawn(move || {
            let result = (|| -> adb::Result<()> {
                match job {
                    Job::Devices => {
                        let (ids, preferred) = crate::adb_files::discover_devices(5555, &cancel)?;
                        let _ = tx.send_blocking(Event::Devices(ids, preferred));
                    }
                    Job::List(device, path, page) => {
                        let entries = adb::list(&device, &path, &cancel)?;
                        let images = photos(&entries);
                        let range = page_range(images.len(), page);
                        let _ = tx.send_blocking(Event::Listing(entries, page));
                        let mut missing = Vec::new();
                        for (index, image) in images[range].iter().enumerate() {
                            let bytes = cache.lock().unwrap().get(
                                &device,
                                &image.path,
                                std::time::Instant::now(),
                            );
                            if let Some(bytes) = bytes {
                                if tx
                                    .send_blocking(Event::Thumbnail(index, Ok(bytes)))
                                    .is_err()
                                {
                                    return Ok(());
                                }
                            } else {
                                missing.push((index, image.path.clone()));
                            }
                        }
                        if !missing.is_empty() {
                            let thumbnails = Thumbnails::new(&device, &cancel)?;
                            let paths: Vec<_> =
                                missing.iter().map(|(_, path)| path.clone()).collect();
                            let frames = thumbnails.batch(&paths, &cancel)?;
                            for ((index, path), data) in missing.into_iter().zip(frames) {
                                if adb::cancelled(&cancel) {
                                    return Err("Cancelled".into());
                                }
                                if let Ok(bytes) = &data {
                                    cache.lock().unwrap().insert(
                                        epoch,
                                        &device,
                                        &path,
                                        bytes.clone(),
                                        std::time::Instant::now(),
                                    );
                                }
                                if tx.send_blocking(Event::Thumbnail(index, data)).is_err() {
                                    return Ok(());
                                }
                            }
                        }
                    }
                    Job::Preview(device, path) => {
                        let thumbnails = Thumbnails::new(&device, &cancel)?;
                        let data = thumbnails.image(&path, 2048, &cancel)?;
                        let _ = tx.send_blocking(Event::Preview(path, data));
                    }
                    Job::Save(device, path, folder) => {
                        adb::download(&device, &path, &folder, &cancel)?
                    }
                }
                if adb::cancelled(&cancel) {
                    Err("Cancelled".into())
                } else {
                    Ok(())
                }
            })();
            let _ = tx.send_blocking(Event::Done(result));
        });
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(event) = rx.recv().await {
                let Some(p) = w.upgrade() else {
                    break;
                };
                if p.generation.get() != generation {
                    break;
                }
                match event {
                    Event::Devices(ids, preferred) => {
                        if p.pending_refresh.replace(false) && p.widget.is_mapped() {
                            p.refresh_devices();
                            continue;
                        }
                        let previous = p.device();
                        p.syncing.set(true);
                        p.model.splice(
                            0,
                            p.model.n_items(),
                            &ids.iter().map(String::as_str).collect::<Vec<_>>(),
                        );
                        let selected = selected_device(
                            &ids,
                            previous.as_deref(),
                            p.automatic_device.borrow().as_deref(),
                            preferred.as_deref(),
                        );
                        *p.automatic_device.borrow_mut() = selected
                            .and_then(|i| preferred.as_ref().filter(|id| **id == ids[i]).cloned());
                        p.devices.set_selected(
                            selected
                                .map(|i| i as u32)
                                .unwrap_or(gtk::INVALID_LIST_POSITION),
                        );
                        p.syncing.set(false);
                        if p.device().is_some() {
                            p.load(0);
                        } else {
                            p.status
                                .set_label(tr("Select an authorized ADB device to browse photos."));
                        }
                    }
                    Event::Listing(entries, page) => p.show_entries(entries, page),
                    Event::Thumbnail(index, data) => {
                        if let Some((picture, placeholder)) = p.images.borrow().get(index) {
                            match data.and_then(|bytes| {
                                gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
                                    .map_err(|_| "Image preview unavailable".into())
                            }) {
                                Ok(texture) => {
                                    picture.set_paintable(Some(&texture));
                                    placeholder.set_visible(false);
                                }
                                Err(_) => {
                                    p.unavailable.set(p.unavailable.get() + 1);
                                    placeholder.set_icon_name(Some("dialog-warning-symbolic"));
                                    picture.set_tooltip_text(Some(tr("Image preview unavailable")));
                                }
                            }
                        }
                    }
                    Event::Preview(path, data) => {
                        if p.widget.is_mapped() {
                            p.show_preview(&path, data);
                        }
                    }
                    Event::Done(result) => {
                        p.busy.set(false);
                        p.cancel.borrow_mut().take();
                        p.controls();
                        match result {
                            Ok(()) => p.status.set_label(tr(if p.device().is_some() {
                                if p.unavailable.get() > 0 {
                                    "Some images could not be previewed."
                                } else {
                                    "Completed"
                                }
                            } else {
                                "Select an authorized ADB device to browse photos."
                            })),
                            Err(error) => p.status.set_label(&if error == "Cancelled" {
                                tr("Cancelled").to_owned()
                            } else {
                                format!("{}: {}", tr("Failed"), tr(&error))
                            }),
                        }
                        if p.pending_refresh.replace(false) && p.widget.is_mapped() {
                            p.refresh_devices();
                        }
                        break;
                    }
                }
            }
        });
    }
    fn show_preview(self: &Rc<Self>, path: &str, bytes: Vec<u8>) {
        let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)) else {
            self.status.set_label(tr("Image preview unavailable"));
            return;
        };
        let dialog = adw::Dialog::builder()
            .title(path.rsplit('/').next().unwrap_or(tr("Gallery")))
            .content_width(900)
            .content_height(700)
            .build();
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        let close = gtk::Button::with_label(tr("Close"));
        let weak = dialog.downgrade();
        close.connect_clicked(move |_| {
            if let Some(d) = weak.upgrade() {
                d.close();
            }
        });
        header.pack_end(&close);
        let save = gtk::Button::with_label(tr("Download original…"));
        header.pack_start(&save);
        let device = self.device();
        let remote = path.to_string();
        let w = Rc::downgrade(self);
        save.connect_clicked(move |button| {
            let Some(device) = device.clone() else {
                return;
            };
            let remote = remote.clone();
            let w = w.clone();
            let parent = button.root().and_downcast::<gtk::Window>();
            gtk::FileDialog::builder()
                .title(tr("Download original…"))
                .build()
                .select_folder(parent.as_ref(), None::<&gio::Cancellable>, move |result| {
                    if let (Some(p), Ok(folder)) = (w.upgrade(), result) {
                        if let Some(folder) = folder.path() {
                            p.start(Job::Save(device, remote, folder));
                        }
                    }
                });
        });
        let picture = gtk::Picture::for_paintable(&texture);
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_vexpand(true);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header);
        content.append(&picture);
        dialog.set_child(Some(&content));
        self.dialogs.borrow_mut().retain(|d| d.is_visible());
        self.dialogs.borrow_mut().push(dialog.clone());
        dialog.present(Some(&self.widget));
    }
}
impl Drop for GalleryPage {
    fn drop(&mut self) {
        self.shutdown();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_replaces_automatic_endpoint_but_preserves_manual_device_choices() {
        let ids = vec![
            "old-wireless".into(),
            "new-wireless".into(),
            "usb-fixture".into(),
        ];
        assert_eq!(
            selected_device(
                &ids,
                Some("old-wireless"),
                Some("old-wireless"),
                Some("new-wireless")
            ),
            Some(1)
        );
        assert_eq!(
            selected_device(&ids, Some("usb-fixture"), None, Some("new-wireless")),
            Some(2)
        );
        assert_eq!(selected_device(&ids, None, None, None), None);
    }
    #[test]
    #[ignore = "requires a desktop session and isolated XDG directories"]
    fn photo_grid_pages_without_adb_and_cancels_owned_work() {
        adw::init().unwrap();
        let page = GalleryPage::build();
        assert!(!page.busy.get());
        assert!(page.cancel.borrow().is_none());
        page.syncing.set(true);
        page.model.append("fixture-device");
        page.devices.set_selected(0);
        page.syncing.set(false);
        let entries: Vec<_> = (0..30)
            .map(|i| Entry {
                path: format!("/sdcard/Pictures/photo-{i:02}.jpg"),
                directory: false,
            })
            .collect();
        page.show_entries(entries.clone(), 0);
        page.controls();
        assert_eq!(page.images.borrow().len(), 24);
        assert!(page.next.is_sensitive());
        assert!(!page.previous.is_sensitive());
        page.show_entries(entries, 1);
        page.controls();
        assert_eq!(page.images.borrow().len(), 6);
        assert!(!page.next.is_sensitive());
        assert!(page.previous.is_sensitive());
        let cancel = Arc::new(AtomicBool::new(false));
        *page.cancel.borrow_mut() = Some(cancel.clone());
        page.shutdown();
        assert!(adb::cancelled(&cancel));
    }
    #[test]
    fn photo_paging_filters_folders_and_videos_and_clamps_last_page() {
        let entries = vec![
            Entry {
                path: "/sdcard/b.JPG".into(),
                directory: false,
            },
            Entry {
                path: "/sdcard/c.mp4".into(),
                directory: false,
            },
            Entry {
                path: "/sdcard/a.png".into(),
                directory: false,
            },
            Entry {
                path: "/sdcard/folder.jpg".into(),
                directory: true,
            },
        ];
        let images = photos(&entries);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].name(), "b.JPG");
        assert_eq!(page_range(0, 9), 0..0);
        assert_eq!(page_range(25, 0), 0..24);
        assert_eq!(page_range(25, 1), 24..25);
        assert_eq!(page_range(25, 9), 24..25);
    }
}
