//! The server's own messages: printed, kept in memory (the last 500) and appended to `server.log`
//! in the data folder, so the web app can show them and they survive a restart.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LINES: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());
static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
const KEEP: usize = 500;
/// When the file grows past this it becomes `server.log.1` (the one before is dropped).
const MAX_BYTES: u64 = 1_000_000;

/// Start writing to `<dir>/server.log` and load the last lines of the previous run.
pub fn init(dir: &std::path::Path) {
    let path = dir.join("server.log");
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(mut l) = LINES.lock() {
            for line in text.lines() {
                if let Some((t, msg)) = line.split_once('\t') {
                    if let Ok(t) = t.parse::<u64>() {
                        if l.len() >= KEEP { l.pop_front(); }
                        l.push_back((t, msg.to_string()));
                    }
                }
            }
        }
    }
    if let Ok(mut f) = FILE.lock() { *f = Some(path); }
}

fn append(path: &std::path::Path, t: u64, msg: &str) {
    if std::fs::metadata(path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{t}\t{}", msg.replace(['\r', '\n'], " | "));
    }
}

/// Print a message on the console and remember it for Settings → Manage → Server log.
pub fn add(msg: String) {
    eprintln!("{msg}");
    let t = crate::db::now();
    if let Ok(mut l) = LINES.lock() {
        if l.len() >= KEEP { l.pop_front(); }
        l.push_back((t, msg.clone()));
    }
    let path = FILE.lock().ok().and_then(|f| f.clone());
    if let Some(p) = path { append(&p, t, &msg); }
}

/// The remembered messages, oldest first, as (unix seconds, text).
pub fn lines() -> Vec<(u64, String)> {
    LINES.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default()
}

macro_rules! elog { ($($a:tt)*) => { crate::log::add(format!($($a)*)) }; }

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_the_latest_lines() {
        for i in 0..(super::KEEP + 20) { super::add(format!("line {i}")); }
        let l = super::lines();
        assert!(l.len() <= super::KEEP);
        assert_eq!(l.last().unwrap().1, format!("line {}", super::KEEP + 19));
    }

    #[test]
    fn file_line_is_one_line() {
        let p = std::env::temp_dir().join(format!("lichtspiel-log-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&p);
        super::append(&p, 5, "a\nb");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "5\ta | b\n");
        let _ = std::fs::remove_file(&p);
    }
}
