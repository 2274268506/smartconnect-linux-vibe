//! A cancellable, session-owned scrcpy process. All ADB/process waits run off the host thread.
use crate::desktop_renderer::ScrcpyOptions;
use crate::stream_settings::Quality;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::os::fd::FromRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub phase: &'static str,
    pub error: Option<String>,
    pub pid: Option<u32>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            phase: "preparing",
            error: None,
            pid: None,
        }
    }
}
#[derive(Clone)]
pub struct Spec {
    pub display_id: i64,
    pub phone_address: Option<String>,
    pub options: ScrcpyOptions,
    pub quality: Quality,
    pub borderless: bool,
    pub session_id: i64,
    pub fullscreen: bool,
    pub window_size: Option<(u32, u32)>,
    pub title: String,
}
#[derive(Clone)]
struct Programs {
    adb: PathBuf,
    scrcpy: PathBuf,
    x11: bool,
    capture_power: bool,
}
fn binary(name: &str) -> PathBuf {
    let key = format!("SMARTCONNECT_{}_BIN", name.to_uppercase());
    std::env::var_os(key)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(name)))
                .filter(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from(name))
}
impl Programs {
    fn current() -> Self {
        Self {
            adb: binary("adb"),
            scrcpy: binary("scrcpy"),
            x11: std::env::var_os("DISPLAY").is_some(),
            capture_power: true,
        }
    }
}

pub struct Link {
    cancel: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
    text: Option<std::os::unix::net::UnixDatagram>,
}
impl Link {
    pub fn start(spec: Spec) -> Self {
        Self::with_programs(spec, Programs::current())
    }
    fn with_programs(spec: Spec, programs: Programs) -> Self {
        let channel=spec.options.computer_ime.then(crate::computer_ime::channel);
        let (text,child_text,channel_error)=match channel {
            Some(Ok((rx,tx)))=>(Some(rx),Some(tx),None),
            Some(Err(error))=>(None,None,Some(error)),
            None=>(None,None,None),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(State::default()));
        let stop = cancel.clone();
        let status = state.clone();
        std::thread::spawn(move || {
            crate::diagnostics::record(
                "INFO",
                &format!(
                    "[{}] scrcpy: preparing ADB display capture",
                    spec.session_id
                ),
            );
            let result = match channel_error {Some(error)=>Err(error),None=>run(&spec, &programs, &stop, &status, child_text.as_ref())};
            let mut s = status.lock().unwrap();
            if stop.load(Ordering::Acquire) {
                s.phase = "stopped";
                s.error = None;
            } else if let Err(error) = result {
                crate::diagnostics::record(
                    "ERROR",
                    &format!("[{}] scrcpy: {error}", spec.session_id),
                );
                s.phase = "failed";
                s.error = Some(error);
            } else {
                s.phase = "stopped";
            }
        });
        Self { cancel, state, text }
    }
    pub fn committed_text(&self)->Vec<String> {self.text.as_ref().map(crate::computer_ime::receive).unwrap_or_default()}
    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Drop for Link {
    fn drop(&mut self) {
        self.stop();
    }
}

fn capture(program: &PathBuf, args: &[String], cancel: &AtomicBool) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    parent_death(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "Could not run ADB or scrcpy; check installation and binary paths")?;
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(65536).read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
            kill_command(&mut child);
            return Err("ADB or scrcpy check cancelled or timed out".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err("ADB or scrcpy command failed; check the ADB connection".into());
                }
                let bytes = rx
                    .recv_timeout(Duration::from_secs(1))
                    .map_err(|_| "ADB output timed out")?;
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => {
                kill_command(&mut child);
                return Err("Could not query the ADB process".into());
            }
        }
    }
}
fn devices(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            let state = fields.next()?;
            matches!(state, "device" | "unauthorized" | "offline")
                .then(|| (serial.into(), state.into()))
        })
        .collect()
}
fn select_device(
    list: &[(String, String)],
    target: &str,
    peer: Option<IpAddr>,
) -> Result<Option<String>, String> {
    if !target.is_empty() {
        return match list.iter().find(|(s, _)| s == target) {
            Some((s, state)) if state == "device" => Ok(Some(s.clone())),
            Some((_, state)) if state == "unauthorized" => {
                Err("Authorize this computer for ADB on the phone".into())
            }
            Some(_) => Err("The selected ADB device is offline".into()),
            None => Ok(None),
        };
    }
    if let Some(ip) = peer {
        if let Some((s, state)) = list
            .iter()
            .find(|(s, _)| s.parse::<SocketAddr>().is_ok_and(|a| a.ip() == ip))
        {
            return if state == "device" {
                Ok(Some(s.clone()))
            } else {
                Err("Authorize ADB on the paired phone and reconnect it".into())
            };
        }
    }
    let online: Vec<_> = list.iter().filter(|(_, state)| state == "device").collect();
    match online.as_slice() {
        [entry] => Ok(Some(entry.0.clone())),
        [] => Ok(None),
        _ => Err(
            "Several ADB devices are connected; select an ADB device in desktop settings".into(),
        ),
    }
}
fn connect_device(
    options: &ScrcpyOptions,
    phone_address: Option<&str>,
    programs: &Programs,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let peer = phone_address.and_then(|s| s.parse::<IpAddr>().ok());
    let list = devices(&capture(&programs.adb, &["devices".into()], cancel)?);
    if let Some(serial) = select_device(&list, &options.adb_device, peer)? {
        return Ok(serial);
    }
    let address = if !options.adb_device.is_empty() {
        options
            .adb_device
            .parse::<SocketAddr>()
            .ok()
            .map(|s| s.to_string())
    } else {
        peer.map(|ip| SocketAddr::new(ip, options.adb_port).to_string())
    };
    if let Some(address) = address {
        let _ = capture(&programs.adb, &["connect".into(), address], cancel)?;
        let list = devices(&capture(&programs.adb, &["devices".into()], cancel)?);
        if let Some(serial) = select_device(&list, &options.adb_device, peer)? {
            return Ok(serial);
        }
    }
    Err("No authorized ADB device is available; enable USB or wireless ADB and authorize this computer".into())
}
/// Wake/dismiss work runs before sending Smart Connect start commands, not after
/// creating its virtual display. Dropping the task cancels any owned ADB command.
pub struct WakePreparation {
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}
impl WakePreparation {
    pub fn start(options: ScrcpyOptions, phone_address: Option<String>) -> Self {
        Self::with_programs(options, phone_address, Programs::current())
    }
    fn with_programs(
        options: ScrcpyOptions,
        phone_address: Option<String>,
        programs: Programs,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let stop = cancel.clone();
        let output = result.clone();
        std::thread::spawn(move || {
            let value = prepare_phone(&options, phone_address.as_deref(), &programs, &stop);
            *output.lock().unwrap() = Some(value);
        });
        Self { cancel, result }
    }
    #[cfg(test)]
    pub fn pending_for_test(_: ScrcpyOptions, _: Option<String>) -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            result: Arc::new(Mutex::new(None)),
        }
    }
    #[cfg(test)]
    pub fn finish_for_test(&self, value: Result<(), String>) {
        *self.result.lock().unwrap() = Some(value);
    }
    pub fn result(&self) -> Option<Result<(), String>> {
        self.result.lock().unwrap().clone()
    }
}
impl Drop for WakePreparation {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
fn keyguard_showing(policy: &str) -> Option<bool> {
    policy.lines().find_map(|line| match line.trim() {
        "mIsShowing=true" => Some(true),
        "mIsShowing=false" => Some(false),
        _ => None,
    })
}
fn prepare_phone(
    options: &ScrcpyOptions,
    phone_address: Option<&str>,
    programs: &Programs,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let serial = connect_device(options, phone_address, programs, cancel)?;
    let shell = |args: &[&str]| {
        let mut command = vec!["-s".into(), serial.clone(), "shell".into()];
        command.extend(args.iter().map(|s| s.to_string()));
        capture(&programs.adb, &command, cancel)
    };
    shell(&["input", "keyevent", "KEYCODE_WAKEUP"])?;
    if cancel.load(Ordering::Acquire) {
        return Err("Phone preparation cancelled".into());
    }
    // A credential-protected keyguard cannot be dismissed by this command.
    let _ = shell(&["wm", "dismiss-keyguard"]);
    for _ in 0..8 {
        if cancel.load(Ordering::Acquire) {
            return Err("Phone preparation cancelled".into());
        }
        match keyguard_showing(&shell(&["dumpsys", "window", "policy"])?) {
            Some(false) => return Ok(()),
            Some(true) => std::thread::sleep(Duration::from_millis(150)),
            None => return Err("Could not verify the phone lock state; unlock it manually".into()),
        }
    }
    Err("Unlock the phone on the device to continue".into())
}

/// scrcpy 4+ binds UHID mouse and keyboard devices to the original virtual source on Android 15+.
/// SDK pointer injection instead targets the mirror display, which can be noninteractive.
fn use_bound_input(enabled: bool, display: i64, sdk: Option<u32>, scrcpy_major: u32) -> bool {
    enabled && display > 0 && sdk.is_some_and(|sdk| sdk >= 35) && scrcpy_major >= 4
}

fn validate_audio(options: &ScrcpyOptions, sdk: Option<u32>) -> Result<(),String> {
    if options.audio && options.independent_audio && sdk.is_none_or(|level|level<33) {
        return Err("Independent audio requires Android 13+; disable independent audio to use phone output".into());
    }
    Ok(())
}

fn arguments(spec: &Spec, serial: &str, supports_hwdec: bool, uhid_mouse: bool, uhid_keyboard: bool) -> Vec<String> {
    let mut args = vec![
        "-s".into(),
        serial.into(),
        format!("--display-id={}", spec.display_id),
        if !spec.options.audio {"--no-audio".into()} else if spec.options.independent_audio {"--audio-source=playback".into()} else {"--audio-source=output".into()},
        if uhid_keyboard {"--keyboard=uhid".into()} else {"--keyboard=sdk".into()},
        if uhid_mouse {
            "--mouse=uhid".into()
        } else {
            "--mouse=sdk".into()
        },
        format!(
            "--window-title={}",
            spec.title
                .chars()
                .filter(|c| !c.is_control())
                .take(180)
                .collect::<String>()
        ),
    ];
    if spec.options.computer_ime {args.push("--prefer-text".into());}
    if spec.fullscreen {
        args.push("--fullscreen".into());
    } else if let Some((w, h)) = spec.window_size {
        args.push(format!("--window-width={w}"));
        args.push(format!("--window-height={h}"));
    }
    if let Some(b) = spec.quality.bitrate_kbps {
        args.push(format!("--video-bit-rate={}", b * 1000));
    }
    if let Some(f) = spec.quality.fps {
        args.push(format!("--max-fps={f}"));
    }
    if spec.borderless {
        args.push("--window-borderless".into());
    }
    if spec.options.software_decode && supports_hwdec {
        args.push("--hwdec=disabled".into());
    }
    args
}
fn configure_input(command: &mut Command, options: &ScrcpyOptions, uhid_mouse: bool, uhid_keyboard: bool) {
    // Phone-side composition needs physical key events, not desktop IME text commits.
    if uhid_keyboard {
        command.env("SDL_IM_MODULE", "none").env("XMODIFIERS", "@im=none");
    }
    if uhid_mouse {
        command.env("SDL_MOUSE_RELATIVE_SPEED_SCALE", format!("{:.2}", f64::from(options.mouse_speed_percent)/100.));
    }
}
fn clean_line(line: &str, serial: &str) -> Option<String> {
    static VT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let vt = VT.get_or_init(|| {
        regex::Regex::new(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]").unwrap()
    });
    let stripped = vt.replace_all(line, "");
    let line = stripped.as_ref();
    if line.contains("(usb)") || line.contains("(tcpip)") || line.contains("ADB device found") {
        return None;
    }
    Some(
        line.replace(serial, "<adb-device>")
            .chars()
            .filter(|c| *c != '\r' && !c.is_control())
            .take(4096)
            .collect(),
    )
}
fn terminal() -> Result<(File, File), String> {
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: openpty writes two owned FDs; null name/termios/winsize select defaults.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result != 0 {
        return Err("Could not open the scrcpy log terminal".into());
    }
    // CLOEXEC prevents a child or helper from keeping the reader alive accidentally.
    unsafe {
        libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(slave, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    Ok(unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) })
}
fn log_reader(master: File, serial: String, sid: i64, state: Arc<Mutex<State>>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(master);
        let mut chunk = [0; 1024];
        let mut line = Vec::new();
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            for &b in &chunk[..n] {
                if b == b'\n' {
                    let text = String::from_utf8_lossy(&line).into_owned();
                    line.clear();
                    if text.contains("INFO: Texture") {
                        let mut s = state.lock().unwrap();
                        if s.phase == "starting" {
                            s.phase = "running";
                        }
                    }
                    if let Some(text) = clean_line(&text, &serial).filter(|s| !s.is_empty()) {
                        let level = if text.contains("ERROR:") {
                            "ERROR"
                        } else if text.contains("WARN:") {
                            "WARN"
                        } else {
                            "INFO"
                        };
                        crate::diagnostics::record(level, &format!("[{sid}] scrcpy: {text}"));
                    }
                } else if line.len() < 4096 {
                    line.push(b);
                }
            }
        }
    });
}
fn parent_death(command: &mut Command) {
    let parent = unsafe { libc::getpid() };
    // SAFETY: this child hook uses only async-signal-safe Linux calls before exec.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                libc::_exit(125);
            }
            Ok(())
        });
    }
}
fn kill_command(child: &mut Child) {
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn terminate(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    // Only our own child/process group is signalled; unrelated scrcpy instances are untouched.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGINT);
    }
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
/// An optional Android helper scoped to this source's scrcpy virtual capture. Its stdin
/// stays open only while the owned ADB process lives. No physical display power is changed.
struct CapturePower {
    adb: PathBuf,
    serial: String,
    remote: String,
    child: Option<Child>,
}
impl CapturePower {
    fn start(
        spec: &Spec,
        programs: &Programs,
        serial: &str,
        cancel: &AtomicBool,
    ) -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let suffix = format!(
            "{}-{}-{}",
            std::process::id(),
            spec.session_id,
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let local = std::env::temp_dir().join(format!("smartconnect-display-power-{suffix}.dex"));
        let remote = format!("/data/local/tmp/smartconnect-display-power-{suffix}.dex");
        let mut guard = Self {
            adb: programs.adb.clone(),
            serial: serial.into(),
            remote,
            child: None,
        };
        let result: Result<(), String> = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&local)
                .map_err(|_| "Could not prepare capture power helper")?;
            file.write_all(include_bytes!("../android/display-power.dex"))
                .map_err(|_| "Could not write capture power helper")?;
            capture(
                &guard.adb,
                &[
                    "-s".into(),
                    serial.into(),
                    "push".into(),
                    local.to_string_lossy().into(),
                    guard.remote.clone(),
                ],
                cancel,
            )?;
            if cancel.load(Ordering::Acquire) {
                return Err("Capture preparation cancelled".into());
            }
            let mut command = Command::new(&guard.adb);
            command
                .args([
                    "-s",
                    serial,
                    "shell",
                    &format!("CLASSPATH={}", guard.remote),
                    "app_process",
                    "/",
                    "org.smartconnect.DisplayPower",
                    &spec.display_id.to_string(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .process_group(0);
            parent_death(&mut command);
            let mut child = command
                .spawn()
                .map_err(|_| "Could not start capture power helper")?;
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            if let Some(output) = child.stdout.take() {
                let sid = spec.session_id;
                std::thread::spawn(move || {
                    for line in BufReader::new(output).lines().map_while(Result::ok) {
                        if line == "capture power: ready" {
                            let _ = ready_tx.try_send(());
                        }
                        // Helper output consists only of bounded static diagnostics and display IDs.
                        crate::diagnostics::record(
                            "INFO",
                            &format!(
                                "[{sid}] scrcpy: {}",
                                line.chars().take(256).collect::<String>()
                            ),
                        );
                    }
                });
            }
            guard.child = Some(child);
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if cancel.load(Ordering::Acquire) {
                    return Err("Capture preparation cancelled".into());
                }
                match ready_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(()) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err("Capture power helper unavailable".into());
                    }
                    _ if Instant::now() >= deadline => {
                        return Err("Capture power helper timed out".into());
                    }
                    _ => {}
                }
            }
            Ok(())
        })();
        let _ = std::fs::remove_file(local);
        result?;
        Ok(guard)
    }
}
impl Drop for CapturePower {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            terminate(&mut child);
        }
        let _ = capture(
            &self.adb,
            &[
                "-s".into(),
                self.serial.clone(),
                "shell".into(),
                "rm".into(),
                "-f".into(),
                self.remote.clone(),
            ],
            &AtomicBool::new(false),
        );
    }
}

fn run(
    spec: &Spec,
    programs: &Programs,
    cancel: &AtomicBool,
    state: &Arc<Mutex<State>>,
    text_socket: Option<&std::os::unix::net::UnixDatagram>,
) -> Result<(), String> {
    let version = capture(&programs.scrcpy, &["--version".into()], cancel)?;
    let major = version
        .split_whitespace()
        .find_map(|word| word.split('.').next()?.parse::<u32>().ok())
        .unwrap_or(0);
    if major < 3 {
        return Err("scrcpy 3.0 or later is required".into());
    }
    if spec.options.computer_ime && (major<5 || !crate::computer_ime::available()) {return Err("Computer input requires scrcpy 5+ and an SDL3-enabled Smart Connect build".into());}
    let supports_hwdec = if spec.options.software_decode {
        let help = capture(&programs.scrcpy, &["--help".into()], cancel)?;
        if !help.contains("--hwdec") {
            return Err("This scrcpy version has no decoder override; disable software decoding or update scrcpy".into());
        }
        true
    } else {
        false
    };
    let serial = connect_device(
        &spec.options,
        spec.phone_address.as_deref(),
        programs,
        cancel,
    )?;
    let sdk = if spec.options.fold_mouse || spec.options.phone_ime || (spec.options.audio && spec.options.independent_audio) {
        capture(
            &programs.adb,
            &[
                "-s".into(),
                serial.clone(),
                "shell".into(),
                "getprop".into(),
                "ro.build.version.sdk".into(),
            ],
            cancel,
        )
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    } else {
        None
    };
    validate_audio(&spec.options, sdk)?;
    let uhid_mouse = use_bound_input(spec.options.fold_mouse, spec.display_id, sdk, major);
    let uhid_keyboard = use_bound_input(spec.options.phone_ime && !spec.options.computer_ime, spec.display_id, sdk, major);
    crate::diagnostics::record(
        "INFO",
        &format!(
            "[{}] scrcpy: mouse={} speed={}%; keyboard={} audio={} independentAudio={} sdk={sdk:?}",
            spec.session_id,
            if uhid_mouse { "bound-uhid" } else { "sdk" },
            spec.options.mouse_speed_percent,
            if spec.options.computer_ime {"computer-ime"} else if uhid_keyboard { "bound-uhid-phone-ime" } else { "sdk" },
            spec.options.audio, spec.options.independent_audio
        ),
    );
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    let _capture_power = if programs.capture_power {
        match CapturePower::start(spec, programs, &serial, cancel) {
            Ok(helper) => Some(helper),
            Err(_) => {
                crate::diagnostics::record(
                    "WARN",
                    &format!(
                        "[{}] scrcpy: capture power helper unavailable",
                        spec.session_id
                    ),
                );
                None
            }
        }
    } else {
        None
    };
    if cancel.load(Ordering::Acquire) {
        return Ok(());
    }
    let (master, slave) = terminal()?;
    let mut command = Command::new(&programs.scrcpy);
    command
        .args(arguments(spec, &serial, supports_hwdec, uhid_mouse, uhid_keyboard))
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            slave
                .try_clone()
                .map_err(|_| "Could not create the scrcpy output")?,
        ))
        .stderr(Stdio::from(slave))
        .process_group(0);
    configure_input(&mut command, &spec.options, uhid_mouse, uhid_keyboard);
    if programs.x11 {
        command.env("SDL_VIDEODRIVER", "x11");
    }
    let _input_adapter=if let Some(socket)=text_socket {Some(crate::computer_ime::Adapter::attach(&mut command,socket)?)} else {None};
    parent_death(&mut command);
    let mut child = command.spawn().map_err(|_| "Could not start scrcpy")?;
    {
        let mut s = state.lock().unwrap();
        s.phase = "starting";
        s.pid = Some(child.id());
    }
    crate::diagnostics::record(
        "INFO",
        &format!(
            "[{}] scrcpy: process started pid={} display={} bitrateKbps={:?} fps={:?} borderless={} softwareDecode={}",
            spec.session_id,
            child.id(),
            spec.display_id,
            spec.quality.bitrate_kbps,
            spec.quality.fps,
            spec.borderless,
            spec.options.software_decode
        ),
    );
    log_reader(master, serial.clone(), spec.session_id, state.clone());
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if cancel.load(Ordering::Acquire) {
            terminate(&mut child);
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "scrcpy exited unexpectedly ({status}); see mirroring logs"
                    ))
                };
            }
            Err(_) => {
                terminate(&mut child);
                return Err("Could not monitor scrcpy".into());
            }
            Ok(None) => {}
        }
        if Instant::now() >= deadline && state.lock().unwrap().phase != "running" {
            terminate(&mut child);
            return Err(
                "scrcpy did not display video in time; check ADB and mirroring logs".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "smartconnect-scrcpy-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn script(&self, name: &str, text: &str) -> PathBuf {
            let p = self.0.join(name);
            std::fs::write(&p, text).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
            p
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn spec() -> Spec {
        Spec {
            display_id: 9,
            phone_address: None,
            options: Default::default(),
            quality: Quality {
                bitrate_kbps: Some(2000),
                fps: Some(30),
            },
            borderless: true,
            session_id: 1,
            fullscreen: false,
            window_size: None,
            title: "Smart Connect test".into(),
        }
    }
    fn wait_for(link: &Link, phase: &str) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if link.state().phase == phase {
                return;
            }
            if link.state().phase == "failed" && phase != "failed" {
                panic!("unexpected worker failure: {:?}", link.state());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("phase {:?}, expected {phase}", link.state());
    }
    #[test]
    fn wake_runs_before_lock_verification_without_a_scrcpy_executable() {
        let temp = Temp::new();
        let log = temp.0.join("order");
        let adb = temp.script(
            "adb",
            &format!(
                r#"#!/bin/sh
if [ "$1" = devices ]; then printf 'List of devices attached\nunit-device\tdevice\n'; exit 0; fi
printf '%s\n' "$*" >> '{}'
case "$*" in *'dumpsys window policy') echo 'mIsShowing=false';; esac
"#,
                log.display()
            ),
        );
        let programs = Programs {
            adb,
            scrcpy: temp.0.join("missing-scrcpy"),
            x11: false,
            capture_power: false,
        };
        assert_eq!(
            prepare_phone(
                &Default::default(),
                None,
                &programs,
                &AtomicBool::new(false)
            ),
            Ok(())
        );
        let order = std::fs::read_to_string(log).unwrap();
        let lines: Vec<_> = order.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with("input keyevent KEYCODE_WAKEUP"));
        assert!(lines[1].ends_with("wm dismiss-keyguard"));
        assert!(lines[2].ends_with("dumpsys window policy"));
    }

    #[test]
    fn secure_keyguard_is_reported_without_injecting_a_password() {
        let temp = Temp::new();
        let adb = temp.script(
            "adb",
            r#"#!/bin/sh
if [ "$1" = devices ]; then printf 'List of devices attached\nunit-device\tdevice\n'; exit 0; fi
case "$*" in *'dumpsys window policy') echo 'mIsShowing=true';; esac
"#,
        );
        let programs = Programs {
            adb,
            scrcpy: temp.0.join("missing"),
            x11: false,
            capture_power: false,
        };
        assert_eq!(
            prepare_phone(
                &Default::default(),
                None,
                &programs,
                &AtomicBool::new(false)
            ),
            Err("Unlock the phone on the device to continue".into())
        );
        assert_eq!(keyguard_showing("unrecognized policy"), None);
    }

    #[test]
    fn dropping_wake_preparation_interrupts_the_owned_adb_check() {
        let temp = Temp::new();
        let marker = temp.0.join("checking");
        let adb = temp.script(
            "adb",
            &format!("#!/bin/sh\ntouch '{}'\nexec sleep 10\n", marker.display()),
        );
        let task = WakePreparation::with_programs(
            Default::default(),
            None,
            Programs {
                adb,
                scrcpy: temp.0.join("missing"),
                x11: false,
                capture_power: false,
            },
        );
        let result = task.result.clone();
        let until = Instant::now() + Duration::from_secs(2);
        while !marker.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(marker.exists());
        drop(task);
        while result.lock().unwrap().is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(result.lock().unwrap().as_ref().is_some_and(Result::is_err));
    }

    #[test]
    fn device_selection_handles_peer_priority_authorization_and_ambiguity() {
        let list = devices(
            "List of devices attached\nunit-one\tdevice\n192.168.1.20:5555\tdevice\nunit-locked\tunauthorized\n",
        );
        assert_eq!(
            select_device(&list, "", Some("192.168.1.20".parse().unwrap()))
                .unwrap()
                .as_deref(),
            Some("192.168.1.20:5555")
        );
        assert!(select_device(&list, "", None).is_err());
        assert!(
            select_device(&list, "unit-locked", None)
                .unwrap_err()
                .contains("Authorize")
        );
        assert_eq!(
            select_device(&list, "unit-one", None).unwrap().as_deref(),
            Some("unit-one")
        );
        assert_eq!(select_device(&[], "", None).unwrap(), None);
    }
    #[test]
    fn folded_mouse_requires_a_source_bound_uhid_implementation() {
        assert!(use_bound_input(true, 9, Some(35), 4));
        assert!(use_bound_input(true, 9, Some(36), 5));
        for (enabled, display, sdk, major) in [
            (false, 9, Some(36), 5),
            (true, 0, Some(36), 5),
            (true, 9, Some(34), 5),
            (true, 9, Some(36), 3),
            (true, 9, None, 5),
        ] {
            assert!(!use_bound_input(enabled, display, sdk, major));
        }
        let args = arguments(&spec(), "unit-device", false, true, false);
        assert!(args.contains(&"--mouse=uhid".into()));
        assert!(args.contains(&"--keyboard=sdk".into()));
        assert!(!args.contains(&"--mouse=sdk".into()));
        assert!(arguments(&spec(), "unit-device", false, false, false).contains(&"--mouse=sdk".into()));
    }

    #[test]
    fn phone_ime_and_mouse_speed_are_independent_and_child_scoped() {
        let args=arguments(&spec(), "unit-device",false,false,true);
        let mut computer=spec();computer.options.computer_ime=true;
        let computer_args=arguments(&computer,"unit-device",false,true,false);
        assert!(computer_args.contains(&"--prefer-text".into()));
        assert!(computer_args.contains(&"--keyboard=sdk".into()));
        assert!(args.contains(&"--keyboard=uhid".into()));
        assert!(args.contains(&"--mouse=sdk".into()));
        assert!(!args.contains(&"--keyboard=sdk".into()));
        let options=ScrcpyOptions {mouse_speed_percent:175,..Default::default()};
        let mut command=Command::new("unused-test-command");
        configure_input(&mut command,&options,true,true);
        assert_eq!(command.get_envs().find(|(k,_)|*k=="SDL_MOUSE_RELATIVE_SPEED_SCALE").unwrap().1.unwrap(),"1.75");
        assert!(command.get_envs().any(|(k,v)|k=="SDL_IM_MODULE" && v.unwrap()=="none"));
        assert!(command.get_envs().any(|(k,v)|k=="XMODIFIERS" && v.unwrap()=="@im=none"));
        let mut sdk_command=Command::new("unused-test-command");
        configure_input(&mut sdk_command,&options,false,false);
        assert!(sdk_command.get_envs().next().is_none());
    }

    #[test]
    fn independent_audio_uses_playback_without_changing_phone_volume_or_duplication() {
        let mut s=spec();s.options.independent_audio=true;
        let args=arguments(&s,"unit-device",false,true,true);
        assert!(args.contains(&"--audio-source=playback".into()));
        for unwanted in ["--audio-source=output","--no-audio","--audio-dup","--require-audio"] {assert!(!args.contains(&unwanted.into()));}
        assert!(validate_audio(&s.options,Some(33)).is_ok());
        assert!(validate_audio(&s.options,Some(36)).is_ok());
        assert!(validate_audio(&s.options,Some(32)).is_err());
        assert!(validate_audio(&s.options,None).is_err());
        s.options.audio=false;
        assert!(arguments(&s,"unit-device",false,true,true).contains(&"--no-audio".into()));
        assert!(validate_audio(&s.options,None).is_ok());
    }

    #[test]
    fn audio_defaults_to_phone_output_and_can_be_disabled_without_changing_input() {
        let s=spec();let enabled=arguments(&s,"unit-device",false,true,true);
        assert!(enabled.contains(&"--audio-source=output".into()));
        assert!(!enabled.contains(&"--no-audio".into()));
        assert!(!enabled.contains(&"--require-audio".into()));
        assert!(!enabled.iter().any(|arg|arg.contains("mic")));
        let mut silent=s;silent.options.audio=false;
        let disabled=arguments(&silent,"unit-device",false,true,true);
        assert!(disabled.contains(&"--no-audio".into()));
        assert!(!disabled.iter().any(|arg|arg.starts_with("--audio-source=")));
        for arg in ["--mouse=uhid","--keyboard=uhid","--display-id=9"] {assert!(disabled.contains(&arg.into()));}
    }

    #[test]
    fn arguments_capture_the_existing_display_and_forward_encoder_settings() {
        let args = arguments(&spec(), "unit-device", true, false, false);
        assert!(args.contains(&"--display-id=9".into()));
        assert!(!args.iter().any(|s| s.contains("--new-display")));
        assert!(args.contains(&"--video-bit-rate=2000000".into()));
        assert!(args.contains(&"--max-fps=30".into()));
        assert!(args.contains(&"--window-borderless".into()));
        let mut s = spec();
        s.fullscreen = true;
        s.window_size = Some((1080, 1920));
        assert!(arguments(&s, "unit-device", true, false, false).contains(&"--fullscreen".into()));
        assert!(
            !arguments(&s, "unit-device", true, false, false)
                .iter()
                .any(|a| a.starts_with("--window-width"))
        );
        s.fullscreen = false;
        assert!(arguments(&s, "unit-device", true, false, false).contains(&"--window-width=1080".into()));
        assert!(arguments(&s, "unit-device", true, false, false).contains(&"--window-height=1920".into()));
        let mut s = spec();
        s.options.software_decode = true;
        assert!(arguments(&s, "unit-device", true, false, false).contains(&"--hwdec=disabled".into()));
        assert!(
            !arguments(&s, "unit-device", false, false, false)
                .iter()
                .any(|s| s.starts_with("--hwdec"))
        );
        assert!(
            !clean_line("selected private-unit-serial", "private-unit-serial")
                .unwrap()
                .contains("private-unit-serial")
        );
        assert!(clean_line("INFO: (usb) other-device", "private-unit-serial").is_none());
        assert_eq!(
            clean_line(
                "\x1b]0;scrcpy title\x07\x1b[33mWARN: message\x1b[0m",
                "unit-device"
            )
            .unwrap(),
            "WARN: message"
        );
    }
    #[test]
    fn worker_detects_video_and_reaps_only_its_own_child_on_stop() {
        let temp = Temp::new();
        let adb = temp.script(
            "adb",
            "#!/bin/sh\nprintf 'List of devices attached\\nunit-device\\tdevice\\n'\n",
        );
        let scrcpy=temp.script("scrcpy","#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'scrcpy 5.0.1'; exit; fi\ntrap 'exit 0' INT TERM\necho 'INFO: Texture: 1280x720'\nwhile :; do sleep 1; done\n");
        let mut other = Command::new("sleep").arg("30").spawn().unwrap();
        let link = Link::with_programs(
            spec(),
            Programs {
                adb,
                scrcpy,
                x11: false,
                capture_power: false,
            },
        );
        wait_for(&link, "running");
        let pid = link.state().pid.unwrap();
        assert!(unsafe { libc::kill(pid as i32, 0) } == 0);
        link.stop();
        wait_for(&link, "stopped");
        assert!(unsafe { libc::kill(pid as i32, 0) } != 0);
        assert!(other.try_wait().unwrap().is_none());
        let _ = other.kill();
        let _ = other.wait();
    }
    #[test]
    fn cancellation_during_adb_preparation_never_spawns_a_viewer() {
        let temp = Temp::new();
        let marker = temp.0.join("checking");
        let adb = temp.script(
            "adb",
            &format!("#!/bin/sh\ntouch '{}'\nexec sleep 10\n", marker.display()),
        );
        let scrcpy = temp.script("scrcpy", "#!/bin/sh\necho 'scrcpy 5.0.1'\n");
        let link = Link::with_programs(
            spec(),
            Programs {
                adb,
                scrcpy,
                x11: false,
                capture_power: false,
            },
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(marker.exists());
        link.stop();
        wait_for(&link, "stopped");
        assert!(link.state().pid.is_none());
    }
    #[test]
    fn missing_scrcpy_is_reported_without_starting_adb() {
        let temp = Temp::new();
        let missing = temp.0.join("missing");
        let link = Link::with_programs(
            spec(),
            Programs {
                adb: missing.clone(),
                scrcpy: missing,
                x11: false,
                capture_power: false,
            },
        );
        wait_for(&link, "failed");
        assert!(link.state().error.unwrap().contains("Could not run"));
        assert!(link.state().pid.is_none());
    }
}
