//! Messages UI adapted from upstream PR #7. Data/drafts live in memory only.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{daemon, i18n::tr};

#[derive(Clone, Debug, Deserialize)]
struct Thread {
    thread: String,
    address: String,
    name: Option<String>,
    count: usize,
    unread: usize,
    last_body: String,
}
#[derive(Clone, Debug, Deserialize)]
struct Message {
    id: i64,
    #[serde(default = "sms_kind")]
    kind: i64,
    message_type: i64,
    body: String,
    read: bool,
}
fn sms_kind() -> i64 {
    1
}
#[derive(Clone, Debug, Deserialize)]
struct ThreadsReply {
    enabled: bool,
    threads: Vec<Thread>,
}
#[derive(Clone, Debug, Deserialize)]
struct ThreadReply {
    messages: Vec<Message>,
}

fn request(value: Value) -> Result<Value, String> {
    let reply = daemon::command(&value.to_string())?;
    if reply.ok {
        Ok(reply.raw)
    } else {
        Err(tr(&reply.message).to_owned())
    }
}
/// One phone sync on explicit open/refresh, followed only by bounded local cache reads.
fn load(command: Value, sync: bool) -> Result<Value, String> {
    load_with(command, sync, request, Duration::from_millis(250))
}
fn load_with(
    mut command: Value,
    sync: bool,
    mut read: impl FnMut(Value) -> Result<Value, String>,
    wait: Duration,
) -> Result<Value, String> {
    command["refresh"] = json!(sync);
    let mut reply = read(command.clone())?;
    if sync && reply["cacheOnlySupported"] == true {
        command["refresh"] = json!(false);
        for _ in 0..8 {
            std::thread::sleep(wait);
            reply = read(command.clone())?;
        }
    }
    Ok(reply)
}
fn retained_draft(current: &str, submitted: &str, accepted: bool) -> String {
    if accepted && current == submitted {
        String::new()
    } else {
        current.into()
    }
}

pub struct MessagesPage {
    pub refresh: gtk::Button,
    status: gtk::Label,
    list: gtk::ListBox,
    connected: Cell<bool>,
    generation: Cell<u64>,
    busy: Cell<bool>,
    dialogs: RefCell<Vec<adw::Dialog>>,
}
impl MessagesPage {
    pub fn build() -> (Rc<Self>, gtk::Widget) {
        let refresh = gtk::Button::builder()
            .label(tr("Refresh"))
            .sensitive(false)
            .build();
        let status = gtk::Label::builder()
            .label(tr("Connect phone, then refresh Messages."))
            .xalign(0.)
            .wrap(true)
            .css_classes(["dim-label"])
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let header = gtk::Box::builder().spacing(12).build();
        header.append(
            &gtk::Label::builder()
                .label(tr("Messages"))
                .xalign(0.)
                .hexpand(true)
                .css_classes(["title-2"])
                .build(),
        );
        header.append(&refresh);
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        column.append(&header);
        column.append(&status);
        column.append(&list);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&column)
            .build();
        (
            Rc::new(Self {
                refresh,
                status,
                list,
                connected: Cell::new(false),
                generation: Cell::new(0),
                busy: Cell::new(false),
                dialogs: RefCell::default(),
            }),
            scroll.upcast(),
        )
    }
    pub fn set_connected(&self, connected: bool) {
        if self.connected.replace(connected) == connected {
            return;
        }
        self.generation.set(self.generation.get() + 1);
        self.busy.set(false);
        self.refresh.set_sensitive(connected);
        if !connected {
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            for dialog in self.dialogs.borrow_mut().drain(..) {
                dialog.force_close();
            }
            self.status
                .set_label(tr("Connect phone, then refresh Messages."));
        }
    }
    pub fn refresh(self: &Rc<Self>) {
        self.refresh_inner(true);
    }
    fn refresh_inner(self: &Rc<Self>, sync: bool) {
        if !self.connected.get() || self.busy.replace(true) {
            return;
        }
        self.refresh.set_sensitive(false);
        if sync {
            self.status.set_label(tr("Syncing messages from phone…"));
        }
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || load(json!({"cmd":"sms_threads"}), sync))
                .await
                .unwrap_or_else(|_| Err(tr("Messages request worker failed.").into()));
            let Some(page) = weak.upgrade() else {
                return;
            };
            if page.generation.get() != generation {
                return;
            }
            page.busy.set(false);
            page.refresh.set_sensitive(page.connected.get());
            match result.and_then(|v| {
                serde_json::from_value(v)
                    .map_err(|_| tr("Phone returned an invalid Messages response.").into())
            }) {
                Ok(data) => page.show_threads(data),
                Err(error) => page.status.set_label(&error),
            }
        });
    }
    fn show_threads(self: &Rc<Self>, data: ThreadsReply) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.status.set_label(tr(if !data.enabled {
            "Enable Messages permission in Smart Connect on phone, then press Refresh."
        } else if data.threads.is_empty() {
            "No recent SMS conversations on phone."
        } else {
            "Messages stay on phone. This view keeps no local history."
        }));
        for thread in data.threads {
            let title = thread
                .name
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| thread.address.clone());
            let detail = if thread.unread > 0 {
                crate::tr_format!(
                    "{} · {} unread · {} messages",
                    "{} · {} 条未读 · {} 条消息",
                    thread.last_body,
                    thread.unread,
                    thread.count
                )
            } else {
                crate::tr_format!(
                    "{} · {} messages",
                    "{} · {} 条消息",
                    thread.last_body,
                    thread.count
                )
            };
            let row = adw::ActionRow::builder()
                .title(&title)
                .subtitle(&detail)
                .use_markup(false)
                .activatable(true)
                .build();
            let weak = Rc::downgrade(self);
            row.connect_activated(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.open_thread(thread.thread.clone(), thread.address.clone());
                }
            });
            self.list.append(&row);
        }
    }
    fn open_thread(self: &Rc<Self>, key: String, address: String) {
        if !self.connected.get() {
            return;
        }
        let dialog = adw::Dialog::builder()
            .title(&address)
            .content_width(560)
            .content_height(620)
            .build();
        self.dialogs.borrow_mut().retain(|d| d.is_visible());
        self.dialogs.borrow_mut().push(dialog.clone());
        let messages = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&messages)
            .build();
        let status = gtk::Label::builder()
            .label(tr("Syncing messages from phone…"))
            .wrap(true)
            .xalign(0.)
            .build();
        let entry = gtk::Entry::builder()
            .placeholder_text(tr("Write SMS"))
            .hexpand(true)
            .build();
        let send = gtk::Button::builder()
            .label(tr("Send"))
            .css_classes(["suggested-action"])
            .build();
        let compose = gtk::Box::builder()
            .spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        compose.append(&entry);
        compose.append(&send);
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        content.append(&scroll);
        content.append(&status);
        content.append(&compose);
        dialog.set_child(Some(&content));
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        let target = key.clone();
        let thread_status = status.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                load(json!({"cmd":"sms_thread","thread":target}), true)
            })
            .await
            .unwrap_or_else(|_| Err(tr("Messages request worker failed.").into()));
            let Some(page) = weak.upgrade() else {
                return;
            };
            if page.generation.get() != generation {
                return;
            }
            match result.and_then(|v| {
                serde_json::from_value::<ThreadReply>(v)
                    .map_err(|_| tr("Phone returned an invalid Messages response.").into())
            }) {
                Ok(data) => {
                    thread_status.set_label(tr(
                        "Messages stay on phone. This view keeps no local history.",
                    ));
                    let mut unread = Vec::new();
                    for message in data.messages {
                        let side = if message.message_type == 2 {
                            gtk::Align::End
                        } else {
                            gtk::Align::Start
                        };
                        let body = if message.body.is_empty() && message.kind == 2 {
                            tr("Multimedia message (attachments are not displayed)")
                        } else {
                            &message.body
                        };
                        messages.append(
                            &gtk::Label::builder()
                                .label(body)
                                .wrap(true)
                                .max_width_chars(52)
                                .xalign(0.)
                                .halign(side)
                                .css_classes(["card", "sc-chip"])
                                .build(),
                        );
                        if !message.read && message.message_type == 1 {
                            unread.push((message.id, message.kind));
                        }
                    }
                    if !unread.is_empty() {
                        let weak = Rc::downgrade(&page);
                        glib::spawn_future_local(async move {
                            let _ = gio::spawn_blocking(move || {
                                for (id, kind) in unread {
                                    let _ =
                                        request(json!({"cmd":"sms_mark_read","id":id,"kind":kind}));
                                }
                            })
                            .await;
                            if let Some(page) = weak.upgrade() {
                                if page.generation.get() == generation {
                                    page.refresh_inner(false);
                                }
                            }
                        });
                    }
                }
                Err(error) => thread_status.set_label(&error),
            }
        });
        let sending = Rc::new(Cell::new(false));
        let weak = Rc::downgrade(self);
        let entry_action = entry.clone();
        let button = send.clone();
        let send_action = Rc::new(move || {
            let Some(page) = weak.upgrade() else {
                return;
            };
            if !page.connected.get() || page.generation.get() != generation || sending.get() {
                return;
            }
            let text = entry_action.text().to_string();
            if text.trim().is_empty() {
                return;
            }
            sending.set(true);
            button.set_sensitive(false);
            entry_action.set_sensitive(false);
            status.set_label(tr("Handing SMS to phone…"));
            let payload = json!({"cmd":"sms_send","thread":key,"number":address,"text":text});
            let submitted = text;
            let entry = entry_action.clone();
            let send = button.clone();
            let status = status.clone();
            let busy = sending.clone();
            let weak = Rc::downgrade(&page);
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || request(payload))
                    .await
                    .unwrap_or_else(|_| Err(tr("Messages request worker failed.").into()));
                busy.set(false);
                let Some(page) = weak.upgrade() else {
                    return;
                };
                if page.generation.get() != generation {
                    return;
                }
                send.set_sensitive(true);
                entry.set_sensitive(true);
                entry.set_text(&retained_draft(&entry.text(), &submitted, result.is_ok()));
                match result {
                    Ok(_) => {
                        status.set_label(tr("SMS handed to phone; delivery is not confirmed."))
                    }
                    Err(error) => status.set_label(&error),
                }
            });
        });
        let action = send_action.clone();
        send.connect_clicked(move |_| action());
        entry.connect_activate(move |_| send_action());
        dialog.present(Some(&self.list));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_daemon_thread_reply_and_preserves_phone_text() {
        let reply:ThreadsReply=serde_json::from_str(r#"{"enabled":true,"threads":[{"thread":"example","address":"<number>","name":null,"count":2,"unread":1,"last_body":"<b>example</b>"}]}"#).unwrap();
        assert!(reply.enabled);
        assert_eq!(reply.threads[0].unread, 1);
        assert_eq!(reply.threads[0].last_body, "<b>example</b>");
    }
    #[test]
    fn sync_sends_one_phone_request_then_only_reads_the_local_cache() {
        let mut requests = Vec::new();
        load_with(
            json!({"cmd":"sms_threads"}),
            true,
            |command| {
                requests.push(command);
                Ok(json!({"cacheOnlySupported":true}))
            },
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(requests.len(), 9);
        assert_eq!(requests[0]["refresh"], true);
        assert!(requests[1..].iter().all(|r| r["refresh"] == false));
        requests.clear();
        load_with(
            json!({"cmd":"sms_threads"}),
            true,
            |command| {
                requests.push(command);
                Ok(json!({}))
            },
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(
            requests.len(),
            1,
            "old daemons must not receive repeated sync requests"
        );
    }
    #[test]
    #[ignore = "requires desktop session and isolated XDG_RUNTIME_DIR"]
    fn disconnect_clears_message_previews_without_requesting_phone_data() {
        adw::init().unwrap();
        let (page, widget) = MessagesPage::build();
        let window = adw::Window::builder().content(&widget).build();
        page.set_connected(true);
        page.show_threads(ThreadsReply {
            enabled: true,
            threads: vec![Thread {
                thread: "example".into(),
                address: "<number>".into(),
                name: Some("<b>Example</b>".into()),
                count: 1,
                unread: 1,
                last_body: "example message".into(),
            }],
        });
        let row = page
            .list
            .first_child()
            .unwrap()
            .downcast::<adw::ActionRow>()
            .unwrap();
        assert_eq!(row.title(), "<b>Example</b>");
        assert!(!row.uses_markup());
        page.set_connected(false);
        assert!(page.list.first_child().is_none());
        assert!(!page.refresh.is_sensitive());
        window.destroy();
    }
    #[test]
    fn failed_send_preserves_draft_and_ack_does_not_erase_newer_text() {
        assert_eq!(retained_draft("hello", "hello", false), "hello");
        assert_eq!(retained_draft("new draft", "hello", true), "new draft");
        assert_eq!(retained_draft("hello", "hello", true), "");
    }
}
