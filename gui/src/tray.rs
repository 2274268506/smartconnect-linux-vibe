//! System-tray item (freedesktop/KDE StatusNotifierItem, via the `ksni` crate).
//!
//! The tray runs on its own thread (ksni's blocking API spawns one). Its menu items and a left
//! click can't touch GTK directly from that thread, so they only drop a [`Command`] into an
//! `async_channel`; the GTK main loop drains it (see `window::Ui::listen_tray`). The icon and the
//! Connect/Disconnect menu entries are refreshed from the GTK side through [`Tray::set`].

use crate::i18n::tr;

use ksni::blocking::{Handle, TrayMethods};
use std::cell::Cell;
use std::sync::{mpsc, Arc, Mutex};

/// Keep only the newest pending update. Neither the producer nor its mutex waits for D-Bus.
struct LatestUpdate<T> {
    pending: Arc<Mutex<Option<T>>>,
    wake: mpsc::SyncSender<()>,
}

impl<T: Send + 'static> LatestUpdate<T> {
    fn new(mut apply: impl FnMut(T) + Send + 'static) -> Self {
        let pending = Arc::new(Mutex::new(None));
        let (wake, receiver) = mpsc::sync_channel(1);
        let next = pending.clone();
        std::thread::spawn(move || {
            while receiver.recv().is_ok() {
                // Release the mutex before invoking a potentially blocking service update.
                let update = next.lock().unwrap().take();
                if let Some(update) = update {
                    apply(update);
                }
            }
        });
        Self { pending, wake }
    }

    fn set(&self, value: T) {
        *self.pending.lock().unwrap() = Some(value);
        let _ = self.wake.try_send(());
    }
}

/// What a tray menu entry (or a left click) asks the window to do, handled on the GTK main loop.
#[derive(Clone, Copy, Debug)]
pub enum Command {
    /// Show the window if hidden, hide it if shown (also the left-click action).
    ToggleWindow,
    Connect,
    Disconnect,
    /// Quit the app *and* stop the daemon (the only thing that does).
    Quit,
}

/// The coarse state the icon reflects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// `smartconnectd` is not answering its control socket.
    DaemonDown,
    /// The daemon was just launched and the socket hasn't answered yet.
    Starting,
    Disconnected,
    Connected,
}

/// The actual StatusNotifierItem. Lives on the ksni thread; the GTK side only reaches it through
/// the [`Handle`] in [`Tray`].
struct Item {
    state: State,
    /// Drives the Show/Hide label.
    window_visible: bool,
    tx: async_channel::Sender<Command>,
}

impl Item {
    fn send(&self, cmd: Command) {
        // Unbounded channel: `try_send` never blocks (only fails if the UI is gone), which is what
        // we want from the tray thread.
        let _ = self.tx.try_send(cmd);
    }
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        "dev.smartconnect.Linux".into()
    }

    fn title(&self) -> String {
        "Smart Connect".into()
    }

    fn icon_name(&self) -> String {
        match self.state {
            State::DaemonDown => "network-offline-symbolic",
            State::Starting => "content-loading-symbolic",
            State::Disconnected | State::Connected => "phone-symbolic",
        }
        .into()
    }

    /// A small check badge on the icon when the phone is connected (hosts that don't draw overlays
    /// just ignore it; the tooltip still says the state).
    fn overlay_icon_name(&self) -> String {
        match self.state {
            State::Connected => "emblem-ok-symbolic".into(),
            _ => String::new(),
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let description = match self.state {
            State::DaemonDown => tr("Daemon not running"),
            State::Starting => tr("Starting…"),
            State::Disconnected => tr("Phone disconnected"),
            State::Connected => tr("Phone connected"),
        };
        ksni::ToolTip {
            icon_name: self.icon_name(),
            icon_pixmap: Vec::new(),
            title: "Smart Connect".into(),
            description: description.into(),
        }
    }

    /// Left click: toggle the window.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Command::ToggleWindow);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        vec![
            StandardItem {
                label: if self.window_visible {
                    tr("Hide Window").into()
                } else {
                    tr("Show Window").into()
                },
                activate: Box::new(|this: &mut Self| this.send(Command::ToggleWindow)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: tr("Connect").into(),
                enabled: matches!(self.state, State::Disconnected),
                activate: Box::new(|this: &mut Self| this.send(Command::Connect)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tr("Disconnect").into(),
                enabled: matches!(self.state, State::Connected),
                activate: Box::new(|this: &mut Self| this.send(Command::Disconnect)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: tr("Quit").into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|this: &mut Self| this.send(Command::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// A spawned tray item. Dropping it tears the tray down.
pub struct Tray {
    handle: Handle<Item>,
    updates: LatestUpdate<(State, bool)>,
    last: Cell<Option<(State, bool)>>,
}

impl Tray {
    /// Spawns the tray on its own thread. Returns the handle and the menu-command receiver, or
    /// `None` when the desktop has no StatusNotifierHost (then the app just runs without a tray).
    pub fn spawn() -> Option<(Tray, async_channel::Receiver<Command>)> {
        let (tx, rx) = async_channel::unbounded();
        let item = Item {
            state: State::Starting,
            window_visible: true,
            tx,
        };
        match item.spawn() {
            Ok(handle) => {
                let service = handle.clone();
                let updates = LatestUpdate::new(move |(state, window_visible)| {
                    service.update(|item| {
                        item.state = state;
                        item.window_visible = window_visible;
                    });
                });
                Some((
                    Tray {
                        handle,
                        updates,
                        last: Cell::new(None),
                    },
                    rx,
                ))
            }
            Err(e) => {
                eprintln!("smartconnect-gui: system tray unavailable: {e}");
                None
            }
        }
    }

    /// Pushes the current connection state and window visibility to the tray (icon, tooltip and the
    /// enabled state of Connect/Disconnect, Show/Hide label).
    pub fn set(&self, state: State, window_visible: bool) {
        let update = (state, window_visible);
        if self.last.replace(Some(update)) != Some(update) {
            self.updates.set(update);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // Enqueue shutdown, but never wait for D-Bus or join the update worker on the GTK thread.
        let _ = self.handle.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn busy_tray_does_not_block_updates_and_latest_visibility_wins() {
        let (entered, ready) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (applied, updates) = mpsc::channel();
        let queue = LatestUpdate::new(move |value| {
            if value == (State::Connected, true) {
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            applied.send(value).unwrap();
        });
        queue.set((State::Connected, true));
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        // The service is deliberately stuck; posts must finish before it is allowed to resume.
        queue.set((State::Disconnected, true));
        queue.set((State::Connected, false));
        release.send(()).unwrap();
        assert_eq!(
            updates.recv_timeout(Duration::from_secs(2)).unwrap(),
            (State::Connected, true)
        );
        assert_eq!(
            updates.recv_timeout(Duration::from_secs(2)).unwrap(),
            (State::Connected, false)
        );
        assert!(updates.try_recv().is_err());
    }
}
