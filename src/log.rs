//! The server's own messages, kept in memory (the last 500) as well as printed, so the web app can show them.

use std::collections::VecDeque;
use std::sync::Mutex;

static LINES: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());
const KEEP: usize = 500;

/// Print a message on the console and remember it for Settings → Manage → Server log.
pub fn add(msg: String) {
    eprintln!("{msg}");
    if let Ok(mut l) = LINES.lock() {
        if l.len() >= KEEP { l.pop_front(); }
        l.push_back((crate::db::now(), msg));
    }
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
}
