//! smartconnect-mirror: shows the phone screen and phone apps in Linux windows.
//! Specs: evidence-2026-10-20/mirroring-spec.md and appstream-and-display-spec.md (section A).
//!
//! smartconnectd relays the signalling over its control socket (`watch` svc 1/15/16/17, `send`);
//! every stream (the whole-screen mirror, Ready For desktop, or one phone app) is its own WebRTC
//! session with its own GStreamer pipeline (`webrtcbin` answerer for the phone's H.264 track) and
//! window.
//!
//!   mirror:     PC svc1 cmd1 (view=1) → phone svc15 cmd2 {pkg:"mirror"} → PC svc15 cmd1
//!   view (phone tile): phone svc1 cmd0 {view} → PC svc1 cmd1 {view, reply:1} → phone svc15 cmd2 {pkg:"mirror"|"desktop"|…}
//!   app (PC):   PC svc15 cmd1 {pkg,cmp,userId,source:0}
//!   app (phone, e.g. notification "Open"): phone svc15 cmd2 {pkg,pkgName,icon,source:1} → PC cmd1 (echo, source:1)
//!   then per session: phone cmd3 (offer, sessionId) → PC cmd4 (answer); cmd20 ICE both ways;
//!   phone cmd8 state:1 {displayId} (svc 16 input target); PC cmd9 heartbeat /30 s; PC cmd7 close.
//!
//! Notification action buttons (daemon local event, see actions.rs) are answered without a window
//! and closed again once the phone has fired the intent.
//!
//! Modes:
//!   smartconnect-mirror                  whole-screen mirror (exits when it ends)
//!   smartconnect-mirror --desktop        Ready For desktop on a phone virtual display (exits when it ends)
//!   smartconnect-mirror --host           long-running stream host (phone-initiated streams + control socket)
//!   smartconnect-mirror --app PKG [CMP]  stream one app (via the host if one runs, else standalone)
//!   smartconnect-mirror --list-apps      print the phone's app list (svc 17)
//!   smartconnect-mirror --sessions       list the host's open sessions

macro_rules! println {
    ($($arg:tt)*) => {{ let message = format!($($arg)*); crate::diagnostics::record("INFO", &message); std::println!("{message}"); }};
}
macro_rules! eprintln {
    ($($arg:tt)*) => {{ let message = format!($($arg)*); crate::diagnostics::record("ERROR", &message); std::eprintln!("{message}"); }};
}
macro_rules! warnln {
    ($($arg:tt)*) => {{ let message = format!($($arg)*); crate::diagnostics::record("WARN", &message); std::eprintln!("{message}"); }};
}
mod computer_ime;
mod diagnostics;
// Shared with the GUI; each binary uses different halves of the storage API.
#[allow(dead_code)]
#[path = "../../shared/mirror_log.rs"]
mod mirror_log;
#[allow(dead_code)]
#[path = "../../shared/app_display.rs"]
mod app_display;
#[allow(dead_code)]
#[path = "../../shared/stream_settings.rs"]
mod stream_settings;
#[allow(dead_code)]
#[path = "../../shared/desktop_renderer.rs"]
mod desktop_renderer;
mod actions;
mod ice;
mod icons;
mod keys;
mod playback;
mod stream_timing;
mod quality;
mod scrcpy;
mod capture_server;
mod desktop_hover;
mod resize;
mod rotation;
mod receive_buffer;
mod views;
mod window_close;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use gst::glib;
use gst::prelude::*;
use serde_json::{Value, json};

use keys::*;

/// How often the host checks app windows for a new size (when resizing is enabled).
const RESIZE_POLL: Duration = Duration::from_millis(250);

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}
fn socket_path() -> PathBuf {
    runtime_dir().join("smartconnectd.sock")
}
fn host_socket_path() -> PathBuf {
    runtime_dir().join("smartconnect-mirror.sock")
}
fn apps_cache_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
    base.join("smartconnect/apps.json")
}

/// The phone calls `releaseAllSessions()` whenever a cmd 1 carries a different `instanceId` than the
/// previous one, so every process (mirror, host, one-shot --app) shares one id per login session.
fn instance_id() -> i64 {
    let path = runtime_dir().join("smartconnect-mirror.instance");
    if let Some(id) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()) {
        return id;
    }
    let id = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(1);
    let _ = std::fs::write(&path, id.to_string());
    id
}

/// The daemon's `status` (plain-line command) on a fresh connection; `pcAddress` picks our ICE address.
fn daemon_status() -> Option<Value> {
    let mut s = UnixStream::connect(socket_path()).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    s.write_all(b"status\n").ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

/// The daemon's command connection (send-only; replies are drained by a thread).
struct Daemon {
    cmd: Mutex<UnixStream>,
}

impl Daemon {
    fn connect() -> Result<Arc<Self>> {
        let cmd = UnixStream::connect(socket_path()).context("is smartconnectd running?")?;
        let drain = cmd.try_clone()?;
        std::thread::spawn(move || {
            for line in BufReader::new(drain).lines().map_while(Result::ok) {
                if line.contains("\"ok\":false") {
                    eprintln!("daemon: {line}");
                }
            }
        });
        Ok(Arc::new(Daemon { cmd: Mutex::new(cmd) }))
    }

    fn send(&self, service: i64, data: Value) {
        let line = json!({ "cmd": "send", "service": service, "data": data }).to_string() + "\n";
        if let Err(e) = self.cmd.lock().unwrap().write_all(line.as_bytes()) {
            eprintln!("daemon write failed: {e}");
        }
    }

    fn send15(&self, cmd: i64, data: Option<Value>) {
        let mut msg = json!({ "ver": "1", "cmd": cmd });
        if let Some(d) = data {
            msg["data"] = d;
        }
        self.send(15, msg);
    }

    fn input(&self, cmd: i64, data: Value) {
        self.send(16, json!({ "cmd": cmd, "data": data, "messageId": 0, "instanceId": 0 }));
    }
}

/// Streams phone frames (and daemon-local events) for `services` into `tx` as `Ev::Phone`.
fn watch(services: &[i64], tx: mpsc::Sender<Ev>) -> Result<()> {
    let watch = UnixStream::connect(socket_path()).context("is smartconnectd running?")?;
    (&watch).write_all(format!("{}\n", json!({ "cmd": "watch", "services": services })).as_bytes())?;
    std::thread::spawn(move || {
        for line in BufReader::new(watch).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if let Some(svc) = v.get("service").and_then(Value::as_i64) {
                    let _ = tx.send(Ev::Phone(svc, v["data"].clone()));
                }
            }
        }
        eprintln!("daemon closed the watch stream");
        let _ = tx.send(Ev::DaemonGone);
    });
    Ok(())
}

enum Ev {
    Phone(i64, Value),
    DaemonGone,
    /// A command on the host's control socket, with where to send the one-line reply.
    Ctl(Value, mpsc::Sender<String>),
}

// ---------------------------------------------------------------------------------------------
// One stream session = one webrtcbin pipeline = one window (or none, for a background action).

#[derive(Default)]
struct SessState {
    mid: String,
    answered: bool,
    pending_local: Vec<(u32, String)>,
    /// The phone's virtual display for this session (svc 16 input target).
    display_id: Option<i64>,
    cap_requested: HashSet<i64>,
    /// Modifier keys currently held on the phone, as (scancode, flags).
    held_mods: HashSet<(i64, i64)>,
    scroll_acc: f64,
    /// Since when ICE has been failed/disconnected/closed (set from webrtcbin's thread).
    ice_bad_since: Option<Instant>,
    closing: bool,
    /// Headless (background action) sessions: when to close.
    headless: Option<actions::HeadlessTimer>,
    resize: resize::ResizeDebouncer,
    stream_size: Option<(u32, u32)>,
    stream_density: Option<i64>,
    rotation: i64,
    window_turned: Option<bool>,
}

struct Session {
    sid: i64,
    pkg: String,
    user_id: Value,
    cmp: Value,
    shortcut_id: Value,
    /// A notification action running without a window (video goes to a fakesink).
    headless: bool,
    requested_display: Option<app_display::AppDisplay>,
    quality: stream_settings::Quality,
    layout: stream_settings::Layout,
    renderer: desktop_renderer::Renderer,
    fullscreen: bool,
    scrcpy_options: desktop_renderer::ScrcpyOptions,
    d: Arc<Daemon>,
    st: Mutex<SessState>,
    pipeline: gst::Pipeline,
    webrtc: gst::Element,
    title: Mutex<String>,
    tag: Mutex<Option<gst::Element>>,
    sink: Mutex<Option<gst::Element>>,
    flip: Mutex<Option<gst::Element>>,
    /// The video window's X11 id once known, and the title last written to it.
    xwin: Mutex<Option<(u32, String, bool)>>,
    desktop_hover: Mutex<desktop_hover::Hover>,
    desktop_hover_poll: Mutex<Instant>,
    desktop_hover_window: Mutex<Option<(u32, u32)>>,
    borderless: AtomicBool,
    window_closed: Arc<AtomicBool>,
    watched_window: Mutex<Option<gst_gl::GLWindow>>,
    progress: Arc<Mutex<playback::Progress>>,
    recovery_pad: Mutex<Option<gst::Pad>>,
    receive_buffers: Mutex<receive_buffer::ReceiveBuffers>,
    timing: Arc<Mutex<stream_timing::Timing>>,
    frame_limiter: Arc<Mutex<quality::FrameLimiter>>,
    external: Mutex<Option<scrcpy::Link>>,
    external_since: Instant,
    phone_address: Option<String>,
}

/// What a new session is: the phone's ids for it plus how we show it.
struct SessionInfo {
    sid: i64,
    pkg: String,
    user_id: Value,
    cmp: Value,
    shortcut_id: Value,
    title: String,
    requested_display: Option<app_display::AppDisplay>,
    quality: stream_settings::Quality,
    layout: stream_settings::Layout,
    renderer: desktop_renderer::Renderer,
    fullscreen: bool,
    scrcpy_options: desktop_renderer::ScrcpyOptions,
    headless: bool,
}

impl Session {
    fn new(info: SessionInfo, filter: ice::CandidateFilter, d: Arc<Daemon>) -> Result<Arc<Self>> {
        let SessionInfo { sid, pkg, user_id, cmp, shortcut_id, title, requested_display, quality, layout, renderer, fullscreen, scrcpy_options, headless } = info;
        let pipeline = gst::Pipeline::with_name(&format!("session-{sid}"));
        let latency = playback::latency_ms();
        let webrtc = gst::ElementFactory::make("webrtcbin")
            .property_from_str("bundle-policy", "max-bundle").property("latency", latency).build()?;
        ice::bind_agent(&webrtc, &filter)?;
        let timing = Arc::new(Mutex::new(stream_timing::Timing::default()));
        stream_timing::observe(&pipeline, timing.clone());
        pipeline.add(&webrtc)?;
        let s = Arc::new(Session {
            sid,
            pkg,
            user_id,
            cmp,
            shortcut_id,
            headless,
            requested_display,
            quality,
            layout,
            renderer,
            fullscreen,
            scrcpy_options,
            d,
            st: Mutex::default(),
            pipeline: pipeline.clone(),
            webrtc: webrtc.clone(),
            title: Mutex::new(title),
            tag: Mutex::new(None),
            sink: Mutex::new(None),
            flip: Mutex::new(None),
            xwin: Mutex::new(None),
            desktop_hover: Mutex::new(desktop_hover::Hover::default()),
            desktop_hover_poll: Mutex::new(Instant::now()),
            desktop_hover_window: Mutex::new(None),
            borderless: AtomicBool::new(false),
            window_closed: Arc::new(AtomicBool::new(false)),
            watched_window: Mutex::new(None),
            progress: Arc::default(),
            recovery_pad: Mutex::new(None),
            receive_buffers: Mutex::default(),
            timing,
            frame_limiter: Arc::default(),
            external: Mutex::new(None),external_since:Instant::now(),
            phone_address: daemon_status().and_then(|s|s["phoneAddress"].as_str().map(str::to_owned)),
        });
        {
            let w = Arc::downgrade(&s);
            webrtc.connect("on-ice-candidate", false, move |args| {
                let (mline, cand) = (args[1].get::<u32>().ok()?, args[2].get::<String>().ok()?);
                if !filter.allows(&cand) {
                    return None; // docker/VPN/other networks: the phone would try them all
                }
                let s = w.upgrade()?;
                let answered = s.st.lock().unwrap().answered;
                if answered {
                    s.send_candidate(mline, &cand);
                } else {
                    s.st.lock().unwrap().pending_local.push((mline, cand));
                }
                None
            });
        }
        {
            let w = Arc::downgrade(&s);
            webrtc.connect_notify(Some("ice-connection-state"), move |wb, _| {
                use gst_webrtc::WebRTCICEConnectionState as S;
                let state: S = wb.property("ice-connection-state");
                if let Some(s) = w.upgrade() {
                    println!("[{}] ICE: {state:?}", s.sid);
                    let bad = matches!(state, S::Failed | S::Disconnected | S::Closed);
                    let mut st = s.st.lock().unwrap();
                    st.ice_bad_since = if bad { st.ice_bad_since.or_else(|| Some(Instant::now())) } else { None };
                }
            });
        }
        {
            let w = Arc::downgrade(&s);
            webrtc.connect_pad_added(move |_, pad| {
                if pad.direction() != gst::PadDirection::Src {
                    return;
                }
                let Some(s) = w.upgrade() else { return };
                // Never panic here: this runs on a GStreamer streaming thread (a panic aborts the process).
                if let Err(e) = s.attach_decoder(pad) {
                    eprintln!("[{}] could not attach the decoder: {e:#}", s.sid);
                }
            });
        }
        pipeline.set_state(gst::State::Playing)?;
        Ok(s)
    }

    fn attach_decoder(self: &Arc<Self>, pad: &gst::Pad) -> Result<()> {
        let caps=pad.current_caps().unwrap_or_else(||pad.query_caps(None));
        let audio=caps.structure(0).is_some_and(|c|c.get::<&str>("media").ok()==Some("audio"));
        if audio {
            let opus=caps.structure(0).and_then(|c|c.get::<&str>("encoding-name").ok()).is_some_and(|e|e.eq_ignore_ascii_case("OPUS"));
            let play=opus && !self.headless && self.renderer!=desktop_renderer::Renderer::Scrcpy;
            let desc=if play {"queue ! rtpopusdepay ! opusdec ! audioconvert ! audioresample ! autoaudiosink sync=false"} else {"queue ! fakesink sync=false async=false"};
            let bin=gst::parse::bin_from_description(desc,true)?;
            self.pipeline.add(&bin)?;pad.link(&bin.static_pad("sink").context("no audio input pad")?)?;bin.sync_state_with_parent()?;
            println!("[{}] offered audio track: playback={play}",self.sid);
            return Ok(());
        }
        if !self.headless {
        let progress = self.progress.clone();
        pad.add_probe(gst::PadProbeType::BUFFER, move |_, _| {
            progress.lock().unwrap().packet(Instant::now());
            gst::PadProbeReturn::Ok
        });
        }
        if self.headless || self.renderer==desktop_renderer::Renderer::Scrcpy {
            if !self.headless {println!("[{}] desktop owner: draining compressed video without a local decoder or window",self.sid);}
            // No window: keep the stream flowing (an unlinked pad would error the pipeline).
            let bin = gst::parse::bin_from_description("queue ! fakesink sync=false async=false", true)?;
            self.pipeline.add(&bin)?;
            pad.link(&bin.static_pad("sink").context("fakesink bin has no sink pad")?)?;
            bin.sync_state_with_parent()?;
            return Ok(());
        }
        *self.recovery_pad.lock().unwrap() = Some(pad.clone());
        println!("[{}] video RTP stream arriving", self.sid);
        let decoder = pick_decoder();
        let sink = pick_sink();
        println!(
            "[{}] playback: decoder={decoder}, sink={sink}, GL window={}, jitter buffer={} ms",
            self.sid,
            std::env::var("GST_GL_WINDOW").unwrap_or_else(|_| "auto".into()),
            self.webrtc.property::<u32>("latency")
        );
        let desc = playback::description(&decoder, &sink);
        let bin = gst::parse::bin_from_description(&desc, true)?;
        let tag = bin.by_name("tag").context("no taginject")?;
        tag.set_property("tags", title_tags(&self.title.lock().unwrap()));
        *self.tag.lock().unwrap() = Some(tag);
        *self.sink.lock().unwrap() = bin.by_name("sink");
        *self.flip.lock().unwrap()=bin.by_name("flip");
        self.apply_rotation(self.st.lock().unwrap().rotation);
        let progress = self.progress.clone();
        bin.by_name("render_queue").context("no render queue")?
            .static_pad("sink").context("no decoded input pad")?
            .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                progress.lock().unwrap().decoded(Instant::now());
                gst::PadProbeReturn::Ok
            });
        let limiter=self.frame_limiter.clone();let fps=self.quality.fps;
        bin.by_name("render_queue").context("no render queue")?.static_pad("src").context("no render output")?
            .add_probe(gst::PadProbeType::BUFFER,move |_,_| {
                if limiter.lock().unwrap().allow(Instant::now(),fps) {gst::PadProbeReturn::Ok} else {gst::PadProbeReturn::Drop}
            });
        self.pipeline.add(&bin)?;
        // Link before starting the bin, then bring it up to the pipeline's state.
        pad.link(&bin.static_pad("sink").context("decoder bin has no sink pad")?)?;
        bin.sync_state_with_parent()?;
        // The video sink sends pointer/keyboard navigation events upstream; turn them into input.
        let conv = bin.by_name("conv").context("no videoconvert")?;
        let sid = self.sid;
        conv.static_pad("src").context("no video output pad")?.add_probe(gst::PadProbeType::BUFFER, move |pad, _| {
            if let Some(caps) = pad.current_caps() { println!("[{sid}] decoded video: {caps}"); }
            gst::PadProbeReturn::Remove
        });
        let w = Arc::downgrade(self);
        bin.by_name("flip").context("no rotation filter")?.static_pad("src").context("no rotation output")?.add_probe(gst::PadProbeType::EVENT_UPSTREAM, move |_, info| {
            if let (Some(s), Some(gst::PadProbeData::Event(ev))) = (w.upgrade(), &info.data) {
                if ev.type_() == gst::EventType::Navigation {
                    s.on_navigation(ev);
                }
            }
            gst::PadProbeReturn::Ok
        });
        Ok(())
    }

    fn rotation(&self)->i64 {self.st.lock().unwrap().rotation}

    fn set_rotation(&self, rot: i64) {
        if self.renderer==desktop_renderer::Renderer::Scrcpy || self.pkg=="desktop" {return;}
        let changed = std::mem::replace(&mut self.st.lock().unwrap().rotation, rot) != rot;
        if changed {
            println!("[{}] display rotation {rot}: turning the picture {}", self.sid, rotation::direction(rot, rotation::mode_from_env()));
            self.apply_rotation(rot);
        }
    }

    fn apply_rotation(&self, rot: i64) {
        if let Some(flip) = self.flip.lock().unwrap().as_ref() {
            flip.set_property_from_str("video-direction", rotation::direction(rot, rotation::mode_from_env()));
        }
    }

    fn set_title(&self, title: &str) {
        let changed = {
            let mut t = self.title.lock().unwrap();
            std::mem::replace(&mut *t, title.to_owned()) != title
        };
        if changed {
            println!("[{}] window title updated", self.sid);
            // taginject re-sends its tags downstream on the next buffer; glimagesink retitles the window.
            if let Some(tag) = self.tag.lock().unwrap().as_ref() {
                tag.set_property("tags", title_tags(title));
            }
        }
    }

    fn send_candidate(&self, mline: u32, candidate: &str) {
        let mid = self.st.lock().unwrap().mid.clone();
        let data = json!({ "type": "candidate", "label": mline, "id": mid, "candidate": candidate }).to_string();
        self.d.send15(
            20,
            Some(json!({
                "sessionId": self.sid, "type": "candidate", "userId": self.user_id, "pkg": self.pkg, "data": data,
            })),
        );
    }

    fn add_remote_candidate(&self, body: &Value) -> Result<()> {
        let c: Value = serde_json::from_str(body.get("data").and_then(Value::as_str).unwrap_or("{}"))?;
        if let Some(cand) = c.get("candidate").and_then(Value::as_str) {
            let label = c.get("label").and_then(Value::as_u64).unwrap_or(0) as u32;
            self.webrtc.emit_by_name::<()>("add-ice-candidate", &[&label, &cand]);
        }
        Ok(())
    }

    /// Answers the offer (cmd 4). On the phone, this answer is what creates the app's virtual
    /// display and launches the app on it.
    fn answer(&self, offer_sdp: &str) -> Result<()> {
        let offer_sdp = h264_only(offer_sdp);
        if let Some(mid) = offer_sdp.lines().find_map(|l| l.strip_prefix("a=mid:")) {
            self.st.lock().unwrap().mid = mid.trim().to_owned();
        }
        let sdp = gst_sdp::SDPMessage::parse_buffer(offer_sdp.as_bytes()).map_err(|_| anyhow!("bad offer SDP"))?;
        let offer = gst_webrtc::WebRTCSessionDescription::new(gst_webrtc::WebRTCSDPType::Offer, sdp);
        let p = gst::Promise::new();
        self.webrtc.emit_by_name::<()>("set-remote-description", &[&offer, &p]);
        p.wait();
        let p = gst::Promise::new();
        self.webrtc.emit_by_name::<()>("create-answer", &[&None::<gst::Structure>, &p]);
        p.wait();
        let reply = p.get_reply().context("no answer")?;
        let answer = reply.value("answer")?.get::<gst_webrtc::WebRTCSessionDescription>()?;
        let text = quality::answer_sdp(&answer.sdp().as_text()?,self.quality);
        let answer = gst_webrtc::WebRTCSessionDescription::new(gst_webrtc::WebRTCSDPType::Answer,
            gst_sdp::SDPMessage::parse_buffer(text.as_bytes()).map_err(|_|anyhow!("bad quality SDP"))?);
        if self.quality.custom() {println!("[{}] stream limits: bitrateKbps={:?} displayFps={:?}",self.sid,self.quality.bitrate_kbps,self.quality.fps);}
        self.webrtc.emit_by_name::<()>("set-local-description", &[&answer, &None::<gst::Promise>]);
        println!("[{}] answer: {} bytes, H.264={}", self.sid, text.len(), text.contains("H264"));
        self.d.send15(
            4,
            Some(json!({
                "sessionId": self.sid, "error": 0, "isWebRTC": 1, "resize": 0, "userId": self.user_id,
                "webRTCData": json!({ "type": "answer", "sdp": text }).to_string(),
            })),
        );
        let pending = {
            let mut st = self.st.lock().unwrap();
            st.answered = true;
            if self.headless {
                st.headless = Some(actions::HeadlessTimer::new(Instant::now()));
            }
            std::mem::take(&mut st.pending_local)
        };
        for (m, c) in pending {
            self.send_candidate(m, &c);
        }
        Ok(())
    }

    /// Records the session's display id and asks the phone, once per display, for a virtual
    /// keyboard + mouse on it (svc 16 REQUEST_CAP, caps bit0 keyboard | bit1 mouse).
    fn learn_display(&self, body: &Value) {
        let Some(d) = body.get("displayId").and_then(Value::as_i64).filter(|d| *d >= 0) else { return };
        let first = {
            let mut st = self.st.lock().unwrap();
            st.display_id = Some(d);
            st.cap_requested.insert(d)
        };
        if first && !self.headless {
            println!("[{}] input: requesting keyboard+mouse on display {d}", self.sid);
            self.d.input(1, json!({ "ver": 1, "displayId": d, "caps": 3 }));
        }
    }

    /// cmd 3 `width`/`height`: the size the phone currently streams (resize baseline).
    fn learn_stream_size(&self, body: &Value) {
        if let Some(density) = body.get("density").and_then(Value::as_i64).filter(|v| *v > 0) {
            self.st.lock().unwrap().stream_density = Some(density);
        }
        let dim = |k: &str| body.get(k).and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
        if let (Some(w), Some(h)) = (dim("width"), dim("height")) {
            if w > 0 && h > 0 {
                let mut state = self.st.lock().unwrap();
                state.resize.set_stream_size(w, h);
                state.stream_size = Some((w, h));
            }
        }
    }

    fn display_id(&self) -> Option<i64> {
        self.st.lock().unwrap().display_id
    }

    fn stream_size(&self) -> Option<(u32, u32)> {
        self.st.lock().unwrap().stream_size
    }

    fn recover_video(&self, now: Instant) {
        if self.headless || self.renderer==desktop_renderer::Renderer::Scrcpy { return; }
        let Some(pad) = self.recovery_pad.lock().unwrap().clone() else { return };
        if !self.progress.lock().unwrap().request_due(now) { return; }
        let accepted = playback::request_keyframe(&pad);
        warnln!("[{}] video recovery: requesting keyframe (accepted={accepted})", self.sid);
    }

    fn stream_density(&self) -> Option<i64> { self.st.lock().unwrap().stream_density }

    fn render_stats(&self) -> Option<Value> {
        let sink = self.sink.lock().unwrap().clone()?;
        let renderer = if sink.find_property("stats").is_some() { sink }
            else { sink.downcast::<gst::Bin>().ok()?.iterate_recurse().into_iter()
                .filter_map(Result::ok).find(|element| element.find_property("stats").is_some())? };
        let stats = renderer.property::<gst::Structure>("stats");
        Some(json!({ "rendered":stats.get::<u64>("rendered").ok()?,
            "dropped":stats.get::<u64>("dropped").ok()? }))
    }

    /// The phone reported the app focused/merged/raised on this session's display: for a background
    /// action that is when it fires the intent.
    fn mark_fired(&self) {
        if let Some(t) = self.st.lock().unwrap().headless.as_mut() {
            println!("[{}] the phone is running the action now", self.sid);
            t.fired(Instant::now());
        }
    }

    fn headless_due(&self, now: Instant) -> Option<&'static str> {
        self.st.lock().unwrap().headless.and_then(|t| t.due(now))
    }

    /// The window's size if it changed and settled (see resize.rs), as a cmd 17 payload.
    fn poll_resize(&self, x: &X11, instance_id: i64, now: Instant) -> Option<Value> {
        if self.headless || self.pkg == "mirror" || self.requested_display.is_some() {
            return None; // the phone never resizes the mirror display
        }
        let display_id = self.display_id()?;
        let (win, _, _) = self.xwin.lock().unwrap().clone()?;
        let size = x.window_size(win)?;
        let wanted = self.st.lock().unwrap().resize.observe(size, now)?;
        println!("[{}] window is {}x{}: asking the phone to resize display {display_id}", self.sid, wanted.0, wanted.1);
        let t = resize::ResizeTarget {
            display_id,
            pkg: &self.pkg,
            user_id: &self.user_id,
            cmp: &self.cmp,
            shortcut_id: &self.shortcut_id,
            instance_id,
        };
        Some(resize::payload(&t, wanted))
    }

    /// Window pointer/keyboard events (GstNavigation from the video sink) → phone input (svc 16).
    /// Coordinates are already in video-frame pixels, which is what the phone expects.
    fn on_navigation(&self, ev: &gst::EventRef) {
        use gst_video::NavigationEvent as N;
        let Some(d) = self.display_id() else { return };
        let Ok(nav) = N::parse(ev) else { return };
        if matches!(&nav, N::MouseButtonPress { .. } | N::MouseScroll { .. } | N::KeyPress { .. }) {
            self.progress.lock().unwrap().input(Instant::now());
        }
        let pressed = matches!(nav, N::MouseButtonPress { .. } | N::KeyPress { .. });
        let input = |cmd, data| self.d.input(cmd, data);
        match nav {
            N::MouseMove { x, y, .. } => input(4, json!({ "displayId": d, "x": x as i64, "y": y as i64 })),
            N::MouseButtonPress { button: b, x, y, .. } | N::MouseButtonRelease { button: b, x, y, .. } => {
                // X11/GTK number buttons 1..9; GL-on-Wayland passes evdev codes (BTN_LEFT = 272…).
                let b = match b {
                    272 => 1,
                    273 => 3,
                    274 => 2,
                    275 => 8,
                    276 => 9,
                    b => b,
                };
                match b {
                    1..=3 => {
                        input(5, json!({ "displayId": d, "key": b, "x": x as i64, "y": y as i64, "down": pressed }))
                    }
                    // Mouse "back" side button → Android BACK.
                    8 if pressed => self.android_key(d, KEYCODE_BACK),
                    // X11 delivers wheel clicks as buttons 4 (up) / 5 (down).
                    4 | 5 if pressed => {
                        input(5, json!({ "displayId": d, "key": 4, "x": x as i64, "y": y as i64, "down": b == 5 }))
                    }
                    _ => {}
                }
            }
            N::MouseScroll { x, y, delta_y, .. } if delta_y != 0.0 => {
                // X11 sends ±1 per wheel click; Wayland smooth scroll sends ~10 units per notch.
                let unit = if delta_y.abs() <= 1.5 { 1.0 } else { 10.0 };
                let notches = {
                    let mut st = self.st.lock().unwrap();
                    st.scroll_acc += delta_y / unit;
                    let n = st.scroll_acc.trunc();
                    st.scroll_acc -= n;
                    n as i64
                };
                // Phone: BTN_WHEEL `down:true` → content scrolls down. GstNavigation: delta_y > 0 = wheel up.
                for _ in 0..notches.abs() {
                    input(5, json!({ "displayId": d, "key": 4, "x": x as i64, "y": y as i64, "down": notches < 0 }));
                }
            }
            N::KeyPress { key, .. } | N::KeyRelease { key, .. } => self.on_key(d, &key, pressed),
            _ => {}
        }
    }

    fn on_key(&self, d: i64, key: &str, down: bool) {
        let sc = scancode(key);
        let (ctrl, alt, shift) = {
            let mut st = self.st.lock().unwrap();
            if let Some(code) = sc.filter(|c| is_modifier(*c)) {
                if down {
                    st.held_mods.insert(code);
                } else {
                    st.held_mods.remove(&code);
                }
            }
            let h = |c: (i64, i64)| st.held_mods.contains(&c);
            (h((0x1D, 0)) || h((0x1D, E0)), h((0x38, 0)) || h((0x38, E0)), h((0x2A, 0)) || h((0x36, 0)))
        };
        // Phone navigation bindings (the key itself is swallowed, press and release).
        let phone_key = match key {
            "Escape" if !shift => Some(KEYCODE_BACK),
            "h" | "H" if ctrl && alt => Some(KEYCODE_HOME),
            "r" | "R" if ctrl && alt => Some(KEYCODE_APP_SWITCH),
            "b" | "B" if ctrl && alt => Some(KEYCODE_BACK),
            _ => None,
        };
        if let Some(k) = phone_key {
            if down {
                self.android_key(d, k);
            }
            return;
        }
        if let Some((code, flags)) = sc {
            self.d.input(3, json!({ "displayId": d, "flags": flags, "key": code, "down": down, "metaState": 0 }));
        } else if down {
            match key_text(key) {
                Some(text) => self.d.input(7, json!({ "displayId": d, "text": text })),
                None => println!("[{}] input: unmapped keyboard key", self.sid),
            }
        }
    }

    /// An Android keycode (BACK/HOME/APP_SWITCH…) as a press + release on the virtual keyboard.
    fn android_key(&self, d: i64, keycode: i64) {
        for down in [true, false] {
            self.d.input(
                3,
                json!({ "displayId": d, "flags": FLAG_NATIVE, "key": keycode, "down": down, "metaState": 0 }),
            );
        }
    }

    /// Tears the window down. `tell_phone`: we are the ones closing, so ask the phone to end the session.
    /// Returns false when the session was already closing.
    fn close(&self, why: &str, tell_phone: bool) -> bool {
        let (d, held) = {
            let mut st = self.st.lock().unwrap();
            if std::mem::replace(&mut st.closing, true) {
                return false;
            }
            (st.display_id, std::mem::take(&mut st.held_mods))
        };
        if let Some(link)=self.external.lock().unwrap().take(){link.stop();}
        println!("[{}] closing {} ({why})", self.sid, self.pkg);
        if tell_phone {
            if let Some(d) = d {
                for (code, flags) in held {
                    self.d.input(
                        3,
                        json!({ "displayId": d, "flags": flags, "key": code, "down": false, "metaState": 0 }),
                    );
                }
            }
            self.d.send15(7, Some(json!({ "sessionId": self.sid, "error": 0 })));
        }
        // GL/driver cleanup can block for seconds. Do not freeze every host control request,
        // heartbeat and other phone window while a single closed window releases its resources.
        let pipeline = self.pipeline.clone();
        std::thread::spawn(move || { let _ = pipeline.set_state(gst::State::Null); });
        true
    }

    fn external_state(&self)->Option<scrcpy::State> {
        if self.renderer!=desktop_renderer::Renderer::Scrcpy{return None;}
        Some(self.external.lock().unwrap().as_ref().map(|l|l.state()).unwrap_or_default())
    }
    fn poll_external(&self)->Option<(String,Option<String>)> {
        if self.renderer!=desktop_renderer::Renderer::Scrcpy{return None;}
        let mut link=self.external.lock().unwrap();
        if link.is_none() {
            if let Some(display_id)=self.display_id().filter(|_|self.st.lock().unwrap().answered) {
                *link=Some(scrcpy::Link::start(scrcpy::Spec{display_id,capture_isolation:capture_server::supports_package(&self.pkg),phone_address:self.phone_address.clone(),
                    options:self.scrcpy_options.clone(),quality:self.quality,borderless:self.borderless.load(Ordering::Relaxed),session_id:self.sid,
                    fullscreen:self.fullscreen,
                    window_size:if self.layout.fixed&&self.layout.fit!=stream_settings::FitAxis::Manual{self.stream_size()}else{None},
                    title:if self.pkg=="desktop"{"Smart Connect Desktop (scrcpy)".into()}else{format!("{} (scrcpy)",self.title.lock().unwrap())}}));
            } else if self.external_since.elapsed()>Duration::from_secs(25) {
                return Some(("scrcpy display initialization timed out".into(),Some("The phone did not create the app or desktop display".into())));
            }
        }
        let link=link.as_ref()?;
        let state=link.state();
        if state.phase=="running" && self.scrcpy_options.computer_ime {
            if let Some(display)=self.display_id() {
                for text in link.committed_text() {self.d.input(7,json!({"displayId":display,"text":text}));}
            }
        }
        match state.phase {
            "failed"=>Some(("scrcpy failed".into(),state.error)),
            "stopped"=>Some(("scrcpy window closed".into(),None)),_=>None,
        }
    }

    /// Window closed (the sink posts an error), stream ended, or ICE down for > 5 s.
    fn poll_dead(&self) -> Option<String> {
        if self.window_closed.load(Ordering::Acquire) { return Some("video window closed".into()); }
        if let Some(bus) = self.pipeline.bus() {
            while let Some(msg) = bus.pop() {
                match msg.view() {
                    gst::MessageView::Error(e) => return Some(format!("window closed or failed: {}", e.error())),
                    gst::MessageView::Eos(_) => return Some("end of stream".into()),
                    gst::MessageView::Latency(_) => { let _ = self.pipeline.recalculate_latency(); }
                    _ => {}
                }
            }
        }
        let ice_dead = self.st.lock().unwrap().ice_bad_since.is_some_and(|t| t.elapsed() > Duration::from_secs(5));
        ice_dead.then(|| "connection to the phone lost (ICE down > 5 s)".into())
    }
}

/// taginject's `tags` property is a GstTagList in structure-string form.
fn title_tags(title: &str) -> String {
    format!("title=\"{}\"", title.replace('\\', "").replace('"', "'"))
}

/// Window titles for glimagesink's X11 (XWayland) windows: the sink ignores title tags, so we
/// look up its GL window handle and set `_NET_WM_NAME`/`WM_NAME`/`WM_CLASS` ourselves.
struct X11 {
    conn: x11rb::rust_connection::RustConnection,
    net_wm_name: u32,
    utf8: u32,
    motif_hints: u32,
    root:u32,net_clients:u32,net_pid:u32,
}

impl X11 {
    fn connect() -> Option<Self> {
        use x11rb::protocol::xproto::ConnectionExt as _;
        use x11rb::connection::Connection as _;
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root=conn.setup().roots.get(screen)?.root;
        let atom = |n: &str| conn.intern_atom(false, n.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
        Some(X11 { net_wm_name: atom("_NET_WM_NAME")?, utf8: atom("UTF8_STRING")?,
            motif_hints: atom("_MOTIF_WM_HINTS")?, root,net_clients:atom("_NET_CLIENT_LIST")?,net_pid:atom("_NET_WM_PID")?,conn })
    }

    fn set_title(&self, win: u32, title: &str, first: bool) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{AtomEnum, PropMode};
        use x11rb::wrapper::ConnectionExt as _;
        let c = &self.conn;
        let _ = c.change_property8(PropMode::REPLACE, win, self.net_wm_name, self.utf8, title.as_bytes());
        let _ = c.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, self.utf8, title.as_bytes());
        if first {
            let class = b"smartconnect-mirror\0smartconnect-mirror\0";
            let _ = c.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, class);
        }
        let _ = c.flush();
    }

    fn find_window(&self, title: &str) -> Option<u32> {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
        let named = |win: u32| -> bool {
            [self.net_wm_name, AtomEnum::WM_NAME.into()].into_iter().any(|prop| {
                self.conn
                    .get_property(false, win, prop, AtomEnum::ANY, 0, 64)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .is_some_and(|r| r.value.starts_with(title.as_bytes()))
            })
        };
        let root = self.conn.setup().roots.first()?.root;
        let mut todo = vec![(root, 0u8)];
        // The window manager may wrap the sink's window in a frame with the same title: take the
        // innermost match, which is the one the sink draws in.
        let mut found: Option<(u8, u32)> = None;
        while let Some((win, depth)) = todo.pop() {
            if win != root && named(win) && found.is_none_or(|(d, _)| depth > d) {
                found = Some((depth, win));
            }
            if depth < 3 {
                if let Some(tree) = self.conn.query_tree(win).ok().and_then(|c| c.reply().ok()) {
                    todo.extend(tree.children.into_iter().map(|c| (c, depth + 1)));
                }
            }
        }
        found.map(|(_, win)| win)
    }

    fn resize_window(&self, win: u32, (w, h): (u32, u32)) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt as _};
        let _ = self.conn.configure_window(win, &ConfigureWindowAux::new().width(w).height(h));
        let _ = self.conn.flush();
    }

    /// The window's client-area size in pixels.
    fn window_size(&self, win: u32) -> Option<(u32, u32)> {
        use x11rb::protocol::xproto::ConnectionExt as _;
        let g = self.conn.get_geometry(win).ok()?.reply().ok()?;
        Some((u32::from(g.width), u32::from(g.height)))
    }

    fn window_for_pid(&self,pid:u32)->Option<u32> {
        use x11rb::protocol::xproto::{ConnectionExt as _,AtomEnum};
        let clients=self.conn.get_property(false,self.root,self.net_clients,AtomEnum::WINDOW,0,4096).ok()?.reply().ok()?;
        clients.value32()?.find(|win|self.conn.get_property(false,*win,self.net_pid,AtomEnum::CARDINAL,0,1).ok().and_then(|c|c.reply().ok()).and_then(|r|r.value32().and_then(|mut v|v.next()))==Some(pid))
    }

    /// Follow the actual pointer window chain, so covered windows never inject hover.
    fn pointer_in_window(&self, win: u32) -> Option<(i32, i32)> {
        use x11rb::protocol::xproto::ConnectionExt as _;
        let mut current = self.root;
        for _ in 0..16 {
            let pointer = self.conn.query_pointer(current).ok()?.reply().ok()?;
            if !pointer.same_screen { return None; }
            if current == win {
                // Do not supplement hover while dragging/clicking with scrcpy.
                if u16::from(pointer.mask) & 0x1f00 != 0 { return None; }
                return Some((i32::from(pointer.win_x), i32::from(pointer.win_y)));
            }
            if pointer.child == 0 { return None; }
            current = pointer.child;
        }
        None
    }

    fn app_layout(&self, win:u32, size:(u32,u32), layout:stream_settings::Layout) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{ConnectionExt as _,ConfigureWindowAux};
        use x11rb::properties::{WmSizeHints,AspectRatio,WmSizeHintsSpecification};
        let mut hints=WmSizeHints::get_normal_hints(&self.conn,win).ok().and_then(|c|c.reply().ok()).flatten().unwrap_or_default();
        let ratio=AspectRatio{numerator:size.0 as i32,denominator:size.1 as i32};
        hints.aspect=Some((ratio,ratio));
        if layout.fit!=stream_settings::FitAxis::Manual {
            // X11 geometry is in X-server pixels, not GTK logical coordinates. The GUI computed
            // encoder dimensions from physical monitor pixels; matching them avoids resampling.
            hints.size=Some((WmSizeHintsSpecification::UserSpecified,size.0 as i32,size.1 as i32));
            hints.min_size=Some((size.0 as i32,size.1 as i32));hints.max_size=hints.min_size;
            self.set_borderless(win,true);
            let _=self.conn.configure_window(win,&ConfigureWindowAux::new().width(size.0).height(size.1));
        }
        let _=hints.set_normal_hints(&self.conn,win);let _=self.conn.flush();
    }

    fn set_borderless(&self, win: u32, borderless: bool) {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::PropMode;
        use x11rb::wrapper::ConnectionExt as _;
        // MWM_HINTS_DECORATIONS = 1<<1; decorations 0 removes the frame, 1 restores all.
        let hints = [2, 0, if borderless { 0 } else { 1 }, 0, 0];
        let _ = self.conn.change_property32(PropMode::REPLACE, win, self.motif_hints, self.motif_hints, &hints);
        let _ = self.conn.flush();
    }
}

/// The X11 window id of a glimagesink's GL window (0 until it exists).
fn gl_window_handle(sink: &gst::Element) -> u32 {
    use gst_gl::prelude::*;
    if sink.find_property("context").is_none() {
        return 0;
    }
    let Some(ctx) = sink.property::<Option<gst_gl::GLContext>>("context") else { return 0 };
    let Some(win) = ctx.window() else { return 0 };
    use glib::translate::ToGlibPtr;
    // SAFETY: `win` is a valid GstGLWindow for the duration of the call.
    unsafe { gst_gl::ffi::gst_gl_window_get_window_handle(win.to_glib_none().0) as u32 }
}

impl Session {
    fn watch_window_close(&self) {
        let mut watched = self.watched_window.lock().unwrap();
        if watched.is_some() { return; }
        // The sink installs its own callbacks during GL setup. Wait for its first completed
        // frame before replacing the close callback, so setup cannot overwrite our observer.
        if self.render_stats().and_then(|stats| stats["rendered"].as_u64()).unwrap_or(0) == 0 { return; }
        let Some(sink) = self.sink.lock().unwrap().clone() else { return };
        if sink.find_property("context").is_none() { return; }
        let Some(context) = sink.property::<Option<gst_gl::GLContext>>("context") else { return };
        use gst_gl::prelude::*;
        let Some(window) = context.window() else { return };
        window_close::watch(&window, &self.window_closed);
        *watched = Some(window);
    }
    fn sync_window_shape(&self, x: &X11) {
        if self.renderer==desktop_renderer::Renderer::Scrcpy || self.pkg=="desktop" || self.layout.fixed {return;}
        let (rot, turned) = {
            let st = self.st.lock().unwrap();
            (st.rotation, st.window_turned)
        };
        let Some(size)=self.stream_size()else{return;};
        let want = (size.0>size.1)^rotation::is_turned(rot,rotation::mode_from_env());
        if Some(want) == turned || self.sink.lock().unwrap().is_none() {
            return;
        }
        let known = self.xwin.lock().unwrap().as_ref().map(|(w, _, _)| *w);
        let win = known.or_else(|| x.find_window(&self.title.lock().unwrap()));
        let Some((win, (w, h))) = win.and_then(|win| Some((win, x.window_size(win)?))) else { return };
        // Some sinks (xvimagesink) reshape their own window when the picture's shape changes;
        // only a window still the wrong way round gets swapped. A stream is portrait unless turned.
        if (w > h) != want {
            println!("[{}] window {w}x{h} → {h}x{w} (display rotation {rot})", self.sid);
            x.resize_window(win, (h, w));
        }
        self.st.lock().unwrap().window_turned = Some(want);
    }

    /// Phone edge monitors must see hover on the original desktop, not scrcpy's clone display.
    fn sync_desktop_hover(&self, x: &X11) {
        if self.headless || self.pkg != "desktop" || self.renderer != desktop_renderer::Renderer::Scrcpy { return; }
        let now = Instant::now();
        { let mut last = self.desktop_hover_poll.lock().unwrap();
          if now.duration_since(*last) < Duration::from_millis(50) { return; } *last = now; }
        let Some(display) = self.display_id() else { return; };
        let Some(size) = self.stream_size() else { return; };
        let point = self.external_state().filter(|s| s.phase == "running").and_then(|s| s.pid)
            .and_then(|pid| {
                let mut cached = self.desktop_hover_window.lock().unwrap();
                if let Some((owner, win)) = *cached { if owner == pid { return Some(win); } }
                let win = x.window_for_pid(pid)?; *cached = Some((pid, win)); Some(win)
            })
            .and_then(|win| desktop_hover::video_point(x.pointer_in_window(win)?, x.window_size(win)?, size));
        if let Some((px, py)) = self.desktop_hover.lock().unwrap().observe(point, size.1) {
            self.d.input(4, json!({"displayId":display,"x":px,"y":py}));
        }
    }

    /// Applies the current title to the X11 window (finds the window the first time).
    fn sync_x11_title(&self, x: &X11) {
        let title = self.title.lock().unwrap().clone();
        let borderless = self.borderless.load(Ordering::Relaxed);
        let mut xw = self.xwin.lock().unwrap();
        match &mut *xw {
            Some((win, last, last_borderless)) if *last != title || *last_borderless != borderless => {
                if *last != title { x.set_title(*win, &title, false); }
                if *last_borderless != borderless { x.set_borderless(*win, borderless); }
                *last = title;
                *last_borderless = borderless;
            }
            Some(_) => {}
            None => {
                let win=if self.renderer==desktop_renderer::Renderer::Scrcpy {
                    if !self.layout.fixed{return;}
                    let Some(pid)=self.external_state().and_then(|state|state.pid)else{return;};
                    let Some(win)=x.window_for_pid(pid)else{return;};win
                }else{
                    let Some(sink)=self.sink.lock().unwrap().clone()else{return;};gl_window_handle(&sink)
                };
                if win != 0 {
                    x.set_title(win, &title, self.renderer!=desktop_renderer::Renderer::Scrcpy);
                    if self.pkg == "desktop" { x.set_borderless(win, borderless); }
                    if self.layout.fixed {if let Some(size)=self.stream_size(){x.app_layout(win,size,self.layout);}}
                    *xw = Some((win, title, borderless));
                }
            }
        }
    }
}

/// Raises (focuses) a window by title on niri, Hyprland and sway; other desktops: no-op.
fn raise_window(title: String) {
    std::thread::spawn(move || {
        let has = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        let run = |argv: &[&str]| {
            let _ = std::process::Command::new(argv[0]).args(&argv[1..]).stdout(std::process::Stdio::null()).status();
        };
        if has("HYPRLAND_INSTANCE_SIGNATURE") {
            let Ok(out) = std::process::Command::new("hyprctl").args(["-j", "clients"]).output() else { return };
            let Ok(list) = serde_json::from_slice::<Value>(&out.stdout) else { return };
            let addr = list.as_array().into_iter().flatten().find(|w| w["title"].as_str() == Some(&title)).and_then(|w| w["address"].as_str());
            if let Some(addr) = addr {
                run(&["hyprctl", "dispatch", "focuswindow", &format!("address:{addr}")]);
            }
        } else if has("SWAYSOCK") {
            // sway criteria take a regex: match the title literally.
            let quoted: String = title.chars().flat_map(|c| if "\\^$.|?*+()[]{}\"".contains(c) { vec!['\\', c] } else { vec![c] }).collect();
            run(&["swaymsg", &format!("[title=\"^{quoted}$\"] focus")]);
        } else {
            let Ok(out) = std::process::Command::new("niri").args(["msg", "--json", "windows"]).output() else { return };
            let Ok(list) = serde_json::from_slice::<Value>(&out.stdout) else { return };
            let id = list.as_array().into_iter().flatten().find(|w| w["title"].as_str() == Some(&title)).and_then(|w| w["id"].as_u64());
            if let Some(id) = id {
                run(&["niri", "msg", "action", "focus-window", "--id", &id.to_string()]);
            }
        }
    });
}

// ---------------------------------------------------------------------------------------------
// The host: routes svc 15 frames to sessions, starts new ones one at a time.

#[derive(Clone, Debug)]
struct StartReq {
    pkg: String,
    cmp: Value,
    user_id: Value,
    shortcut_id: Value,
    source: i64,
    name: Option<String>,
    /// Answer without a window and close once the phone ran the notification action.
    headless: bool,
    display: Option<app_display::AppDisplay>,
    quality: stream_settings::Quality,
    layout: stream_settings::Layout,
    renderer: desktop_renderer::Renderer,
    fullscreen: bool,
    scrcpy_options: desktop_renderer::ScrcpyOptions,
}

enum PreparedStart {
    App(StartReq),
    Desktop,
}
struct PendingWake {
    action: PreparedStart,
    task: scrcpy::WakePreparation,
}

struct Host {
    d: Arc<Daemon>,
    instance_id: i64,
    /// View sessions (whole-screen mirror, Ready For desktop, virtual phone) belong to this process.
    own_mirror: bool,
    /// App sessions (any other pkg, incl. phone-initiated) belong to this process.
    own_apps: bool,
    /// Answer the phone's svc 1 cmd 0 (Screen share / Mobile desktop started on the phone).
    answer_views: bool,
    /// Run notification action buttons without a window (actions.rs).
    background: bool,
    actions: actions::PendingActions,
    /// Ask the phone to resize app displays to their window (resize.rs; opt-in).
    resize: bool,
    last_resize_poll: Instant,
    /// The desktop resolution from the phone's last view invite (used for the "desktop" stream).
    view_resolution: Option<(i64, i64)>,
    desktop_scale: u32,
    desktop_borderless: bool,
    desktop_quality: stream_settings::Quality,
    desktop_renderer: desktop_renderer::Renderer,
    desktop_fullscreen: bool,
    app_errors: HashMap<String,String>,
    desktop_scrcpy: desktop_renderer::ScrcpyOptions,
    desktop_error: Option<String>,
    desktop_pending: bool,
    mirror_pending: bool,
    cancelled_starts: HashMap<String, Instant>,
    wakes: HashMap<String,PendingWake>,
    wake_start: fn(desktop_renderer::ScrcpyOptions,Option<String>)->scrcpy::WakePreparation,
    /// One-shot process: quit once its sessions are gone.
    exit_when_idle: bool,
    sessions: HashMap<i64, Arc<Session>>,
    queue: VecDeque<StartReq>,
    inflight: Option<(StartReq, Instant)>,
    had_session: bool,
    last_hb: Instant,
    last_log_stats: Instant,
    /// pkg → app name (svc 17 list, cmd 2 pkgName, cmd 13 appName).
    names: HashMap<String, String>,
    apps: Value,
    x11: Option<X11>,
    main_loop: glib::MainLoop,
}

impl Host {
    fn borderless_supported(&self) -> bool {
        self.x11.is_some() && pick_sink().split_whitespace().next() == Some("glimagesink")
    }
    fn title_for(&self, pkg: &str) -> String {
        if let Some(t) = views::title(pkg) {
            return t.into();
        }
        format!("{} — Phone", self.names.get(pkg).cloned().unwrap_or_else(|| pkg.to_owned()))
    }

    fn owns_pkg(&self, pkg: &str) -> bool {
        if views::is_view_pkg(pkg) { self.own_mirror } else { self.own_apps }
    }

    fn by_display(&self, d: i64) -> Option<Arc<Session>> {
        self.sessions.values().find(|s| s.display_id() == Some(d)).cloned()
    }

    fn pending_packages(&self) -> Vec<String> {
        let mut packages: Vec<_> = self.queue.iter().map(|r|r.pkg.clone())
            .chain(self.inflight.iter().map(|(r,_)|r.pkg.clone())).collect();
        if self.desktop_pending { packages.push("desktop".into()); }
        if self.mirror_pending { packages.push("mirror".into()); }
        packages.extend(self.wakes.keys().cloned());
        packages.sort(); packages.dedup(); packages
    }

    fn cancel_start(&mut self, pkg: &str) {
        let active_view=views::is_view_pkg(pkg) && (self.pending_packages().iter().any(|p|p==pkg)
            || self.sessions.values().any(|s|s.pkg==pkg));
        self.cancelled_starts.insert(pkg.to_owned(), Instant::now());
        self.wakes.remove(pkg);
        self.queue.retain(|r|r.pkg!=pkg);
        if self.inflight.as_ref().is_some_and(|(r,_)|r.pkg==pkg) { self.inflight=None; }
        if pkg=="desktop" { self.desktop_pending=false; self.desktop_error=None; }
        if pkg=="mirror" { self.mirror_pending=false; }
        self.app_errors.remove(pkg);
        let sessions:Vec<_>=self.sessions.values().filter(|s|s.pkg==pkg&&!s.headless).map(|s|s.sid).collect();
        for sid in sessions { self.close_session(sid,"startup cancelled",true); }
        if active_view { self.d.send(1,json!({"ver":"1.0.0","cmd":5})); }
        self.pump();
    }

    fn prepare_start(&mut self,pkg:&str,options:desktop_renderer::ScrcpyOptions,action:PreparedStart) {
        if !options.wake_unlock {self.run_prepared_start(action);return;}
        println!("ADB preflight: waking/unlocking before starting {pkg}");
        let address=daemon_status().and_then(|s|s["phoneAddress"].as_str().map(str::to_owned));
        self.wakes.insert(pkg.into(),PendingWake{action,task:(self.wake_start)(options,address)});
    }
    fn run_prepared_start(&mut self,action:PreparedStart) {
        match action {
            PreparedStart::App(request)=>self.request(request,false),
            PreparedStart::Desktop=>{
                if let Some(size)=self.view_resolution {self.d.send(1,views::desktop_resolution(size));}
                println!("asking the phone to start Ready For desktop");
                self.d.send(1,views::start(2));
            }
        }
    }
    fn poll_wakes(&mut self) {
        let done:Vec<_>=self.wakes.iter().filter_map(|(pkg,p)|p.task.result().map(|r|(pkg.clone(),r))).collect();
        for (pkg,result) in done {
            let Some(pending)=self.wakes.remove(&pkg)else{continue;};
            match result {
                Ok(())=>{println!("ADB preflight: phone ready for {pkg}");self.run_prepared_start(pending.action);}
                Err(error)=>{
                    eprintln!("ADB preflight failed for {pkg}: {error}");
                    if pkg=="desktop" {self.desktop_pending=false;self.desktop_error=Some(error);}
                    else{self.app_errors.insert(pkg,error);}
                }
            }
        }
    }

    fn request(&mut self, r: StartReq, front: bool) {
        if let Some(n) = &r.name {
            self.names.insert(r.pkg.clone(), n.clone());
        }
        // The phone re-sends unanswered invites; one cmd 1 per app is enough.
        let starting =
            self.inflight.as_ref().is_some_and(|(q, _)| q.pkg == r.pkg) || self.queue.iter().any(|q| q.pkg == r.pkg);
        if starting {
            println!("already starting {}; ignoring the repeated request", r.pkg);
            return;
        }
        if front {
            self.queue.push_front(r)
        } else {
            self.queue.push_back(r)
        }
        self.pump();
    }

    /// Sends the next queued cmd 1. The phone keeps a single pending start per PC, so wait for the
    /// cmd 3 offer (or a cmd 8 state 0 / cmd 12 merge, or 20 s) before the next one.
    fn pump(&mut self) {
        if self.inflight.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_secs(20)) {
            return;
        }
        if let Some((r, _)) = self.inflight.take() {
            eprintln!("start of {} timed out", r.pkg);
        }
        let Some(mut r) = self.queue.pop_front() else { return };
        if r.pkg=="desktop" {r.quality=self.desktop_quality;r.renderer=self.desktop_renderer;r.fullscreen=self.desktop_fullscreen;r.scrcpy_options=self.desktop_scrcpy.clone();}
        println!("requesting stream: pkg {} cmp {} source {}", r.pkg, r.cmp, r.source);
        // The size is the bound the phone fits the stream into. For Ready For desktop it is the
        // desktop resolution, so use the one the user picked on the phone when we know it.
        let (w, h) = match (r.pkg.as_str(), self.view_resolution) {
            ("desktop", Some(res)) => res,
            _ => (1920, 1080),
        };
        let mm = |px: i64| (px as f64 * 344.0 / 1920.0).round() as i64;
        let mut data = json!({
            "pkg": r.pkg, "userId": r.user_id,
            "width": w, "height": h, "phyWidth": mm(w), "phyHeight": mm(h),
            "displayWidth": w, "displayHeight": h, "displayDensity": 160, "displayScale": 1.0,
            "align": 0, "instanceId": self.instance_id, "port": 0, "source": r.source,
            "supportedMediaFormat": 1, "supportWebRTC": 1,
            // Booleans in the phone's bean: sending 0 makes Gson throw and the start silently dies.
            "supportCustomDisplay": false, "vrSupportUnfolded": false,
        });
        if r.pkg != "mirror" {
            data["cmp"] = r.cmp.clone();
            data["shortcutId"] = r.shortcut_id.clone();
        }
        if r.pkg == "desktop" {
            for (key, value) in views::desktop_display((w,h), self.desktop_scale).as_object().unwrap() {
                data[key] = value.clone();
            }
        }
        if let Some(display)=r.display.filter(|_| !r.headless && !views::is_view_pkg(&r.pkg)) {
            for (key,value) in display.wire().as_object().unwrap() { data[key]=value.clone(); }
            println!("custom app display: {}x{} density={}",display.width,display.height,display.density);
        }
        self.d.send15(1, Some(data));
        self.inflight = Some((r, Instant::now()));
    }

    fn finish_inflight(&mut self, pkg: &str, ok: bool, why: &str) {
        if self.inflight.as_ref().is_some_and(|(r, _)| r.pkg == pkg) {
            if !ok {
                eprintln!("could not start {pkg}: {why}");
                let error=if why.starts_with("reason 147 ") {"Phone confirmation timed out; unlock the phone and try again"}else{"The phone refused the stream; see mirroring logs"}.to_owned();
                if pkg=="desktop" {self.desktop_pending=false;self.desktop_error=Some(error);}
                else if pkg=="mirror" {self.mirror_pending=false;}
                else {self.app_errors.insert(pkg.to_owned(),error);}
            }
            self.inflight = None;
            self.pump();
        }
    }

    fn close_session(&mut self, sid: i64, why: &str, tell_phone: bool) {
        if let Some(s) = self.sessions.remove(&sid) {
            if s.pkg=="desktop"{self.desktop_pending=false;}
            s.close(why, tell_phone);
        }
    }

    fn close_all(&mut self, why: &str, tell_phone: bool) {
        for sid in self.sessions.keys().copied().collect::<Vec<_>>() {
            self.close_session(sid, why, tell_phone);
        }
    }

    fn on_frame(&mut self, svc: i64, msg: &Value) -> Result<()> {
        match svc {
            1 => return self.on_svc1(msg),
            17 => return self.on_svc17(msg),
            actions::SERVICE_LOCAL_EVENTS => return self.on_local_event(msg),
            15 => {}
            _ => return Ok(()),
        }
        let cmd = msg.get("cmd").and_then(Value::as_i64).unwrap_or(-1);
        let body = msg.get("data").cloned().unwrap_or(Value::Null);
        let sid = body.get("sessionId").and_then(Value::as_i64);
        let pkg = body.get("pkg").and_then(Value::as_str).unwrap_or("").to_owned();
        if matches!(cmd,2|3|8) && self.cancelled_starts.contains_key(&pkg) {
            if let Some(sid)=sid { self.d.send15(7,Some(json!({"sessionId":sid,"error":0}))); }
            println!("ignored late cancelled start for {pkg}");
            return Ok(());
        }
        let mine = sid.and_then(|s| self.sessions.get(&s).cloned());
        let shown = |d: Option<i64>, me: &Self| d.and_then(|d| me.by_display(d));
        // The stream invite (cmd 2) and focus changes (cmd 13) carry the app's icon as base64 PNG:
        // keep it for the GUI (`<icon cache>/<pkg>.png`, written once).
        if matches!(cmd, 2 | 13) {
            if let Some(icon) = body.get("icon").and_then(Value::as_str).filter(|i| !i.is_empty()) {
                if let Some(path) = icons::harvest(&pkg, icon) {
                    println!("saved the icon of {pkg} to {}", path.display());
                }
            }
        }
        match cmd {
            // Stream invite: the phone set up a view (after svc 1) or wants to show an app here
            // (notification "Open" or action button, hand-off, the phone's App streaming picker).
            // Answer cmd 1 with source 1 and the echoed fields.
            2 => {
                if !self.owns_pkg(&pkg) {
                    return Ok(());
                }
                if views::is_view_pkg(&pkg) && self.sessions.values().any(|s| s.pkg == pkg) {
                    println!("ignoring a new {pkg} setup: already open");
                    return Ok(());
                }
                let headless = !views::is_view_pkg(&pkg) && self.background && self.actions.take(&pkg, Instant::now());
                println!(
                    "phone invites stream: pkg {pkg} ({}) source {}{}",
                    body["pkgName"],
                    body["source"],
                    if headless { " — notification action, running it without a window" } else { "" }
                );
                self.request(
                    StartReq {
                        pkg: pkg.clone(),
                        cmp: body.get("cmp").cloned().unwrap_or(Value::Null),
                        user_id: body.get("userId").cloned().unwrap_or(json!(0)),
                        shortcut_id: body.get("shortcutId").cloned().unwrap_or(Value::Null),
                        source: body.get("source").and_then(Value::as_i64).unwrap_or(1),
                        name: body.get("pkgName").and_then(Value::as_str).map(str::to_owned),
                        headless,
                        display: None,
        quality: Default::default(),
        layout: Default::default(),
        renderer: Default::default(),
        fullscreen:false,
        scrcpy_options: Default::default(),
                    },
                    true,
                );
            }
            3 => {
                let Some(sid) = sid else { return Ok(()) };
                let offer = body.get("webRTCData").and_then(Value::as_str);
                let s = match (mine, offer) {
                    (Some(s), _) => s,
                    (None, Some(_)) if self.owns_pkg(&pkg) => {
                        let req = self.inflight.as_ref().map(|(r, _)| r).filter(|r| r.pkg == pkg);
                        let headless = req.is_some_and(|r| r.headless);
                        let field = |k: &str, fallback: Option<&Value>| {
                            body.get(k).filter(|v| !v.is_null()).or(fallback).cloned().unwrap_or(Value::Null)
                        };
                        let info = SessionInfo {
                            sid,
                            pkg: pkg.clone(),
                            user_id: body.get("userId").cloned().unwrap_or(json!(0)),
                            cmp: field("cmp", req.map(|r| &r.cmp)),
                            shortcut_id: field("shortcutId", req.map(|r| &r.shortcut_id)),
                            requested_display: req.and_then(|r|r.display),
                            quality: req.map(|r|r.quality).unwrap_or_default(),
                            layout: req.map(|r|r.layout).unwrap_or_default(),
                            renderer: req.map(|r|r.renderer).unwrap_or_default(),
                            fullscreen:req.is_some_and(|r|r.fullscreen),
                            scrcpy_options: req.map(|r|r.scrcpy_options.clone()).unwrap_or_default(),
                            title: self.title_for(&pkg),
                            headless,
                        };
                        let filter = ice::CandidateFilter::current(
                            daemon_status().as_ref().and_then(ice::pc_address_from_status),
                        );
                        println!(
                            "[{sid}] new session: pkg {pkg} {}x{}{}; ICE: offering {filter}",
                            body["width"],
                            body["height"],
                            if headless { " (headless)" } else { "" }
                        );
                        let s = Session::new(info, filter, self.d.clone())?;
                        if pkg=="mirror" { self.mirror_pending=false; }
                        if pkg == "desktop" { self.desktop_pending=false;s.borderless.store(self.desktop_borderless, Ordering::Relaxed); }
                        else if s.layout.fit!=stream_settings::FitAxis::Manual {s.borderless.store(true,Ordering::Relaxed);}
                        self.sessions.insert(sid, s.clone());
                        self.had_session = true;
                        self.finish_inflight(&pkg, true, "");
                        s
                    }
                    _ => return Ok(()),
                };
                s.learn_display(&body);
                s.learn_stream_size(&body);
                if let Some(offer) = offer {
                    if !s.st.lock().unwrap().answered {
                        let offer: Value = serde_json::from_str(offer)?;
                        s.answer(offer["sdp"].as_str().context("offer without sdp")?)?;
                        self.d.send15(9, None);
                        self.last_hb = Instant::now();
                    }
                }
            }
            20 => {
                if let Some(s) = mine {
                    s.add_remote_candidate(&body)?;
                }
            }
            8 => {
                let state = body.get("state").and_then(Value::as_i64).unwrap_or(-1);
                let target = mine.or_else(|| {
                    if sid.is_some() {
                        return None;
                    }
                    body.get("displayId").and_then(Value::as_i64).and_then(|d| self.by_display(d))
                });
                match (state, target) {
                    (1, Some(s)) => {
                        println!(
                            "[{}] connected: display {} supportTouch {}",
                            s.sid, body["displayId"], body["supportTouch"]
                        );
                        s.learn_display(&body);
                    }
                    (0, Some(s)) => {
                        let why = format!("phone stopped it (reason {} {})", body["reason"], body["msg"]);
                        self.close_session(s.sid, &why, false);
                    }
                    (0, None) => {
                        let why = format!("reason {} {}", body["reason"], body["msg"]);
                        self.finish_inflight(&pkg, false, &why);
                    }
                    (st, Some(s)) => println!("[{}] state {st} reason {}", s.sid, body["reason"]),
                    _ => {}
                }
            }
            7 => {
                if let Some(s) = mine {
                    self.close_session(s.sid, "phone closed the session", false);
                }
            }
            // App merged into an existing display / move-to-top: raise that window. For a
            // background action this is the phone running the intent on that display.
            12 | 14 => {
                if cmd == 12 {
                    self.finish_inflight(&pkg, true, "");
                }
                if let Some(s) = shown(body.get("displayId").and_then(Value::as_i64), self) {
                    if s.headless {
                        s.mark_fired();
                    } else {
                        println!("[{}] raise (cmd {cmd})", s.sid);
                        raise_window(s.title.lock().unwrap().clone());
                    }
                }
            }
            // Focus changed inside a display: retitle that window. The phone fires a pending
            // notification intent at exactly this moment (background actions: close soon).
            13 => {
                let d = body.get("displayId").and_then(Value::as_i64);
                if let Some(s) = shown(d, self) {
                    if s.headless {
                        s.mark_fired();
                    } else if let Some(name) =
                        body.get("appName").and_then(Value::as_str).filter(|_| !views::is_view_pkg(&s.pkg))
                    {
                        if !pkg.is_empty() {
                            self.names.insert(pkg.clone(), name.to_owned());
                        }
                        s.set_title(&format!("{name} — Phone"));
                    }
                }
            }
            16 | 23 => {
                if let Some(s) = mine {
                    println!("[{}] cmd {cmd}: rotation {} display {}", s.sid, body["rotation"], body["displayId"]);
                    s.learn_display(&body);
                    if let Some(rotation)=body["rotation"].as_i64(){s.set_rotation(rotation);}
                }
            }
            26 => {
                let d = body.get("displayId").and_then(Value::as_i64);
                if let Some(s) = shown(d, self) {
                    println!("[{}] resize result: success {} error {}", s.sid, body["success"], body["error"]);
                }
            }
            10 | 21 => {} // heartbeat ack, audio app list
            _ => {
                if self.own_apps || mine.is_some() {
                    println!("svc15 cmd {cmd} received");
                }
            }
        }
        Ok(())
    }

    fn on_svc1(&mut self, msg: &Value) -> Result<()> {
        // cmd 0: the user started a view on the phone (Screen share / Mobile desktop tile, phone
        // cards). The phone waits 20 s for our cmd 1, then invites the stream on svc 15.
        if let Some(inv) = views::parse_invite(msg) {
            if self.cancelled_starts.contains_key(views::view_pkg(inv.view).unwrap_or("")) {
                self.d.send(1,json!({"ver":"1.0.0","cmd":5}));
                return Ok(());
            }
            if self.answer_views {
                let pkg = views::view_pkg(inv.view).unwrap_or("?");
                println!("phone asks to start {pkg} (view {}, resolution {:?}): accepting", inv.view, inv.resolution);
                if inv.view == 2 {
                    self.view_resolution = inv.resolution;
                }
                self.d.send(1, views::accept(inv.view));
            }
            return Ok(());
        }
        let cmd = msg.get("cmd").and_then(Value::as_i64);
        // 5 = stop/cancel, 11 = error: the phone ended (or refused) the view.
        if self.own_mirror && matches!(cmd, Some(5 | 11)) {
            self.mirror_pending=false;
            if self.desktop_pending {
                self.desktop_pending=false;
                if cmd==Some(11){self.desktop_error=Some("The phone refused desktop mode; unlock it and try again".into());}
            }
            println!("svc1 cmd {} received", cmd.unwrap_or(0));
            let ended: Vec<i64> =
                self.sessions.values().filter(|s| views::is_view_pkg(&s.pkg)).map(|s| s.sid).collect();
            for sid in ended {
                self.close_session(sid, "phone ended the view", false);
            }
            if self.exit_when_idle && self.sessions.is_empty() {
                self.main_loop.quit();
            }
        }
        Ok(())
    }

    fn on_svc17(&mut self, msg: &Value) -> Result<()> {
        if msg.get("cmd").and_then(Value::as_i64) != Some(1) {
            return Ok(());
        }
        let apps = app_list(msg);
        for a in apps.as_array().into_iter().flatten() {
            if let (Some(p), Some(n)) = (a["pkg"].as_str(), a["name"].as_str()) {
                self.names.insert(p.to_owned(), n.to_owned());
            }
        }
        println!("app list: {} apps", apps.as_array().map_or(0, Vec::len));
        save_apps(&apps);
        self.apps = apps;
        Ok(())
    }

    /// Daemon-local events (`watch` service -1): notification clicks the phone will carry out
    /// through app streaming.
    fn on_local_event(&mut self, ev: &Value) -> Result<()> {
        if !(self.background && self.own_apps) {
            return Ok(());
        }
        if let Some(click) = actions::parse_click(ev) {
            let what = match click.kind {
                actions::ClickKind::Action => "action button: the next invite runs without a window",
                actions::ClickKind::Open => "open: shown in a window",
            };
            println!("notification click for {}: {what}", click.pkg);
            self.actions.note(&click, Instant::now());
        }
        Ok(())
    }

    fn on_ctl(&mut self, v: &Value) -> Value {
        match v.get("cmd").and_then(Value::as_str).unwrap_or("") {
            "capabilities" => json!({ "ok": true, "appResolution": true, "streamQuality": true, "desktopScrcpy": true, "appScrcpy": true, "scrcpyFullscreen": true, "cancelStart": true, "adbWakeUnlock": true, "foldMouse": true, "scrcpyAudio": true, "independentAudio": true, "scrcpyInputSettings": true, "computerIme": computer_ime::available(), "displayRotation": true, "appLayout": self.borderless_supported(), "desktopResolution": true, "desktopScaling": true, "desktopBorderless": self.borderless_supported() }),
            "cancel" => {
                let Some(pkg)=v.get("pkg").and_then(Value::as_str).filter(|s|!s.is_empty()&&s.len()<=255) else {
                    return json!({"ok":false,"message":"cancel needs pkg"});
                };
                self.cancel_start(pkg);
                json!({"ok":true})
            }
            "desktop_window" => {
                let Some(borderless) = v.get("borderless").and_then(Value::as_bool) else {
                    return json!({"ok":false,"message":"desktop_window needs a boolean borderless"});
                };
                if borderless && !self.borderless_supported() {
                    return json!({"ok":false,"message":"borderless desktop needs the default XWayland renderer"});
                }
                if self.sessions.values().any(|s|s.pkg=="desktop"&&s.renderer==desktop_renderer::Renderer::Scrcpy){
                    return json!({"ok":false,"message":"Restart the scrcpy desktop to change window decorations"});
                }
                self.desktop_borderless = borderless;
                for session in self.sessions.values().filter(|s| s.pkg == "desktop") {
                    session.borderless.store(borderless, Ordering::Relaxed);
                }
                json!({"ok":true})
            }
            "app" => {
                let Some(pkg) = v.get("pkg").and_then(Value::as_str) else {
                    return json!({ "ok": false, "message": "app needs pkg" });
                };
                let renderer=match desktop_renderer::Renderer::from_request(v){Ok(r)=>r,Err(e)=>return json!({"ok":false,"message":e})};
                let scrcpy_options=match desktop_renderer::ScrcpyOptions::from_request(v){Ok(o)=>o,Err(e)=>return json!({"ok":false,"message":e})};
                let quality=match stream_settings::Quality::from_request(v) {Ok(q)=>q,Err(e)=>return json!({"ok":false,"message":e})};
                let layout=match stream_settings::Layout::from_request(v) {Ok(l)=>l,Err(e)=>return json!({"ok":false,"message":e})};
                if layout.fixed && !self.borderless_supported() {return json!({"ok":false,"message":"Fixed app aspect ratio needs the default XWayland renderer"});}
                let display=match app_display::from_request(v) {
                    Ok(display)=>display, Err(error)=>return json!({"ok":false,"message":error}),
                };
                if layout.fixed && display.is_none() {return json!({"ok":false,"message":"Fixed aspect ratio requires custom app resolution"});}
                if let Some(d)=display.filter(|_|layout.fixed) {
                    if layout.width_for_height(d.height).is_none_or(|w|w.abs_diff(d.width)>2) {return json!({"ok":false,"message":"App resolution does not match the fixed aspect ratio"});}
                }
                if (display.is_some() || quality.custom() || layout.fixed || renderer==desktop_renderer::Renderer::Scrcpy || scrcpy_options.wake_unlock) && views::is_view_pkg(pkg) { return json!({"ok":false,"message":"Use desktop settings for view sessions"}); }
                if let Some(s) = self.sessions.values().find(|s| s.pkg == pkg && !s.headless) {
                    if s.requested_display != display || s.quality!=quality || s.layout!=layout || s.renderer!=renderer || s.scrcpy_options!=scrcpy_options { return json!({"ok":false,"message":"Close this app window before changing its stream settings"}); }
                    raise_window(s.title.lock().unwrap().clone());
                }
                if self.inflight.as_ref().map(|(r,_)|r).into_iter().chain(self.queue.iter())
                    .any(|r|r.pkg==pkg && (r.display!=display || r.quality!=quality || r.layout!=layout || r.renderer!=renderer || r.scrcpy_options!=scrcpy_options)) { return json!({"ok":false,"message":"This app is already starting with different display settings"}); }
                if self.wakes.contains_key(pkg) {return json!({"ok":false,"message":"This app is already starting"});}
                let mut request=start_req_for(pkg, v.get("cmp").and_then(Value::as_str), &self.apps);
                self.cancelled_starts.remove(pkg);
                self.app_errors.remove(pkg);
                request.display=display;request.quality=quality;request.layout=layout;request.renderer=renderer;request.scrcpy_options=scrcpy_options;
                let options=request.scrcpy_options.clone();
                self.prepare_start(pkg,options,PreparedStart::App(request));
                json!({ "ok": true, "message": "requested" })
            }
            "sessions" => json!({ "ok": true, "desktopError":self.desktop_error,"desktopPending":self.desktop_pending,"mirrorPending":self.mirror_pending,"pending":self.pending_packages(),"adbPreparing":self.wakes.keys().collect::<Vec<_>>(),"appErrors":self.app_errors, "sessions": self.sessions.values().map(|s| json!({
                "sessionId": s.sid, "pkg": s.pkg, "displayId": s.display_id(), "title": *s.title.lock().unwrap(),
                "headless": s.headless,
                "borderless": s.borderless.load(Ordering::Relaxed),
                "renderStats": s.render_stats(),
                "density": s.stream_density(),
                "requestedScale": if s.pkg == "desktop" { Some(self.desktop_scale) } else { None },
                "videoProgress": s.progress.lock().unwrap().snapshot(Instant::now()),
                "streamTiming": s.timing.lock().unwrap().snapshot(),
                "requestedQuality": s.quality, "requestedLayout": s.layout,
                "renderer":s.renderer,"rotation":s.rotation(),"fullscreen":s.fullscreen,"requestedScrcpy":s.scrcpy_options,"scrcpyState":s.external_state().map(|state|state.phase),
                "scrcpyPid":s.external_state().and_then(|state|state.pid),
                "displayRateDropped": s.frame_limiter.lock().unwrap().dropped,
                "jitterStats": stream_timing::jitter_stats(&s.pipeline),
                "udpReceiveBuffers": s.receive_buffers.lock().unwrap().sizes(),
                "width": s.stream_size().map(|size| size.0), "height": s.stream_size().map(|size| size.1),
                "requestedWidth": if s.pkg == "desktop" { self.view_resolution.map(|size| size.0) } else { s.requested_display.map(|p| i64::from(p.width)) },
                "requestedHeight": if s.pkg == "desktop" { self.view_resolution.map(|size| size.1) } else { s.requested_display.map(|p| i64::from(p.height)) },
                "requestedDensity": s.requested_display.map(|p|p.density),
            })).collect::<Vec<_>>() }),
            "close" => match v.get("sessionId").and_then(Value::as_i64) {
                Some(sid) if self.sessions.contains_key(&sid) => {
                    self.close_session(sid, "closed via control socket", true);
                    json!({ "ok": true })
                }
                _ => json!({ "ok": false, "message": "no such session" }),
            },
            "apps" => json!({ "ok": true, "apps": self.apps }),
            "mirror" => {
                self.cancelled_starts.remove("mirror");self.mirror_pending=true;
                println!("asking the phone to start mirroring");
                self.d.send(1, views::start(1));
                json!({ "ok": true, "message": "requested" })
            }
            "desktop" => {
                let renderer=match desktop_renderer::Renderer::from_request(v){Ok(r)=>r,Err(e)=>return json!({"ok":false,"message":e})};
                let scrcpy_options=match desktop_renderer::ScrcpyOptions::from_request(v){Ok(o)=>o,Err(e)=>return json!({"ok":false,"message":e})};
                let fullscreen=match v.get("fullscreen"){None=>false,Some(value)=>match value.as_bool(){Some(value)=>value,None=>return json!({"ok":false,"message":"Fullscreen must be a boolean"})}};
                let fullscreen=fullscreen&&renderer==desktop_renderer::Renderer::Scrcpy;
                let quality=match stream_settings::Quality::from_request(v) {Ok(q)=>q,Err(e)=>return json!({"ok":false,"message":e})};
                let size = match views::requested_desktop_size(v) {
                    Ok(size) => size,
                    Err(error) => return json!({ "ok": false, "message": error.to_string() }),
                };
                let scale = match views::requested_desktop_scale(v) {
                    Ok(scale) => scale,
                    Err(error) => return json!({"ok":false,"message":error.to_string()}),
                };
                let borderless = match v.get("borderless") {
                    Some(value) => match value.as_bool() {
                        Some(value) => value,
                        None => return json!({"ok":false,"message":"desktop needs a boolean borderless"}),
                    },
                    None => self.desktop_borderless,
                };
                if renderer==desktop_renderer::Renderer::Builtin && borderless && !self.borderless_supported() {
                    return json!({"ok":false,"message":"borderless desktop needs the default XWayland renderer"});
                }
                if let Some(session) = self.sessions.values().find(|s| s.pkg == "desktop") {
                    if (size.is_some() && size != self.view_resolution) || scale != self.desktop_scale || quality != self.desktop_quality || renderer!=self.desktop_renderer || fullscreen!=self.desktop_fullscreen || (renderer==desktop_renderer::Renderer::Scrcpy&&scrcpy_options!=self.desktop_scrcpy) {
                        return json!({ "ok": false, "message": "Close the desktop session before changing its resolution, scaling or quality" });
                    }
                    self.desktop_borderless = borderless;
                    session.borderless.store(borderless, Ordering::Relaxed);
                    raise_window(session.title.lock().unwrap().clone());
                    return json!({ "ok": true, "message": "already running" });
                }
                if self.sessions.values().any(|s| s.pkg == "mirror" || s.pkg == "mobile") {
                    return json!({ "ok": false, "message": "close screen mirroring or virtual phone before starting desktop mode" });
                }
                if self.wakes.contains_key("desktop") || self.inflight.as_ref().is_some_and(|(r,_)|r.pkg=="desktop") {
                    return json!({"ok":false,"message":"Desktop mode is already starting"});
                }
                self.cancelled_starts.remove("desktop");
                self.desktop_error=None;self.desktop_pending=true;
                self.desktop_renderer=renderer;self.desktop_fullscreen=fullscreen;self.desktop_scrcpy=scrcpy_options;
                self.desktop_quality = quality;
                self.view_resolution = size;
                self.desktop_scale = scale;
                self.desktop_borderless = borderless;
                self.prepare_start("desktop",self.desktop_scrcpy.clone(),PreparedStart::Desktop);
                json!({ "ok": true, "message": "requested" })
            }
            _ => {
                json!({ "ok": false, "message": "commands: app {pkg,cmp?}, sessions, close {sessionId}, apps, mirror, desktop {width?,height?}, capabilities" })
            }
        }
    }

    /// Periodic work on the main loop.
    fn tick(&mut self) {
        let now = Instant::now();
        self.poll_wakes();
        for session in self.sessions.values() {
            session.receive_buffers.lock().unwrap().update(&session.pipeline);
            session.watch_window_close();
            session.recover_video(now);
        }
        self.cancelled_starts.retain(|_,since| now.duration_since(*since)<Duration::from_secs(30));
        let external_done:Vec<_>=self.sessions.values().filter_map(|s|s.poll_external().map(|(why,error)|(s.sid,s.pkg.clone(),why,error))).collect();
        for (sid,pkg,why,error) in external_done {
            if pkg=="desktop"{self.desktop_error=error;}else if let Some(error)=error {self.app_errors.insert(pkg,error);}

            self.close_session(sid,&why,true);
        }
        let dead: Vec<(i64, String)> =
            self.sessions.values().filter_map(|s| s.poll_dead().map(|w| (s.sid, w))).collect();
        for (sid, why) in dead {
            self.close_session(sid, &why, true);
        }
        let done: Vec<(i64, &str)> =
            self.sessions.values().filter_map(|s| s.headless_due(now).map(|w| (s.sid, w))).collect();
        for (sid, why) in done {
            self.close_session(sid, why, true);
        }
        if let Some(x) = &self.x11 {
            for s in self.sessions.values() {
                s.sync_x11_title(x);
                s.sync_desktop_hover(x);
                s.sync_window_shape(x);
            }
            if self.resize && now.duration_since(self.last_resize_poll) >= RESIZE_POLL {
                self.last_resize_poll = now;
                for s in self.sessions.values() {
                    if let Some(mut req) = s.poll_resize(x, self.instance_id, now) {
                        if s.pkg == "desktop" {
                            let size = (req["width"].as_i64().unwrap(), req["height"].as_i64().unwrap());
                            for (key, value) in views::desktop_display(size, self.desktop_scale).as_object().unwrap() {
                                req[key] = value.clone();
                            }
                        }
                        self.d.send15(17, Some(req));
                    }
                }
            }
        }
        if now.duration_since(self.last_log_stats) >= Duration::from_secs(5) {
            self.last_log_stats = now;
            for session in self.sessions.values().filter(|session| !session.headless) {
                let progress = session.progress.lock().unwrap().snapshot(now);
                diagnostics::record("INFO", &format!("[{}] stats pkg={} renderer={:?} size={:?} density={:?} render={} progress={progress} timing={} jitter={}",session.sid,session.pkg,session.renderer,
                    session.stream_size(),session.stream_density(),session.render_stats().unwrap_or(Value::Null),
                    session.timing.lock().unwrap().snapshot(),stream_timing::jitter_stats(&session.pipeline)));
            }
        }
        // One heartbeat covers every session of this PC.
        if !self.sessions.is_empty() && self.last_hb.elapsed() >= Duration::from_secs(30) {
            self.d.send15(9, None);
            self.last_hb = Instant::now();
        }
        self.pump();
        if self.exit_when_idle
            && self.had_session
            && self.sessions.is_empty()
            && self.inflight.is_none()
            && self.queue.is_empty()
        {
            println!("all windows closed");
            self.main_loop.quit();
        }
    }
}



/// svc 17 cmd 1 → `[{pkg,name,cmp,userId,iconName,work}]` (personal + work-profile apps; the
/// download token/port are dropped). `iconName` is the file name the GUI looks up in the icon cache.
fn app_list(msg: &Value) -> Value {
    let pick = |key: &str| -> Vec<Value> {
        msg.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|a| {
                json!({
                    "pkg": a["pkg"], "name": a["name"], "cmp": a["cmp"], "userId": a["userId"],
                    "iconName": a["iconName"], "work": key == "wpApps",
                })
            })
            .collect()
    };
    let mut all = pick("apps");
    all.extend(pick("wpApps"));
    Value::Array(all)
}

fn save_apps(apps: &Value) {
    let path = apps_cache_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, apps.to_string());
}

fn load_apps() -> Value {
    std::fs::read_to_string(apps_cache_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!([]))
}

/// A PC-initiated start (source 0). `cmp`/`userId`/name come from the cached app list when known;
/// an empty `cmp` makes the phone resolve the package's launcher activity.
fn start_req_for(pkg: &str, cmp: Option<&str>, apps: &Value) -> StartReq {
    let mut list = apps.clone();
    if list.as_array().is_none_or(Vec::is_empty) {
        list = load_apps();
    }
    let known = list.as_array().into_iter().flatten().find(|a| a["pkg"].as_str() == Some(pkg)).cloned();
    let cmp = cmp.map(|c| json!(c)).or_else(|| known.as_ref().map(|a| a["cmp"].clone())).unwrap_or(json!(""));
    StartReq {
        pkg: pkg.to_owned(),
        cmp,
        user_id: known.as_ref().map(|a| a["userId"].clone()).filter(|u| !u.is_null()).unwrap_or(json!(0)),
        shortcut_id: Value::Null,
        source: 0,
        name: known.as_ref().and_then(|a| a["name"].as_str()).map(str::to_owned),
        headless: false,
        display: None,
        quality: Default::default(),
        layout: Default::default(),
        renderer: Default::default(),
        fullscreen:false,
        scrcpy_options: Default::default(),
    }
}

/// Keep only H.264 payload types (the phone lists H.265 first, which we don't decode yet).
fn h264_only(sdp: &str) -> String {
    let lines: Vec<&str> = sdp.split("\r\n").flat_map(|l| l.split('\n')).collect();
    let h264: Vec<String> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("a=rtpmap:"))
        .filter(|r| r.contains(" H264/90000"))
        .filter_map(|r| r.split(' ').next().map(str::to_owned))
        .collect();
    if h264.is_empty() {
        eprintln!("offer has no H.264; leaving codecs as offered");
        return sdp.to_owned();
    }
    let pt_of = |l: &str| -> Option<String> {
        ["a=rtpmap:", "a=fmtp:", "a=rtcp-fb:"]
            .iter()
            .find_map(|p| l.strip_prefix(p))
            .and_then(|r| r.split(' ').next())
            .map(str::to_owned)
    };
    let mut out = Vec::new();
    let mut video=false;
    for l in lines {
        if l.starts_with("m=") {video=l.starts_with("m=video ");}
        if !video {out.push(l.to_owned());continue;}
        if l.starts_with("a=extmap:") && l.contains("urn:3gpp:video-orientation") {
            continue; // make the phone rotate frames itself instead of tagging them
        }
        if l.starts_with("m=video") {
            let parts: Vec<&str> = l.split(' ').collect();
            if parts.len()<3 {out.push(l.to_owned());continue;}
            let keep: Vec<&str> = parts[3..].iter().copied().filter(|pt| h264.iter().any(|h| h == pt)).collect();
            out.push(format!("{} {}", parts[..3].join(" "), keep.join(" ")));
        } else if let Some(pt) = pt_of(l) {
            if h264.contains(&pt) {
                out.push(l.to_owned());
            }
        } else {
            out.push(l.to_owned());
        }
    }
    out.join("\r\n")
}

const BINDINGS: &str = "keys: everything is sent as real key presses (Ctrl+C/V/A, Alt+Tab, arrows, F-keys…)
  Esc            → Android BACK      (Shift+Esc sends a real Escape)
  Ctrl+Alt+B     → BACK   (also mouse back button)
  Ctrl+Alt+H     → HOME
  Ctrl+Alt+R     → RECENTS (app switcher)
  Super combos are usually taken by the desktop compositor.";

const USAGE: &str = "usage: smartconnect-mirror [--host [--resize] | --desktop [--size WxH] [--borderless] | --app PKG [CMP] | --list-apps | --sessions | --close SESSIONID]
  (no args)        mirror the whole phone screen
  --desktop        Ready For desktop on a phone virtual display (experimental)
  --size WxH       with --desktop: request a landscape resolution, e.g. 1920x1080
  --borderless     with --desktop: hide the window title bar and borders (XWayland)
  --host           stay up: open a window for every stream the phone starts here (notification
                   Open, App streaming / Screen share / Mobile desktop tiles, hand-off), run
                   notification action buttons without a window, and accept requests on
                   $XDG_RUNTIME_DIR/smartconnect-mirror.sock
  --resize         (with --host) ask the phone to resize app displays to their windows (experimental;
                   also SMARTCONNECT_MIRROR_RESIZE=1)
  --app PKG [CMP] [--size WxH] [--dpi DPI]  stream one app, optionally on a custom display
  --list-apps [--json]  print the phone's launchable apps (pkg, userId, name, cmp; --json: one JSON array)
  --sessions       print the host's open sessions (JSON)
  --logs           show recent sanitized diagnostic logs (works while stopped)
  --log-path       print the diagnostic log path
  --close SID      close one host session
environment:
  SMARTCONNECT_BACKGROUND_ACTIONS=0   notification action buttons open a window like Open does
  SMARTCONNECT_ICE_ADDRESSES=IP,...   local ICE addresses to offer (IPs, IPv4 CIDRs, or all)";

/// Hardware H.264 decoding (VA-API) when available, else libav. Override: MIRROR_DECODER=…
fn pick_decoder() -> String {
    if let Ok(d) = std::env::var("MIRROR_DECODER") {
        return d;
    }
    let ok = |n: &str| gst::ElementFactory::find(n).is_some();
    if ok("vah264dec") { "vah264dec".into() } else { "avdec_h264".into() }
}

/// glimagesink forwards wheel events on Wayland (waylandsink has no scroll support). Override: MIRROR_SINK=…
fn pick_sink() -> String {
    if let Ok(s) = std::env::var("MIRROR_SINK") {
        return s;
    }
    if gst::ElementFactory::find("glimagesink").is_some() { "glimagesink".into() } else { "autovideosink".into() }
}

/// One request/reply on the host's control socket; None when no host is running.
fn host_call(req: &Value) -> Option<Value> {
    let mut s = UnixStream::connect(host_socket_path()).ok()?;
    s.write_all(format!("{req}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

fn serve_control(tx: mpsc::Sender<Ev>) -> Result<()> {
    let path = host_socket_path();
    if UnixStream::connect(&path).is_ok() {
        bail!("a host is already running ({})", path.display());
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::thread::spawn(move || {
        for conn in listener.incoming().map_while(Result::ok) {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let Ok(mut w) = conn.try_clone() else { return };
                for line in BufReader::new(conn).lines().map_while(Result::ok) {
                    let reply = match serde_json::from_str::<Value>(&line) {
                        Ok(v) => {
                            let (rtx, rrx) = mpsc::channel();
                            if tx.send(Ev::Ctl(v, rtx)).is_err() {
                                return;
                            }
                            rrx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| "{\"ok\":false}".into())
                        }
                        Err(e) => json!({ "ok": false, "message": e.to_string() }).to_string(),
                    };
                    if w.write_all(format!("{reply}\n").as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    Ok(())
}

/// `--list-apps`: asks the phone to re-send its app list (svc 17 cmd 2) and prints it.
/// With `json`, prints the list as one JSON array (`pkg`, `name`, `cmp`, `userId`, `work`) instead of TSV.
fn list_apps(json: bool) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    watch(&[17], tx)?;
    let d = Daemon::connect()?;
    std::thread::sleep(Duration::from_millis(200));
    d.send(17, json!({ "ver": "1", "cmd": 2 }));
    let deadline = Instant::now() + Duration::from_secs(15);
    let apps = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Ev::Phone(17, msg)) if msg.get("cmd").and_then(Value::as_i64) == Some(1) => break app_list(&msg),
            Ok(Ev::Phone(..)) => continue,
            _ => {
                let cached = load_apps();
                if cached.as_array().is_some_and(|a| !a.is_empty()) {
                    eprintln!("the phone did not answer; showing the cached list ({})", apps_cache_path().display());
                    break cached;
                }
                bail!("the phone did not send its app list (svc 17) within 15 s");
            }
        }
    };
    save_apps(&apps);
    if json {
        println!("{apps}");
        return Ok(());
    }
    for a in apps.as_array().into_iter().flatten() {
        println!(
            "{}\t{}\t{}\t{}",
            a["pkg"].as_str().unwrap_or(""),
            a["userId"],
            a["name"].as_str().unwrap_or(""),
            a["cmp"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let result = run();
    if let Err(error) = &result { diagnostics::record("ERROR", &format!("{error:#}")); }
    diagnostics::flush();
    result
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    let app_options=if mode=="--app" {
        args.get(1).context("--app needs a package name")?;
        Some(app_display::cli_options(&args[2..]).map_err(|error|anyhow!(error))?)
    } else { None };
    let (desktop_size, desktop_borderless) = if mode == "--desktop" {
        views::desktop_cli_options(&args[1..])?
    } else { (None, false) };
    match mode {
        "-h" | "--help" => {
            println!("{USAGE}");
            return Ok(());
        }
        "--logs" => { std::println!("{}", mirror_log::read_recent(mirror_log::VIEW_LIMIT)?); return Ok(()); }
        "--log-path" => { std::println!("{}", mirror_log::path().display()); return Ok(()); }
        "--list-apps" => return list_apps(args.get(1).is_some_and(|a| a == "--json")),
        "--sessions" => {
            let r = host_call(&json!({ "cmd": "sessions" })).context("no host running")?;
            println!("{r}");
            return Ok(());
        }
        "--close" => {
            let sid: i64 = args.get(1).context("--close SESSIONID")?.parse()?;
            println!("{}", host_call(&json!({ "cmd": "close", "sessionId": sid })).context("no host running")?);
            return Ok(());
        }
        "--app" => {
            let pkg = args.get(1).context("--app needs a package name")?;
            let mut req = json!({ "cmd": "app", "pkg": pkg });
            let (component,display)=app_options.as_ref().unwrap();
            if let Some(cmp) = component { req["cmp"] = json!(cmp); }
            if let Some(display)=display {
                let caps=host_call(&json!({"cmd":"capabilities"}));
                if caps.is_some() && caps.as_ref().is_none_or(|value|value["appResolution"]!=true) {
                    bail!("the running stream host does not support custom app resolution; restart it");
                }
                req["width"]=json!(display.width);req["height"]=json!(display.height);req["density"]=json!(display.density);
            }
            if let Some(r) = host_call(&req) {
                if r["ok"]!=true {bail!("{}",r["message"].as_str().unwrap_or("app request failed"));}
                println!("host: {r}");
                return Ok(());
            }
            println!("no host running: streaming {pkg} from this process");
        }
        // A running host owns the views too (one process, one instanceId, no double answers).
        "" | "--desktop" => {
            let cmd = if mode.is_empty() { "mirror" } else { "desktop" };
            let mut request = json!({ "cmd": cmd });
            if mode == "--desktop" { request["borderless"] = json!(desktop_borderless); }
            if desktop_borderless {
                let caps = host_call(&json!({"cmd":"capabilities"}));
                if caps.is_some() && caps.as_ref().is_none_or(|caps| caps["desktopBorderless"] != true) {
                    bail!("the running stream host does not support borderless desktop windows");
                }
            }
            if let Some((width, height)) = desktop_size {
                let caps = host_call(&json!({ "cmd": "capabilities" }));
                if caps.is_some() && caps.as_ref().is_none_or(|v| v["desktopResolution"] != true) {
                    bail!("the running stream host does not support desktop resolution settings; restart it");
                }
                request["width"] = json!(width);
                request["height"] = json!(height);
            }
            if let Some(r) = host_call(&request) {
                if r["ok"] != true { bail!("{}", r["message"].as_str().unwrap_or("request failed")); }
                println!("host: {r}");
                return Ok(());
            }
        }
        "--host" => {}
        other => bail!("unknown argument {other}\n{USAGE}"),
    }
    for extra in args.iter().skip(1).filter(|a| a.starts_with("--") && mode != "--app") {
        if !(mode == "--host" && extra == "--resize" || mode == "--desktop" && matches!(extra.as_str(), "--size" | "--borderless")) {
            eprintln!("ignoring unknown argument {extra} for {mode:?}");
        }
    }

    // GL's Wayland window has no keyboard support, its X11 window has keys + wheel + buttons:
    // prefer X11 (XWayland) when there is one. MIRROR_GL_WAYLAND=1 keeps native Wayland.
    if std::env::var_os("DISPLAY").is_some()
        && std::env::var_os("GST_GL_WINDOW").is_none()
        && std::env::var_os("MIRROR_GL_WAYLAND").is_none()
    {
        // SAFETY: single-threaded here, before GStreamer or any other thread starts.
        unsafe { std::env::set_var("GST_GL_WINDOW", "x11") };
    }
    diagnostics::init();
    gst::init()?;
    diagnostics::attach_gstreamer();
    let main_loop = glib::MainLoop::new(None, false);

    let (tx, rx) = mpsc::channel::<Ev>();
    watch(&[1, 15, 17, actions::SERVICE_LOCAL_EVENTS], tx.clone())?;
    let d = Daemon::connect()?;
    let is_host = mode == "--host";
    if is_host {
        serve_control(tx.clone())?;
    }
    let env = |k: &str| std::env::var(k).ok();
    let resize =
        is_host && (args.iter().any(|a| a == "--resize") || resize::enabled(env(resize::ENV_RESIZE).as_deref()));
    let background = is_host && actions::enabled(env(actions::ENV_BACKGROUND).as_deref());
    let host = Rc::new(RefCell::new(Host {
        d: d.clone(),
        instance_id: instance_id(),
        own_mirror: matches!(mode, "" | "--host" | "--desktop"),
        own_apps: matches!(mode, "--host" | "--app"),
        answer_views: is_host,
        background,
        actions: actions::PendingActions::default(),
        resize,
        last_resize_poll: Instant::now(),
        view_resolution: desktop_size,
        desktop_scale: 100,
        desktop_borderless,
        desktop_quality: Default::default(),
        desktop_renderer: Default::default(),desktop_fullscreen:false,app_errors:HashMap::new(),desktop_scrcpy:Default::default(),desktop_error:None,desktop_pending:false,mirror_pending:false,cancelled_starts:HashMap::new(),wakes:HashMap::new(),wake_start:scrcpy::WakePreparation::start,
        exit_when_idle: !is_host,
        sessions: HashMap::new(),
        queue: VecDeque::new(),
        inflight: None,
        had_session: false,
        last_hb: Instant::now(),
        last_log_stats: Instant::now(),
        names: HashMap::new(),
        apps: load_apps(),
        x11: if std::env::var("GST_GL_WINDOW").as_deref() == Ok("x11") { X11::connect() } else { None },
        main_loop: main_loop.clone(),
    }));
    if desktop_borderless && !host.borrow().borderless_supported() {
        bail!("borderless desktop needs the default XWayland renderer");
    }
    {
        let mut h = host.borrow_mut();
        let apps = h.apps.clone();
        for a in apps.as_array().into_iter().flatten() {
            if let (Some(p), Some(n)) = (a["pkg"].as_str(), a["name"].as_str()) {
                h.names.insert(p.to_owned(), n.to_owned());
            }
        }
    }

    {
        let h = host.clone();
        glib::timeout_add_local(Duration::from_millis(15), move || {
            let mut h = h.borrow_mut();
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    Ev::DaemonGone => {
                        h.close_all("daemon went away", false);
                        h.main_loop.quit();
                        return glib::ControlFlow::Break;
                    }
                    Ev::Phone(svc, data) => {
                        if let Err(e) = h.on_frame(svc, &data) {
                            eprintln!("handling svc {svc}: {e:#}");
                        }
                    }
                    Ev::Ctl(v, reply) => {
                        let r = h.on_ctl(&v);
                        let _ = reply.send(r.to_string());
                    }
                }
            }
            h.tick();
            glib::ControlFlow::Continue
        });
    }
    for sig in [2, 15] {
        let h = host.clone();
        glib::unix_signal_add_local(sig, move || {
            let mut h = h.borrow_mut();
            h.close_all("signal", true);
            h.main_loop.quit();
            glib::ControlFlow::Break
        });
    }
    println!("{BINDINGS}");

    match mode {
        "" => {
            println!("asking the phone to start mirroring");
            d.send(1, views::start(1));
        }
        "--desktop" => {
            println!("asking the phone to start Ready For desktop");
            if let Some(size) = desktop_size { d.send(1, views::desktop_resolution(size)); }
            d.send(1, views::start(2));
        }
        "--app" => {
            let (component,display)=app_options.as_ref().unwrap();
            let mut h = host.borrow_mut();
            let mut r = start_req_for(&args[1], component.as_deref(), &h.apps);
            r.display=*display;
            h.request(r, false);
        }
        _ => println!(
            "host: waiting for streams (control socket {}); background notification actions {}, window resize {}",
            host_socket_path().display(),
            if background { "on" } else { "off" },
            if resize { "on" } else { "off" }
        ),
    }
    main_loop.run();
    if is_host {
        let _ = std::fs::remove_file(host_socket_path());
    }
    // Let the stop messages reach the daemon before the socket closes.
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h264_selection_preserves_audio_codec_and_parameters() {
        let sdp="v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96 97\r\na=rtpmap:96 H264/90000\r\na=rtpmap:97 VP8/90000\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\na=fmtp:111 minptime=10;useinbandfec=1\r\n";
        let filtered=h264_only(sdp);
        assert!(!filtered.contains("VP8"));
        assert!(filtered.contains("a=rtpmap:111 opus/48000/2"));
        assert!(filtered.contains("a=fmtp:111 minptime=10;useinbandfec=1"));
    }

    #[test]
    fn h264_only_keeps_h264_and_drops_orientation() {
        let offer = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96 98 102\r\na=mid:0\r\n\
                     a=extmap:3 urn:3gpp:video-orientation\r\n\
                     a=rtpmap:96 H265/90000\r\na=rtcp-fb:96 nack\r\n\
                     a=rtpmap:98 H264/90000\r\na=fmtp:98 profile-level-id=42e01f\r\n\
                     a=rtpmap:102 rtx/90000\r\na=fmtp:102 apt=96";
        let out = h264_only(offer);
        assert!(out.contains("m=video 9 UDP/TLS/RTP/SAVPF 98"));
        assert!(out.contains("a=rtpmap:98 H264/90000") && out.contains("a=fmtp:98 profile-level-id=42e01f"));
        assert!(!out.contains("H265") && !out.contains("rtx") && !out.contains("video-orientation"));
        assert!(out.contains("a=mid:0"));
    }

    /// A host whose "daemon" is one end of a socket pair; the other end sees what it sends.
    fn test_host(answer_views: bool) -> (Host, UnixStream) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        theirs.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let host = Host {
            d: Arc::new(Daemon { cmd: Mutex::new(ours) }),
            instance_id: 7,
            own_mirror: true,
            own_apps: true,
            answer_views,
            background: true,
            actions: actions::PendingActions::default(),
            resize: false,
            last_resize_poll: Instant::now(),
            view_resolution: None,
            desktop_scale: 100,
            desktop_borderless: false,
            desktop_quality: Default::default(),
        desktop_renderer: Default::default(),desktop_fullscreen:false,app_errors:HashMap::new(),desktop_scrcpy:Default::default(),desktop_error:None,desktop_pending:false,mirror_pending:false,cancelled_starts:HashMap::new(),wakes:HashMap::new(),wake_start:scrcpy::WakePreparation::pending_for_test,
            exit_when_idle: false,
            sessions: HashMap::new(),
            queue: VecDeque::new(),
            inflight: None,
            had_session: false,
            last_hb: Instant::now(),
            last_log_stats: Instant::now(),
            names: HashMap::new(),
            apps: json!([]),
            x11: None,
            main_loop: glib::MainLoop::new(None, false),
        };
        (host, theirs)
    }

    /// The `send` requests written so far, as (service, data).
    fn sent(theirs: &UnixStream) -> Vec<(i64, Value)> {
        let mut out = Vec::new();
        let mut r = BufReader::new(theirs);
        let mut line = String::new();
        while r.read_line(&mut line).is_ok_and(|n| n > 0) {
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(v["cmd"], "send");
            out.push((v["service"].as_i64().unwrap(), v["data"].clone()));
            line.clear();
        }
        out
    }

    fn click(kind: &str, pkg: &str) -> Value {
        json!({ "event": "notification_action", "kind": kind, "pkg": pkg, "key": format!("0|{pkg}|1|null|10001"), "actionId": 1, "service": 5 })
    }

    fn invite(pkg: &str) -> Value {
        json!({ "ver": "1", "cmd": 2, "data": { "deviceId": "<phone-id>", "pkg": pkg, "pkgName": "Example", "source": 1, "userId": 0, "cmp": null } })
    }

    #[test]
    #[ignore="requires an isolated X11 server"]
    fn desktop_hover_uses_only_the_uncovered_owned_client() {
        use x11rb::{connection::Connection as _, protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass}};
        let x = X11::connect().unwrap();
        let window = x.conn.generate_id().unwrap();
        x.conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, x.root, 0, 0, 300, 200, 0,
            WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new()).unwrap();
        x.conn.map_window(window).unwrap();
        x.conn.warp_pointer(0u32, window, 0, 0, 0, 0, 80, 2).unwrap();x.conn.flush().unwrap();
        assert_eq!(x.pointer_in_window(window), Some((80, 2)));
        let overlay = x.conn.generate_id().unwrap();
        x.conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, overlay, x.root, 0, 0, 300, 200, 0,
            WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new()).unwrap();
        x.conn.map_window(overlay).unwrap();x.conn.flush().unwrap();
        assert_eq!(x.pointer_in_window(window), None);
        x.conn.destroy_window(overlay).unwrap();x.conn.destroy_window(window).unwrap();x.conn.flush().unwrap();
    }

    #[test]
    fn adb_wake_precedes_phone_start_and_cancel_cannot_start_it_later() {
        let (mut host,theirs)=test_host(true);
        let req=json!({"cmd":"app","pkg":"com.example.notes","renderer":"builtin","scrcpy":{"wakeUnlock":true}});
        assert_eq!(host.on_ctl(&req)["ok"],true);
        assert!(host.inflight.is_none());assert!(sent(&theirs).is_empty());
        assert_eq!(host.pending_packages(),vec!["com.example.notes"]);
        host.wakes["com.example.notes"].task.finish_for_test(Ok(()));host.poll_wakes();
        assert_eq!(host.inflight.as_ref().unwrap().0.pkg,"com.example.notes");assert_eq!(sent(&theirs).len(),1);
        host.inflight=None;
        host.on_ctl(&json!({"cmd":"desktop","scrcpy":{"wakeUnlock":true}}));
        assert!(host.desktop_pending);assert!(sent(&theirs).is_empty());
        host.cancel_start("desktop");host.poll_wakes();assert!(!host.desktop_pending);assert!(host.wakes.is_empty());
        assert!(!sent(&theirs).iter().any(|(svc,v)|*svc==1&&v["cmd"]==1));
    }

    #[test]
    fn wake_failure_stops_before_phone_commands_and_keeps_other_errors() {
        let (mut host,theirs)=test_host(true);
        host.app_errors.insert("com.example.other".into(),"unchanged".into());
        host.on_ctl(&json!({"cmd":"desktop","scrcpy":{"wakeUnlock":true}}));
        host.wakes["desktop"].task.finish_for_test(Err("Unlock the phone on the device to continue".into()));
        host.poll_wakes();assert!(!host.desktop_pending);assert!(host.desktop_error.is_some());
        assert!(host.inflight.is_none());assert_eq!(host.app_errors["com.example.other"],"unchanged");assert!(sent(&theirs).is_empty());
    }

    #[test]
    #[ignore="requires an X11/XWayland desktop session"]
    fn unicode_titles_preserve_utf8_in_both_x11_properties() {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConnectionExt,AtomEnum,CreateWindowAux,WindowClass};
        let x=X11::connect().expect("X11 connection");let win=x.conn.generate_id().unwrap();
        x.conn.create_window(0,win,x.root,0,0,64,64,0,WindowClass::INPUT_OUTPUT,0,&CreateWindowAux::new()).unwrap().check().unwrap();
        let title="中文计算器 — Phone 🧮";x.set_title(win,title,true);
        for property in [x.net_wm_name,u32::from(AtomEnum::WM_NAME)] {
            let value=x.conn.get_property(false,win,property,AtomEnum::ANY,0,1024).unwrap().reply().unwrap();
            assert_eq!(value.type_,x.utf8);assert_eq!(std::str::from_utf8(&value.value).unwrap(),title);
        }
        x.conn.destroy_window(win).unwrap().check().unwrap();
    }

    #[test]
    fn cancelling_absent_view_does_not_stop_another_pending_view() {
        let (mut host,theirs)=test_host(true);
        host.on_ctl(&json!({"cmd":"mirror"}));let _=sent(&theirs);
        host.on_ctl(&json!({"cmd":"cancel","pkg":"desktop"}));
        assert!(host.mirror_pending);assert!(sent(&theirs).is_empty());
    }

    #[test]
    fn phone_confirmation_failure_clears_pending_and_reports_the_app_error() {
        let (mut host,theirs)=test_host(true);
        host.on_ctl(&json!({"cmd":"app","pkg":"com.example.one"}));
        host.on_frame(15,&json!({"cmd":8,"data":{"pkg":"com.example.one","state":0,"reason":147}})).unwrap();
        assert!(host.pending_packages().is_empty());
        assert_eq!(host.app_errors["com.example.one"],"Phone confirmation timed out; unlock the phone and try again");
        assert!(host.desktop_error.is_none());let _=sent(&theirs);
    }

    #[test]
    fn cancel_drops_queued_start_without_interrupting_other_app() {
        let (mut host,theirs)=test_host(true);
        host.on_ctl(&json!({"cmd":"app","pkg":"com.example.one"}));
        host.on_ctl(&json!({"cmd":"app","pkg":"com.example.two"}));
        assert_eq!(host.pending_packages(),vec!["com.example.one","com.example.two"]);
        assert_eq!(host.on_ctl(&json!({"cmd":"cancel","pkg":"com.example.two"}))["ok"],true);
        assert!(host.queue.is_empty());assert_eq!(host.inflight.as_ref().unwrap().0.pkg,"com.example.one");
        assert_eq!(sent(&theirs).len(),1);
    }

    #[test]
    fn cancelled_late_offer_is_closed_without_answering_or_creating_a_session() {
        let (mut host,theirs)=test_host(true);
        host.on_ctl(&json!({"cmd":"app","pkg":"com.example.one"}));
        host.on_ctl(&json!({"cmd":"app","pkg":"com.example.two"}));
        host.on_ctl(&json!({"cmd":"cancel","pkg":"com.example.one"}));
        assert_eq!(host.inflight.as_ref().unwrap().0.pkg,"com.example.two");
        host.on_frame(15,&json!({"cmd":3,"data":{"pkg":"com.example.one","sessionId":81,"webRTCData":"{}"}})).unwrap();
        host.on_frame(15,&invite("com.example.one")).unwrap();
        assert!(host.sessions.is_empty());assert!(host.queue.is_empty());
        let frames=sent(&theirs);
        assert!(frames.iter().any(|(svc,v)|*svc==15&&v["cmd"]==7&&v["data"]["sessionId"]==81));
        assert!(!frames.iter().any(|(_,v)|v["cmd"]==4));
    }

    #[test]
    fn cancel_view_clears_waiting_state_and_retry_is_explicit() {
        let (mut host,theirs)=test_host(true);
        host.on_ctl(&json!({"cmd":"desktop","renderer":"scrcpy"}));
        assert!(host.desktop_pending);
        host.on_ctl(&json!({"cmd":"cancel","pkg":"desktop"}));
        assert!(!host.desktop_pending);assert!(host.desktop_error.is_none());
        host.on_frame(1,&json!({"cmd":0,"view":2})).unwrap();
        host.on_frame(15,&invite("desktop")).unwrap();assert!(host.inflight.is_none());
        host.on_ctl(&json!({"cmd":"desktop","renderer":"scrcpy"}));
        host.on_frame(15,&invite("desktop")).unwrap();assert!(host.inflight.is_some());
        host.on_ctl(&json!({"cmd":"mirror"}));assert!(host.mirror_pending);
        host.on_ctl(&json!({"cmd":"cancel","pkg":"mirror"}));assert!(!host.mirror_pending);
        assert!(sent(&theirs).iter().any(|(svc,v)|*svc==1&&v["cmd"]==5));
    }

    #[test]
    fn custom_app_geometry_is_exact_and_does_not_leak_to_other_queued_apps() {
        let (mut host,theirs)=test_host(true);
        assert_eq!(host.on_ctl(&json!({"cmd":"capabilities"}))["appResolution"],true);
        assert_eq!(host.on_ctl(&json!({"cmd":"app","pkg":"com.example.one","width":720,"height":1280,"density":240}))["ok"],true);
        let out=sent(&theirs);let data=&out[0].1["data"];
        assert_eq!(data["width"],720);assert_eq!(data["displayHeight"],1280);
        assert_eq!(data["displayDensity"],240);assert_eq!(data["supportCustomDisplay"],true);
        assert_eq!(host.on_ctl(&json!({"cmd":"app","pkg":"com.example.one","width":1080,"height":1920}))["ok"],false);
        assert!(sent(&theirs).is_empty());
        assert_eq!(host.on_ctl(&json!({"cmd":"app","pkg":"com.example.two"}))["ok"],true);
        host.inflight=None;host.pump();let out=sent(&theirs);let data=&out[0].1["data"];
        assert_eq!(data["supportCustomDisplay"],false);assert_eq!(data["width"],1920);
        assert_eq!(data["displayDensity"],160);
    }

    #[test]
    fn scrcpy_app_profile_is_queued_independently_and_clears_only_its_error() {
        let (mut host,theirs)=test_host(true);
        host.desktop_error=Some("desktop error".into());host.app_errors.insert("com.example.test".into(),"app error".into());
        assert_eq!(host.on_ctl(&json!({"cmd":"app","pkg":"com.example.test","renderer":"scrcpy","width":1080,"height":1920,"quality":{"fps":30},"scrcpy":{"softwareDecode":true}}))["ok"],true);
        let req=&host.inflight.as_ref().unwrap().0;
        assert_eq!(req.renderer,desktop_renderer::Renderer::Scrcpy);assert!(!req.fullscreen);assert!(req.scrcpy_options.software_decode);assert_eq!(req.quality.fps,Some(30));
        assert!(host.app_errors.is_empty());assert_eq!(host.desktop_error.as_deref(),Some("desktop error"));
        host.inflight=None;
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop","renderer":"scrcpy","fullscreen":true}))["ok"],true);
        host.on_frame(15,&invite("desktop")).unwrap();assert!(host.inflight.as_ref().unwrap().0.fullscreen);
        let _=sent(&theirs);
    }

    #[test]
    fn scrcpy_desktop_settings_are_validated_and_stay_out_of_app_starts() {
        let (mut host,theirs)=test_host(true);
        assert_eq!(host.on_ctl(&json!({"cmd":"capabilities"}))["desktopScrcpy"],true);
        for value in [json!({"cmd":"desktop","renderer":"other"}),json!({"cmd":"desktop","renderer":"scrcpy","scrcpy":{"adbPort":0}})] {
            assert_eq!(host.on_ctl(&value)["ok"],false);
        }
        assert!(sent(&theirs).is_empty());
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop","renderer":"scrcpy","borderless":true,"scrcpy":{"softwareDecode":true}}))["ok"],true);
        assert!(host.desktop_pending);
        host.on_frame(15,&invite("desktop")).unwrap();
        let start=&host.inflight.as_ref().unwrap().0;
        assert_eq!(start.renderer,desktop_renderer::Renderer::Scrcpy);assert!(!start.headless);assert!(start.scrcpy_options.software_decode);
        host.inflight=None;
        host.on_ctl(&json!({"cmd":"app","pkg":"com.example.app"}));
        assert_eq!(host.inflight.as_ref().unwrap().0.renderer,desktop_renderer::Renderer::Builtin);
        let _=sent(&theirs);
    }

    #[test]
    fn invalid_quality_and_unsupported_layout_send_nothing() {
        let (mut host,theirs)=test_host(true);
        for value in [json!({"cmd":"app","pkg":"com.example.test","quality":{"fps":0}}),
            json!({"cmd":"desktop","quality":{"bitrateKbps":1}}),
            json!({"cmd":"app","pkg":"com.example.test","layout":{"fixed":true}})] {
            assert_eq!(host.on_ctl(&value)["ok"],false);
        }
        assert!(sent(&theirs).is_empty());assert!(host.inflight.is_none());
    }
    #[test]
    fn desktop_and_app_quality_are_kept_with_their_own_queued_start() {
        let (mut host,theirs)=test_host(true);
        let q=json!({"bitrateKbps":2000,"fps":15});
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop","quality":q}))["ok"],true);
        host.on_frame(15,&invite("desktop")).unwrap();
        assert_eq!(host.inflight.as_ref().unwrap().0.quality.fps,Some(15));
        host.inflight=None;
        host.on_ctl(&json!({"cmd":"app","pkg":"com.example.test","quality":{"fps":30}}));
        assert_eq!(host.inflight.as_ref().unwrap().0.quality.fps,Some(30));
        assert_eq!(host.inflight.as_ref().unwrap().0.quality.bitrate_kbps,None);
        host.inflight=None;
        host.on_ctl(&json!({"cmd":"app","pkg":"com.example.automatic"}));
        assert!(!host.inflight.as_ref().unwrap().0.quality.custom());
        let _=sent(&theirs);
    }

    #[test]
    fn invalid_app_custom_sizes_fail_before_phone_commands() {
        let (mut host,theirs)=test_host(true);
        for request in [json!({"cmd":"app","pkg":"com.example.one","width":721,"height":1280}),
            json!({"cmd":"app","pkg":"com.example.one","density":320}),
            json!({"cmd":"app","pkg":"mirror","width":720,"height":1280})] {
            assert_eq!(host.on_ctl(&request)["ok"],false);
        }
        assert!(sent(&theirs).is_empty());assert!(host.queue.is_empty());assert!(host.inflight.is_none());
    }

    #[test]
    fn desktop_control_sets_phone_resolution_and_stream_bound_without_affecting_apps() {
        let (mut host, theirs) = test_host(true);
        assert_eq!(host.on_ctl(&json!({"cmd":"capabilities"}))["desktopResolution"], true);
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop","width":1280,"height":720,"scale":200}))["ok"], true);
        host.on_frame(15, &invite("desktop")).unwrap();
        let out = sent(&theirs);
        assert_eq!(out.len(), 3);
        assert_eq!((out[0].0, out[0].1["cmd"].as_i64()), (1,Some(33)));
        assert_eq!(out[0].1["resolution"], json!({"width":1280,"height":720}));
        assert_eq!((out[1].0, out[1].1["view"].as_i64()), (1,Some(2)));
        assert_eq!(out[1].1["reply"], 0);
        assert_eq!(out[2].1["data"]["width"], 1280);
        assert_eq!(out[2].1["data"]["height"], 720);
        assert_eq!(out[2].1["data"]["displayDensity"], 320);
        assert_eq!(out[2].1["data"]["displayScale"], 2.0);
        host.inflight = None;
        host.on_frame(15, &invite("com.example.app")).unwrap();
        let out = sent(&theirs);
        assert_eq!(out[0].1["data"]["width"], 1920);
        assert_eq!(out[0].1["data"]["height"], 1080);
        assert_eq!(out[0].1["data"]["displayDensity"], 160);
        assert_eq!(out[0].1["data"]["displayScale"], 1.0);
    }

    #[test]
    fn invalid_desktop_control_sends_nothing_and_default_clears_previous_size() {
        let (mut host, theirs) = test_host(true);
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop","width":1281,"height":720}))["ok"], false);
        assert!(sent(&theirs).is_empty());
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop","scale":999}))["ok"],false);
        assert!(sent(&theirs).is_empty());
        host.view_resolution = Some((2560,1440));
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop"}))["ok"], true);
        assert_eq!(host.view_resolution, None);
        let out = sent(&theirs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1["cmd"], 1);
    }

    #[test]
    fn borderless_rejects_invalid_or_unsupported_requests_without_sending_to_phone() {
        let (mut host, theirs) = test_host(true);
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop_window","borderless":"yes"}))["ok"], false);
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop_window","borderless":true}))["ok"], false);
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop","borderless":true}))["ok"], false);
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop","borderless":1}))["ok"], false);
        assert_eq!(host.on_ctl(&json!({"cmd":"desktop_window","borderless":false}))["ok"], true);
        assert!(sent(&theirs).is_empty());
    }

    #[test]
    fn action_click_then_invite_runs_headless() {
        let (mut h, theirs) = test_host(true);
        h.on_frame(actions::SERVICE_LOCAL_EVENTS, &click("action", "com.example.clock")).unwrap();
        h.on_frame(15, &invite("com.example.clock")).unwrap();
        let (req, _) = h.inflight.as_ref().expect("cmd 1 in flight");
        assert!(req.headless);
        let out = sent(&theirs);
        assert_eq!(out.len(), 1);
        let (svc, data) = &out[0];
        assert_eq!((*svc, data["cmd"].as_i64()), (15, Some(1)));
        assert_eq!(data["data"]["pkg"], "com.example.clock");
        assert_eq!(data["data"]["source"], 1);
        assert_eq!(data["data"]["supportCustomDisplay"], false);
    }

    #[test]
    fn open_clicks_and_plain_invites_get_a_window() {
        let (mut h, _theirs) = test_host(true);
        h.on_frame(actions::SERVICE_LOCAL_EVENTS, &click("action", "com.a")).unwrap();
        h.on_frame(actions::SERVICE_LOCAL_EVENTS, &click("open", "com.a")).unwrap();
        h.on_frame(15, &invite("com.a")).unwrap();
        assert!(!h.inflight.as_ref().unwrap().0.headless, "Open wins over an earlier button click");
        let (mut h, _theirs) = test_host(true);
        h.on_frame(actions::SERVICE_LOCAL_EVENTS, &click("action", "com.other")).unwrap();
        h.on_frame(15, &invite("com.a")).unwrap();
        assert!(!h.inflight.as_ref().unwrap().0.headless, "a click for another app");
        let (mut h, _theirs) = test_host(true);
        h.background = false;
        h.on_frame(actions::SERVICE_LOCAL_EVENTS, &click("action", "com.a")).unwrap();
        h.on_frame(15, &invite("com.a")).unwrap();
        assert!(!h.inflight.as_ref().unwrap().0.headless, "SMARTCONNECT_BACKGROUND_ACTIONS=0");
    }

    #[test]
    fn repeated_invites_send_one_start() {
        let (mut h, theirs) = test_host(true);
        h.on_frame(15, &invite("com.a")).unwrap();
        h.on_frame(15, &invite("com.a")).unwrap();
        assert_eq!(sent(&theirs).len(), 1);
        assert!(h.queue.is_empty());
    }

    #[test]
    fn phone_view_invite_is_accepted_and_sized() {
        let (mut h, theirs) = test_host(true);
        let svc1 = json!({ "ver": "1.0.0", "cmd": 0, "view": 2, "resolution": { "width": 2560, "height": 1440 } });
        h.on_frame(1, &svc1).unwrap();
        h.on_frame(15, &invite("desktop")).unwrap();
        let out = sent(&theirs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, 1);
        assert_eq!(
            (out[0].1["cmd"].as_i64(), out[0].1["view"].as_i64(), out[0].1["reply"].as_i64()),
            (Some(1), Some(2), Some(1))
        );
        let start = &out[1].1["data"];
        assert_eq!((out[1].0, start["pkg"].as_str()), (15, Some("desktop")));
        assert_eq!((start["width"].as_i64(), start["height"].as_i64()), (Some(2560), Some(1440)));
        assert_eq!(h.title_for("desktop"), "Mobile desktop — Smart Connect");
    }

    #[test]
    fn only_the_host_answers_view_invites() {
        let (mut h, theirs) = test_host(false);
        h.on_frame(1, &json!({ "ver": "1.0.0", "cmd": 0, "view": 1 })).unwrap();
        h.on_frame(1, &json!({ "ver": "1.0.0", "cmd": 31, "view": 7 })).unwrap();
        assert!(sent(&theirs).is_empty());
    }

    #[test]
    fn start_request_defaults() {
        let apps = json!([{ "pkg": "com.example", "name": "Example", "cmp": "com.example.Main", "userId": 10 }]);
        let r = start_req_for("com.example", None, &apps);
        assert_eq!((r.cmp.as_str(), r.user_id.as_i64(), r.source), (Some("com.example.Main"), Some(10), 0));
        assert_eq!(r.name.as_deref(), Some("Example"));
        assert!(!r.headless, "PC-initiated starts always get a window");
    }
}
