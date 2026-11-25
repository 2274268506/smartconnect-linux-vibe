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
/// Refresh only the phone's unread metadata; never sends SMS or changes read state.
fn unread_status_request() -> Value {
    json!({"cmd":"send","service":204,"data":{"ver":"5.0","cmd":1016,"pId":110}})
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
    if sync { let _ = read(unread_status_request()); }
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

/// Only incoming read state belongs to the local phone; SMS has no recipient read receipt.
fn message_status(message: &Message) -> &'static str {
    if message.message_type == 2 { "Sent" }
    else if message.read { "Read" } else { "Unread" }
}
fn read_labels(messages: &[Message], labels: &[(i64, i64, gtk::Label)]) {
    for (id, kind, label) in labels {
        if let Some(message) = messages.iter().find(|m| m.id == *id && m.kind == *kind) {
            label.set_label(tr(message_status(message)));
            if message.message_type == 1 && !message.read {label.add_css_class("sc-unread-badge");}
            else {label.remove_css_class("sc-unread-badge");}
        }
    }
}
/// Sending cmd 8 only queues a request: wait briefly for phone updates in the local cache.
fn confirm_read_with(
    key: &str,
    unread: &[(i64, i64)],
    mut command: impl FnMut(Value) -> Result<Value, String>,
    wait: Duration,
) -> Result<(ThreadReply, bool), String> {
    for (id, kind) in unread {
        command(json!({"cmd":"sms_mark_read","id":id,"kind":kind}))?;
    }
    for attempt in 0..9 {
        let reply: ThreadReply = serde_json::from_value(command(json!({"cmd":"sms_thread","thread":key,"refresh":false}))?)
            .map_err(|_| tr("Phone returned an invalid Messages response.").to_owned())?;
        let confirmed = unread.iter().all(|(id, kind)| reply.messages.iter()
            .any(|m| m.id == *id && m.kind == *kind && m.read));
        if confirmed || attempt == 8 {
            let _ = command(unread_status_request());
            return Ok((reply, confirmed));
        }
        std::thread::sleep(wait);
    }
    unreachable!()
}

/// A running older daemon can keep its connection while the GUI is upgraded.
fn queue_all_read_with(mut command: impl FnMut(Value) -> Result<Value, String>) -> Result<(), String> {
    match command(json!({"cmd":"sms_mark_all_read"})) {
        Ok(_) => return Ok(()),
        Err(error) if error.contains("unknown command") && error.contains("sms_mark_all_read") => {},
        Err(error) => return Err(error),
    }
    let threads: ThreadsReply = serde_json::from_value(command(json!({"cmd":"sms_threads","refresh":false}))?)
        .map_err(|_| tr("Phone returned an invalid Messages response.").to_owned())?;
    for thread in threads.threads.into_iter().filter(|t| t.unread > 0) {
        let reply: ThreadReply = serde_json::from_value(command(json!({"cmd":"sms_thread","thread":thread.thread,"refresh":false}))?)
            .map_err(|_| tr("Phone returned an invalid Messages response.").to_owned())?;
        for message in reply.messages.into_iter().filter(|m| m.message_type == 1 && !m.read) {
            command(json!({"cmd":"sms_mark_read","id":message.id,"kind":message.kind}))?;
        }
    }
    Ok(())
}

pub struct MessagesPage {
    pub refresh: gtk::Button,
    mark_all: gtk::Button,
    unread_count: Cell<usize>,
    marking: Cell<bool>,
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
        let mark_all = gtk::Button::builder().label(tr("Mark all as read")).sensitive(false).build();
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
        header.append(&mark_all);
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
        let page = Rc::new(Self {
                refresh,
                mark_all,
                unread_count: Cell::new(0),
                marking: Cell::new(false),
                status,
                list,
                connected: Cell::new(false),
                generation: Cell::new(0),
                busy: Cell::new(false),
                dialogs: RefCell::default(),
            });
        let weak = Rc::downgrade(&page);
        page.mark_all.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {page.mark_all_read();}
        });
        (page, scroll.upcast())
    }
    pub fn set_connected(&self, connected: bool) {
        if self.connected.replace(connected) == connected {
            return;
        }
        self.generation.set(self.generation.get() + 1);
        if connected {
            // One metadata refresh per connection edge; no background polling or content sync.
            glib::spawn_future_local(async {
                let _ = gio::spawn_blocking(|| request(unread_status_request())).await;
            });
        }
        self.busy.set(false);
        self.refresh.set_sensitive(connected);
        self.marking.set(false);
        self.mark_all.set_sensitive(connected && self.unread_count.get() > 0);
        if !connected {
            self.unread_count.set(0);
            self.mark_all.set_sensitive(false);
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
        if !self.connected.get() || self.marking.get() || self.busy.replace(true) {
            return;
        }
        self.refresh.set_sensitive(false);
        self.mark_all.set_sensitive(false);
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
            page.mark_all.set_sensitive(page.connected.get() && page.unread_count.get() > 0 && !page.marking.get());
        });
    }
    fn mark_all_read(self: &Rc<Self>) {
        if !self.connected.get() || self.busy.get() || self.marking.replace(true) {return;}
        self.mark_all.set_sensitive(false);
        self.refresh.set_sensitive(false);
        self.status.set_label(tr("Marking messages as read…"));
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(|| {
                queue_all_read_with(request)?;
                let mut data: ThreadsReply = serde_json::from_value(load(json!({"cmd":"sms_threads"}), false)?)
                    .map_err(|_| tr("Phone returned an invalid Messages response.").to_owned())?;
                for _ in 0..8 {
                    if data.threads.iter().all(|t| t.unread == 0) {break;}
                    std::thread::sleep(Duration::from_millis(250));
                    data = serde_json::from_value(load(json!({"cmd":"sms_threads"}), false)?)
                        .map_err(|_| tr("Phone returned an invalid Messages response.").to_owned())?;
                }
                let _ = request(unread_status_request());
                Ok::<_, String>(data)
            }).await.unwrap_or_else(|_| Err(tr("Messages request worker failed.").into()));
            let Some(page) = weak.upgrade() else {return;};
            if page.generation.get() != generation {return;}
            page.marking.set(false);
            page.refresh.set_sensitive(page.connected.get());
            match result {
                Ok(data) => {
                    let confirmed = data.threads.iter().all(|t| t.unread == 0);
                    page.show_threads(data);
                    if !confirmed {page.status.set_label(tr("Read status not confirmed. The phone must allow Smart Connect to modify SMS; see troubleshooting."));}
                }
                Err(error) => page.status.set_label(&error),
            }
            page.mark_all.set_sensitive(page.connected.get() && page.unread_count.get() > 0);
        });
    }
    fn show_threads(self: &Rc<Self>, data: ThreadsReply) {
        self.unread_count.set(data.threads.iter().map(|t| t.unread).sum());
        self.mark_all.set_sensitive(self.connected.get() && self.unread_count.get() > 0 && !self.marking.get());
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
            let state = gtk::Label::builder()
                .label(tr(if thread.unread > 0 { "Unread" } else { "Read" }))
                .css_classes(if thread.unread > 0 { vec!["sc-unread-badge"] } else { vec!["dim-label"] })
                .build();
            if thread.unread > 0 {row.add_css_class("sc-unread-thread");}
            row.add_suffix(&state);
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
        let header = adw::HeaderBar::builder().show_start_title_buttons(false).show_end_title_buttons(false).build();
        let close = gtk::Button::builder().label(tr("Close")).build();
        header.pack_end(&close);
        let weak_dialog = dialog.downgrade();
        close.connect_clicked(move |_| {if let Some(dialog)=weak_dialog.upgrade(){dialog.close();}});
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        content.append(&header);
        content.append(&scroll);
        content.append(&status);
        content.append(&compose);
        dialog.set_child(Some(&content));
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        let target = key.clone();
        let thread_status = status.clone();
        let read_target = key.clone();
        let loading_dialog = dialog.downgrade();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                load(json!({"cmd":"sms_thread","thread":target}), true)
            })
            .await
            .unwrap_or_else(|_| Err(tr("Messages request worker failed.").into()));
            let Some(page) = weak.upgrade() else {
                return;
            };
            if page.generation.get() != generation || !loading_dialog.upgrade().is_some_and(|d| d.is_visible()) {
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
                    let mut labels = Vec::new();
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
                        let bubble = gtk::Box::builder()
                            .orientation(gtk::Orientation::Vertical).spacing(4).halign(side)
                            .css_classes(["card", "sc-chip"]).build();
                        bubble.append(
                            &gtk::Label::builder()
                                .label(body)
                                .wrap(true)
                                .max_width_chars(52)
                                .xalign(0.)
                                .build(),
                        );
                        let state = gtk::Label::builder().label(tr(message_status(&message)))
                            .xalign(0.).css_classes(["caption", "dim-label"]).build();
                        if message.message_type == 2 {
                            state.set_tooltip_text(Some(tr("SMS does not provide recipient read receipts.")));
                        }
                        if message.message_type == 1 && !message.read {state.add_css_class("sc-unread-badge");}
                        bubble.append(&state);
                        labels.push((message.id, message.kind, state));
                        messages.append(&bubble);
                        if !message.read && message.message_type == 1 {
                            unread.push((message.id, message.kind));
                        }
                    }
                    if !unread.is_empty() {
                        let weak = Rc::downgrade(&page);
                        glib::spawn_future_local(async move {
                            let result = gio::spawn_blocking(move || {
                                confirm_read_with(&read_target, &unread, request, Duration::from_millis(250))
                            }).await.unwrap_or_else(|_| Err(tr("Messages request worker failed.").into()));
                            if let Some(page) = weak.upgrade() {
                                if page.generation.get() == generation && loading_dialog.upgrade().is_some_and(|d| d.is_visible()) {
                                    match result {
                                        Ok((data, confirmed)) => {
                                            read_labels(&data.messages, &labels);
                                            if !confirmed {
                                                thread_status.set_label(tr("Read status not confirmed. The phone must allow Smart Connect to modify SMS; see troubleshooting."));
                                            }
                                        }
                                        Err(error) => thread_status.set_label(&error),
                                    }
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
    fn cache_only_load_never_queries_phone_status_or_changes_message_state() {
        let mut calls=Vec::new();
        load_with(json!({"cmd":"sms_threads"}),false,|v|{calls.push(v);Ok(json!({"cacheOnlySupported":true}))},Duration::ZERO).unwrap();
        assert_eq!(calls,vec![json!({"cmd":"sms_threads","refresh":false})]);
        assert_eq!(unread_status_request(),json!({"cmd":"send","service":204,"data":{"ver":"5.0","cmd":1016,"pId":110}}));
    }
    #[test]
    fn parses_daemon_thread_reply_and_preserves_phone_text() {
        let reply:ThreadsReply=serde_json::from_str(r#"{"enabled":true,"threads":[{"thread":"example","address":"<number>","name":null,"count":2,"unread":1,"last_body":"<b>example</b>"}]}"#).unwrap();
        assert!(reply.enabled);
        assert_eq!(reply.threads[0].unread, 1);
        assert_eq!(reply.threads[0].last_body, "<b>example</b>");
    }
    fn sample_message(kind: i64, read: bool, outgoing: bool) -> Value {
        json!({"id":7,"kind":kind,"read":read,"body":"example","message_type":if outgoing {2} else {1}})
    }
    #[test]
    fn bulk_read_falls_back_for_old_daemons_and_preserves_message_kinds() {
        let mut sent=Vec::new();
        queue_all_read_with(|v| {
            match v["cmd"].as_str().unwrap() {
                "sms_mark_all_read" => return Err("unknown command sms_mark_all_read".into()),
                "sms_threads" => {assert_eq!(v["refresh"],false);return Ok(json!({"enabled":true,"threads":[{"thread":"example","address":"<number>","name":null,"count":4,"unread":2,"last_body":"example"}]}));},
                "sms_thread" => {assert_eq!(v["refresh"],false);return Ok(json!({"messages":[sample_message(1,false,false),sample_message(2,false,false),sample_message(1,true,false),sample_message(1,false,true)]}));},
                _ => sent.push(v),
            }
            Ok(json!({"ok":true}))
        }).unwrap();
        assert_eq!(sent,vec![json!({"cmd":"sms_mark_read","id":7,"kind":1}),json!({"cmd":"sms_mark_read","id":7,"kind":2})]);
        let mut calls=0;
        assert!(queue_all_read_with(|_|{calls+=1;Err("phone not connected".into())}).is_err());
        assert_eq!(calls,1,"transport errors must not trigger fallback actions");
    }
    #[test]
    fn incoming_read_states_are_distinct_from_outgoing_delivery() {
        for (read, outgoing, expected) in [(false,false,"Unread"),(true,false,"Read"),(false,true,"Sent"),(true,true,"Sent")] {
            let m = serde_json::from_value(sample_message(1,read,outgoing)).unwrap();
            assert_eq!(message_status(&m),expected);
        }
    }
    #[test]
    fn read_confirmation_waits_for_matching_kind_and_only_reads_cache() {
        let mut requests = Vec::new();
        let (_, confirmed) = confirm_read_with("example", &[(7,2)], |v| {
            requests.push(v.clone());
            if v["cmd"] == "sms_mark_read" { return Ok(json!({"ok":true})); }
            // A read SMS with the same ID must not confirm the unread MMS.
            Ok(json!({"messages":[sample_message(1,true,false), sample_message(2,requests.len()>=3,false)]}))
        },Duration::ZERO).unwrap();
        assert!(confirmed);
        assert_eq!(requests[0],json!({"cmd":"sms_mark_read","id":7,"kind":2}));
        assert_eq!(requests.len(),4);
        assert!(requests[1..3].iter().all(|v| v["cmd"]=="sms_thread" && v["refresh"]==false));
        assert_eq!(requests[3],unread_status_request());
    }
    #[test]
    fn queued_mark_does_not_invent_read_confirmation_and_errors_propagate() {
        let mut count = 0;
        let (reply, confirmed) = confirm_read_with("example", &[(7,1)], |v| {
            count += 1;
            Ok(if v["cmd"]=="sms_mark_read" {json!({"ok":true})} else {json!({"messages":[sample_message(1,false,false)]})})
        },Duration::ZERO).unwrap();
        assert!(!confirmed); assert!(!reply.messages[0].read); assert_eq!(count,11);
        assert!(confirm_read_with("example", &[(7,1)], |_| Err("offline".into()),Duration::ZERO).is_err());
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
        assert_eq!(requests.len(), 10);
        assert_eq!(requests[0]["refresh"], true);
        assert_eq!(requests[1],unread_status_request());
        assert!(requests[2..].iter().all(|r| r["refresh"] == false));
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
            2,
            "old daemons receive one content sync and one metadata refresh, never repeated content syncs"
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
        assert!(row.has_css_class("sc-unread-thread"));
        assert!(page.mark_all.is_sensitive());
        fn contains_label(widget: &gtk::Widget, text: &str) -> bool {
            if widget.downcast_ref::<gtk::Label>().is_some_and(|l| l.text() == text) {return true;}
            let mut child = widget.first_child();
            while let Some(current) = child {
                if contains_label(&current, text) {return true;}
                child = current.next_sibling();
            }
            false
        }
        assert!(contains_label(row.upcast_ref(),tr("Unread")));
        let label = gtk::Label::new(Some(tr("Unread")));
        let received = serde_json::from_value(sample_message(2,true,false)).unwrap();
        read_labels(&[received], &[(7,2,label.clone())]);
        assert_eq!(label.text(),tr("Read"));
        assert!(!label.has_css_class("sc-unread-badge"));
        page.show_threads(ThreadsReply {enabled:true, threads:vec![Thread {
            thread:"example".into(), address:"<number>".into(), name:None,
            count:1, unread:0, last_body:"example".into(),
        }]});
        assert!(contains_label(&page.list.first_child().unwrap(),tr("Read")));
        assert!(!page.mark_all.is_sensitive());
        assert!(!page.list.first_child().unwrap().has_css_class("sc-unread-thread"));
        window.present();
        page.open_thread("example".into(),"<number>".into());
        let dialog=page.dialogs.borrow().last().unwrap().clone();
        let header=dialog.child().unwrap().first_child().unwrap().downcast::<adw::HeaderBar>().unwrap();
        assert!(!header.shows_start_title_buttons() && !header.shows_end_title_buttons(), "automatic close controls must not duplicate Close");
        fn close_button(widget:&gtk::Widget)->Option<gtk::Button> {
            if let Some(button)=widget.downcast_ref::<gtk::Button>() {
                if button.label().as_deref()==Some(tr("Close")){return Some(button.clone());}
            }
            let mut child=widget.first_child();
            while let Some(current)=child {
                if let Some(button)=close_button(&current){return Some(button);}
                child=current.next_sibling();
            }
            None
        }
        let closed=Rc::new(Cell::new(false));let signal=closed.clone();
        dialog.connect_closed(move |_|signal.set(true));
        close_button(&dialog.child().unwrap()).expect("explicit Close button missing").emit_clicked();
        let deadline=std::time::Instant::now()+Duration::from_secs(2);
        while !closed.get() && std::time::Instant::now()<deadline {
            glib::MainContext::default().iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(closed.get(),"reply dialog Close button did not close the dialog");
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
