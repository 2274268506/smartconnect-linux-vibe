//! Bounded asynchronous diagnostic writer: never wait for disk I/O on media/control threads.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static WRITER: OnceLock<mpsc::SyncSender<Entry>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);
enum Entry {
    Line(String),
    Flush(mpsc::Sender<()>),
}

pub fn init() {
    WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(256);
        std::thread::Builder::new()
            .name("mirror-log".into())
            .spawn(move || {
                let dir = crate::mirror_log::directory();
                let mut reported = false;
                while let Ok(entry) = rx.recv() {
                    match entry {
                        Entry::Line(line) => {
                            if let Err(error) = crate::mirror_log::append(&dir, &line) {
                                if !reported {
                                    std::eprintln!("mirror log unavailable: {error}");
                                    reported = true;
                                }
                            }
                        }
                        Entry::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .expect("starting log writer");
        tx
    });
    record(
        "INFO",
        &format!("mirror process started pid={}", std::process::id()),
    );
}

fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: initialized time value and a writable tm; gmtime_r is thread-safe.
    if unsafe { libc::gmtime_r(&seconds, tm.as_mut_ptr()) }.is_null() {
        return seconds.to_string();
    }
    let tm = unsafe { tm.assume_init() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

pub fn record(level: &str, message: &str) {
    let Some(writer) = WRITER.get() else { return };
    let dropped = DROPPED.swap(0, Ordering::Relaxed);
    let stamp = timestamp();
    let mut text = String::new();
    if dropped > 0 {
        text.push_str(&format!(
            "{stamp} [WARN] logging queue full: {dropped} entries skipped\n"
        ));
    }
    for line in message.lines().take(20) {
        text.push_str(&format!(
            "{stamp} [{level}] {}\n",
            crate::mirror_log::sanitize(line)
        ));
    }
    if writer.try_send(Entry::Line(text)).is_err() {
        DROPPED.fetch_add(dropped + 1, Ordering::Relaxed);
    }
}

pub fn flush() {
    if let Some(writer) = WRITER.get() {
        let (tx, rx) = mpsc::channel();
        if writer.try_send(Entry::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_millis(500));
        }
    }
}

pub fn attach_gstreamer() {
    if std::env::var_os("GST_DEBUG").is_none() {
        gst::log::set_default_threshold(gst::DebugLevel::Warning);
    }
    gst::log::add_log_function(|category, level, _, _, _, _, message| {
        if level <= gst::DebugLevel::Warning {
            if let Some(message) = message.get() {
                record(
                    if level == gst::DebugLevel::Error {
                        "ERROR"
                    } else {
                        "WARN"
                    },
                    &format!("GStreamer {}: {message}", category.name()),
                );
            }
        }
    });
}
