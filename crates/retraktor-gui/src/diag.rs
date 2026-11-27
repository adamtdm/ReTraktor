//! Freeze diagnostics. The UI thread names each step it takes ([`stage`]); a watchdog thread notices when frames
//! stop and logs that step plus the system call the UI thread is blocked in (and the file it names), to
//! `~/.config/retraktor/diagnostics.log` (the previous run's goes to `diagnostics.prev.log`). Frames over 250 ms
//! are logged with the steps that took the time.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
/// Milliseconds since start at the UI thread's last frame start or end.
static BEAT: AtomicU64 = AtomicU64::new(0);
static STAGE: Mutex<&'static str> = Mutex::new("startup");
/// `/proc/<pid>/task/<tid>` of the UI thread.
static UI_TASK: OnceLock<String> = OnceLock::new();
static LOG: Mutex<Option<File>> = Mutex::new(None);
const BETWEEN_FRAMES: &str = "between frames (egui layout/paint, or waiting for events)";

thread_local! {
    /// The current frame's steps so far, and the one running now.
    static FRAME: RefCell<(Vec<(&'static str, Duration)>, Option<(&'static str, Instant)>)> = const { RefCell::new((Vec::new(), None)) };
}

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn log(line: &str) {
    let line = format!("[{:>8.3}s] {line}", now_ms() as f64 / 1000.0);
    eprintln!("retraktor: {line}");
    if let Ok(mut f) = LOG.lock() {
        if let Some(f) = f.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Opens the log and starts the watchdog; call first thing in `main`, on the UI thread.
pub fn start() {
    now_ms();
    if let Ok(link) = fs::read_link("/proc/thread-self") {
        let _ = UI_TASK.set(format!("/proc/{}", link.display()));
    }
    let dir = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/retraktor");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::rename(dir.join("diagnostics.log"), dir.join("diagnostics.prev.log"));
    if let Ok(mut l) = LOG.lock() {
        *l = OpenOptions::new().create(true).write(true).truncate(true).open(dir.join("diagnostics.log")).ok();
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    log(&format!("ReTraktor {} started (unix time {secs})", env!("CARGO_PKG_VERSION")));
    let _ = std::thread::Builder::new().name("retraktor-watchdog".into()).spawn(watchdog);
}

/// Names what the UI thread does next, for the watchdog and the slow-frame log.
pub fn stage(name: &'static str) {
    let now = Instant::now();
    FRAME.with(|f| {
        let mut f = f.borrow_mut();
        if let Some((prev, t)) = f.1.replace((name, now)) {
            f.0.push((prev, now - t));
        }
    });
    if let Ok(mut s) = STAGE.lock() {
        *s = name;
    }
}

pub fn frame_start() {
    BEAT.store(now_ms(), Ordering::Relaxed);
    FRAME.with(|f| *f.borrow_mut() = (Vec::new(), None));
    stage("frame start");
}

pub fn frame_end() {
    stage(BETWEEN_FRAMES);
    BEAT.store(now_ms(), Ordering::Relaxed);
    let steps = FRAME.with(|f| std::mem::take(&mut f.borrow_mut().0));
    let total: Duration = steps.iter().map(|s| s.1).sum();
    if total >= Duration::from_millis(250) {
        let mut slow: Vec<_> = steps.into_iter().filter(|s| s.1 >= Duration::from_millis(20)).collect();
        slow.sort_by(|a, b| b.1.cmp(&a.1));
        let parts: Vec<String> = slow.iter().map(|(n, d)| format!("{n} {} ms", d.as_millis())).collect();
        log(&format!("slow frame {} ms: {}", total.as_millis(), parts.join(", ")));
    }
}

fn watchdog() {
    let mut stalled_since: Option<u64> = None;
    let mut next_report = 0;
    let mut tally: HashMap<String, u32> = HashMap::new();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let (now, beat) = (now_ms(), BEAT.load(Ordering::Relaxed));
        let stage = STAGE.lock().map_or("?", |s| *s);
        let call = UI_TASK.get().map(|t| ui_syscall(t)).unwrap_or_default();
        // Between frames with the UI thread waiting for input is just an idle or hidden window.
        let idle = stage == BETWEEN_FRAMES && ["poll", "ppoll", "epoll_wait", "epoll_pwait", "pselect6"].iter().any(|c| call.starts_with(c));
        if now.saturating_sub(beat) < 1000 || idle {
            if let Some(from) = stalled_since.take() {
                let mut t: Vec<_> = tally.drain().collect();
                t.sort_by(|a, b| b.1.cmp(&a.1));
                let top: Vec<String> = t.iter().take(8).map(|(k, n)| format!("{n}× {k}")).collect();
                log(&format!("UI resumed after a {:.1}s stall; samples every 0.2s: {}", (now - from) as f64 / 1000.0, top.join(" | ")));
            }
            continue;
        }
        let from = *stalled_since.get_or_insert(beat);
        let short = call.split(" file=").next().unwrap_or("").to_string();
        *tally.entry(format!("{stage} · {short}")).or_default() += 1;
        if now >= next_report {
            next_report = now + 2000;
            let wchan = UI_TASK.get().and_then(|t| fs::read_to_string(format!("{t}/wchan")).ok()).unwrap_or_default();
            log(&format!("UI stalled {:.1}s in: {stage} · syscall {call} · wchan {wchan}", (now - from) as f64 / 1000.0));
        }
    }
}

/// What the UI thread is in right now: a system call by name, with the file it names, or "running" when on CPU.
fn ui_syscall(task: &str) -> String {
    let Ok(text) = fs::read_to_string(format!("{task}/syscall")) else { return "unknown".into() };
    let f: Vec<&str> = text.split_whitespace().collect();
    let Some(nr) = f.first().and_then(|n| n.parse::<i64>().ok()) else { return text.trim().to_string() };
    let arg = |i: usize| f.get(i + 1).and_then(|a| u64::from_str_radix(a.trim_start_matches("0x"), 16).ok()).unwrap_or(0);
    let (name, path_arg, fd_arg) = syscall_name(nr);
    let file = match (path_arg, fd_arg) {
        (Some(i), _) => read_c_string(arg(i)),
        (_, true) => fs::read_link(format!("/proc/self/fd/{}", arg(0))).ok().map(|p| p.display().to_string()),
        _ => None,
    };
    match file {
        Some(p) => format!("{name} file={p}"),
        None => name,
    }
}

/// x86_64 numbers of the calls a stall is likely in: (name, which argument is a path, whether arg 0 is a file).
fn syscall_name(nr: i64) -> (String, Option<usize>, bool) {
    let (name, path, fd) = match nr {
        -1 => ("none (blocked in the kernel, not in a call)", None, false),
        0 => ("read", None, true),
        1 => ("write", None, true),
        2 => ("open", Some(0), false),
        3 => ("close", None, true),
        4 => ("stat", Some(0), false),
        5 => ("fstat", None, true),
        6 => ("lstat", Some(0), false),
        7 => ("poll", None, false),
        17 => ("pread64", None, true),
        18 => ("pwrite64", None, true),
        21 => ("access", Some(0), false),
        74 => ("fsync", None, true),
        78 => ("getdents", None, true),
        82 => ("rename", Some(0), false),
        89 => ("readlink", Some(0), false),
        202 => ("futex (waiting on a lock)", None, false),
        217 => ("getdents64", None, true),
        230 => ("clock_nanosleep", None, false),
        232 => ("epoll_wait", None, false),
        257 => ("openat", Some(1), false),
        262 => ("newfstatat", Some(1), false),
        270 => ("pselect6", None, false),
        271 => ("ppoll", None, false),
        281 => ("epoll_pwait", None, false),
        316 => ("renameat2", Some(1), false),
        332 => ("statx", Some(1), false),
        439 => ("faccessat2", Some(1), false),
        _ => return (format!("#{nr}"), None, false),
    };
    (name.to_string(), path.filter(|_| cfg!(target_arch = "x86_64")), fd)
}

/// Reads a path string out of this process's memory; the UI thread is blocked in the call using it, so it's live.
fn read_c_string(addr: u64) -> Option<String> {
    let mut buf = [0u8; 512];
    let n = File::open("/proc/self/mem").ok()?.read_at(&mut buf, addr).ok()?;
    let s = &buf[..n];
    let end = s.iter().position(|&b| b == 0).unwrap_or(n);
    Some(String::from_utf8_lossy(&s[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The watchdog's reading of a blocked thread: this thread's own syscall file while it's in a call is
    /// "running", and a path argument reads back out of process memory.
    #[test]
    fn reads_paths_and_names_calls() {
        let path = std::ffi::CString::new("/music/a track.mp3").unwrap();
        assert_eq!(read_c_string(path.as_ptr() as u64).as_deref(), Some("/music/a track.mp3"));
        assert_eq!(syscall_name(262).0, "newfstatat");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            tx.send(format!("/proc/{}", fs::read_link("/proc/thread-self").unwrap().display())).unwrap();
            std::thread::sleep(Duration::from_secs(2));
        });
        let task = rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(ui_syscall(&task), "clock_nanosleep");
    }
}
