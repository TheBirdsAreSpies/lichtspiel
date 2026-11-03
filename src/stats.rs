//! What the dashboard shows about the machine: processor and memory use (of
//! the whole computer and of the server itself), the bytes sent to players,
//! and who used the server lately. A background thread takes a sample every
//! few seconds and keeps the last hour; averages per minute (one day) and per
//! hour (for good) are kept in stats.json in the data folder.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Seconds between two samples.
pub const SAMPLE_SECONDS: u64 = 5;
/// Samples kept (one hour).
pub const SAMPLES_KEPT: usize = 720;
/// Minute averages kept (one day).
pub const MINUTES_KEPT: usize = 1440;
/// Hour averages kept (twenty years).
pub const HOURS_KEPT: usize = 24 * 366 * 20;

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sample {
    /// Unix seconds.
    pub at: u64,
    /// Processor use of the whole computer, 0–100 (None = not known here).
    pub cpu: Option<f32>,
    /// Processor use of this server, 0–100 of the whole computer.
    pub cpu_server: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    /// Memory of this server (resident set / working set).
    pub mem_server: Option<u64>,
    /// Bytes per second sent since the last sample.
    pub out_bps: u64,
    /// The part of it sent outside the local network.
    #[serde(default)]
    pub out_remote_bps: u64,
    pub streams: usize,
    pub transcodes: usize,
}

/// Someone who sent a signed-in request lately.
#[derive(Clone, Debug)]
pub struct Seen {
    pub at: u64,
    pub ip: String,
    pub agent: String,
}

#[derive(Default)]
pub struct Stats {
    /// Every byte of every response body since the start.
    pub bytes_out: Arc<AtomicU64>,
    /// The bytes of them sent outside the local network.
    pub bytes_remote: Arc<AtomicU64>,
    pub long: Mutex<LongTerm>,
    /// The same per user id.
    pub user_bytes: Mutex<HashMap<u64, Arc<AtomicU64>>>,
    pub samples: Mutex<VecDeque<Sample>>,
    pub seen: Mutex<HashMap<u64, Seen>>,
    /// Bytes per second per user over the last sample.
    pub user_bps: Mutex<HashMap<u64, u64>>,
}

impl Stats {
    pub fn counter_for(&self, user: Option<u64>) -> Option<Arc<AtomicU64>> {
        let uid = user?;
        Some(self.user_bytes.lock().unwrap().entry(uid).or_default().clone())
    }

    pub fn saw(&self, user: u64, ip: String, agent: &str) {
        let agent = short_agent(agent);
        self.seen.lock().unwrap().insert(user, Seen { at: crate::db::now(), ip, agent });
    }

    pub fn push(&self, s: Sample) {
        let mut q = self.samples.lock().unwrap();
        self.long.lock().unwrap().roll(s.at, &q);
        q.push_back(s);
        while q.len() > SAMPLES_KEPT {
            q.pop_front();
        }
    }

    pub fn last(&self) -> Option<Sample> {
        self.samples.lock().unwrap().back().copied()
    }
}

/// "Firefox on Windows" from a browser's User-Agent text.
pub fn short_agent(ua: &str) -> String {
    if ua.is_empty() {
        return String::new();
    }
    let browser = if ua.contains("Edg/") {
        "Edge"
    } else if ua.contains("OPR/") {
        "Opera"
    } else if ua.contains("Firefox/") {
        "Firefox"
    } else if ua.contains("Chrome/") {
        "Chrome"
    } else if ua.contains("Safari/") {
        "Safari"
    } else if ua.contains("curl/") {
        "curl"
    } else {
        ""
    };
    let os = if ua.contains("Android") {
        "Android"
    } else if ua.contains("iPhone") || ua.contains("iPad") {
        "iOS"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Mac OS") {
        "macOS"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        ""
    };
    match (browser, os) {
        ("", "") => ua.chars().take(40).collect(),
        (b, "") => b.into(),
        ("", o) => o.into(),
        (b, o) => format!("{b} · {o}"),
    }
}

/// A response body that counts what is read from it (= sent).
pub struct Counting {
    pub inner: Box<dyn Read + Send>,
    pub all: Arc<AtomicU64>,
    pub user: Option<Arc<AtomicU64>>,
    /// Set when the caller is outside the local network.
    pub remote: Option<Arc<AtomicU64>>,
}

impl Read for Counting {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.all.fetch_add(n as u64, Ordering::Relaxed);
        if let Some(u) = &self.user {
            u.fetch_add(n as u64, Ordering::Relaxed);
        }
        if let Some(r) = &self.remote {
            r.fetch_add(n as u64, Ordering::Relaxed);
        }
        Ok(n)
    }
}

/// The mean of several samples (streams: the most at once), stamped `at`.
pub fn average(at: u64, list: &[Sample]) -> Sample {
    let mean_f = |f: fn(&Sample) -> Option<f32>| {
        let v: Vec<f32> = list.iter().filter_map(f).collect();
        if v.is_empty() { None } else { Some(v.iter().sum::<f32>() / v.len() as f32) }
    };
    let mean_u = |f: fn(&Sample) -> Option<u64>| {
        let v: Vec<u64> = list.iter().filter_map(f).collect();
        if v.is_empty() { None } else { Some(v.iter().sum::<u64>() / v.len() as u64) }
    };
    let n = list.len().max(1) as u64;
    Sample {
        at,
        cpu: mean_f(|s| s.cpu),
        cpu_server: mean_f(|s| s.cpu_server),
        mem_used: mean_u(|s| s.mem_used),
        mem_total: list.iter().rev().find_map(|s| s.mem_total),
        mem_server: mean_u(|s| s.mem_server),
        out_bps: list.iter().map(|s| s.out_bps).sum::<u64>() / n,
        out_remote_bps: list.iter().map(|s| s.out_remote_bps).sum::<u64>() / n,
        streams: list.iter().map(|s| s.streams).max().unwrap_or(0),
        transcodes: list.iter().map(|s| s.transcodes).max().unwrap_or(0),
    }
}

/// Averages per minute (the last day) and per hour (for good), so the dashboard
/// can show the last 12 hours up to everything since the first start.
#[derive(Default, Serialize, Deserialize)]
pub struct LongTerm {
    pub minutes: VecDeque<Sample>,
    pub hours: VecDeque<Sample>,
    /// The minute and hour being collected (unix time / 60, / 3600).
    #[serde(skip)]
    minute: u64,
    #[serde(skip)]
    hour: u64,
}

impl LongTerm {
    /// A new sample at `at` is coming: a minute (or an hour) that is over is averaged.
    fn roll(&mut self, at: u64, recent: &VecDeque<Sample>) {
        let m = at / 60;
        if self.minute == 0 { self.minute = m; }
        if self.hour == 0 { self.hour = m / 60; }
        if m == self.minute { return; }
        let from = self.minute * 60;
        let part: Vec<Sample> = recent.iter().filter(|s| s.at >= from && s.at < from + 60).copied().collect();
        if !part.is_empty() { self.minutes.push_back(average(from, &part)); }
        while self.minutes.len() > MINUTES_KEPT { self.minutes.pop_front(); }
        self.minute = m;
        if m / 60 != self.hour {
            let hf = self.hour * 3600;
            let part: Vec<Sample> = self.minutes.iter().filter(|s| s.at >= hf && s.at < hf + 3600).copied().collect();
            if !part.is_empty() && self.hours.back().is_none_or(|h| h.at < hf) { self.hours.push_back(average(hf, &part)); }
            while self.hours.len() > HOURS_KEPT { self.hours.pop_front(); }
            self.hour = m / 60;
        }
    }
}

impl Stats {
    pub fn load(&self, path: &std::path::Path) {
        let Ok(text) = std::fs::read_to_string(path) else { return };
        match serde_json::from_str::<LongTerm>(&text) {
            Ok(l) => *self.long.lock().unwrap() = l,
            Err(e) => elog!("{} can't be read ({e}); the dashboard starts its history again", path.display()),
        }
    }

    pub fn save(&self, path: &std::path::Path) {
        let text = { serde_json::to_string(&*self.long.lock().unwrap()).unwrap_or_default() };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Values for a time range, one per step on an even grid (None where the
    /// server wasn't running): "realtime" (15 minutes), 12h, 24h, 7d, 30d, 90d, 365d, all.
    pub fn range(&self, range: &str, now: u64) -> (u64, Vec<(u64, Option<Sample>)>) {
        let recent = self.samples.lock().unwrap();
        let long = self.long.lock().unwrap();
        // The hour being collected isn't in `hours` yet: its minutes stand in for it.
        let hours_now = || -> Vec<Sample> {
            let mut v: Vec<Sample> = long.hours.iter().copied().collect();
            let hf = long.hour * 3600;
            let part: Vec<Sample> = long.minutes.iter().filter(|s| s.at >= hf).copied().collect();
            if !part.is_empty() && v.last().is_none_or(|h| h.at < hf) { v.push(average(hf, &part)); }
            v
        };
        let minutes_now = || -> Vec<Sample> {
            let mut v: Vec<Sample> = long.minutes.iter().copied().collect();
            let mf = long.minute * 60;
            let part: Vec<Sample> = recent.iter().filter(|s| s.at >= mf).copied().collect();
            if !part.is_empty() && v.last().is_none_or(|h| h.at < mf) { v.push(average(mf, &part)); }
            v
        };
        let (span, step, source): (u64, u64, Vec<Sample>) = match range {
            "12h" => (12 * 3600, 120, minutes_now()),
            "24h" => (24 * 3600, 240, minutes_now()),
            "7d" => (7 * 86400, 3600, hours_now()),
            "30d" => (30 * 86400, 2 * 3600, hours_now()),
            "90d" => (90 * 86400, 6 * 3600, hours_now()),
            "365d" => (365 * 86400, 86400, hours_now()),
            "all" => {
                let h = hours_now();
                let first = h.first().map(|s| s.at).or_else(|| long.minutes.front().map(|s| s.at)).unwrap_or(now.saturating_sub(86400));
                let span = now.saturating_sub(first).max(86400);
                // About 400 values, in whole hours.
                let step = (span / 400).div_ceil(3600).max(1) * 3600;
                (span, step, h)
            }
            _ => (15 * 60, SAMPLE_SECONDS, recent.iter().copied().collect()),
        };
        let start = (now.saturating_sub(span) / step) * step;
        let mut out = vec![];
        let mut i = source.partition_point(|s| s.at < start);
        let mut t = start;
        while t <= now {
            let j = i + source[i..].partition_point(|s| s.at < t + step);
            out.push((t, if j > i { Some(average(t, &source[i..j])) } else { None }));
            i = j;
            t += step;
        }
        (step, out)
    }
}

// ---------------------------------------------------------------- processor and memory

/// Counters read from the system; two readings give the use in between.
#[derive(Clone, Copy, Default)]
pub struct Times {
    /// All processor time of the computer (every core), and the idle part.
    pub total: u64,
    pub idle: u64,
    /// Processor time of this process, in the same unit.
    pub process: u64,
}

pub struct Memory {
    pub used: u64,
    pub total: u64,
    pub process: u64,
}

/// Use between two readings: (computer %, server %).
pub fn cpu_between(a: Times, b: Times) -> Option<(f32, f32)> {
    let total = b.total.checked_sub(a.total)?;
    if total == 0 {
        return None;
    }
    let idle = b.idle.saturating_sub(a.idle).min(total);
    let process = b.process.saturating_sub(a.process).min(total);
    let pct = |x: u64| (x as f64 * 100.0 / total as f64) as f32;
    Some((pct(total - idle), pct(process)))
}

#[cfg(target_os = "linux")]
pub fn times() -> Option<Times> {
    // /proc/stat: "cpu user nice system idle iowait irq softirq steal …" (in ticks).
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if v.len() < 4 {
        return None;
    }
    let total: u64 = v.iter().take(8).sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    // /proc/self/stat: utime and stime are fields 14 and 15 (after the name in brackets).
    let me = std::fs::read_to_string("/proc/self/stat").ok()?;
    let rest = &me[me.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let process = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
    Some(Times { total, idle, process })
}

#[cfg(target_os = "linux")]
pub fn memory() -> Option<Memory> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = |key: &str| -> Option<u64> {
        info.lines().find(|l| l.starts_with(key))?.split_whitespace().nth(1)?.parse::<u64>().ok().map(|k| k * 1024)
    };
    let total = kb("MemTotal:")?;
    let avail = kb("MemAvailable:").or_else(|| kb("MemFree:"))?;
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let process = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|k| k.parse::<u64>().ok())
        .map(|k| k * 1024)
        .unwrap_or(0);
    Some(Memory { used: total.saturating_sub(avail), total, process })
}

#[cfg(windows)]
mod win {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct FileTime {
        pub lo: u32,
        pub hi: u32,
    }
    impl FileTime {
        pub fn get(self) -> u64 {
            ((self.hi as u64) << 32) | self.lo as u64
        }
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct MemoryStatusEx {
        pub length: u32,
        pub memory_load: u32,
        pub total_phys: u64,
        pub avail_phys: u64,
        pub total_page_file: u64,
        pub avail_page_file: u64,
        pub total_virtual: u64,
        pub avail_virtual: u64,
        pub avail_extended_virtual: u64,
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct ProcessMemoryCounters {
        pub cb: u32,
        pub page_fault_count: u32,
        pub peak_working_set_size: usize,
        pub working_set_size: usize,
        pub quota_peak_paged_pool_usage: usize,
        pub quota_paged_pool_usage: usize,
        pub quota_peak_non_paged_pool_usage: usize,
        pub quota_non_paged_pool_usage: usize,
        pub pagefile_usage: usize,
        pub peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        pub fn GlobalMemoryStatusEx(m: *mut MemoryStatusEx) -> i32;
        pub fn GetCurrentProcess() -> isize;
        pub fn GetProcessTimes(h: isize, created: *mut FileTime, exited: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        pub fn K32GetProcessMemoryInfo(h: isize, p: *mut ProcessMemoryCounters, cb: u32) -> i32;
    }
}

#[cfg(windows)]
pub fn times() -> Option<Times> {
    use win::*;
    let (mut idle, mut kernel, mut user) = (FileTime::default(), FileTime::default(), FileTime::default());
    let (mut c, mut e, mut pk, mut pu) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
    // SAFETY: plain out-parameters of the right size; the process handle is the pseudo handle.
    unsafe {
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) == 0 {
            return None;
        }
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut pk, &mut pu) == 0 {
            return None;
        }
    }
    // Kernel time includes the idle time.
    Some(Times { total: kernel.get() + user.get(), idle: idle.get(), process: pk.get() + pu.get() })
}

#[cfg(windows)]
pub fn memory() -> Option<Memory> {
    use win::*;
    let mut m = MemoryStatusEx { length: std::mem::size_of::<MemoryStatusEx>() as u32, ..Default::default() };
    let mut p = ProcessMemoryCounters { cb: std::mem::size_of::<ProcessMemoryCounters>() as u32, ..Default::default() };
    // SAFETY: the structs carry their own size as the functions require.
    unsafe {
        if GlobalMemoryStatusEx(&mut m) == 0 {
            return None;
        }
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut p, p.cb) == 0 {
            p.working_set_size = 0;
        }
    }
    Some(Memory { used: m.total_phys.saturating_sub(m.avail_phys), total: m.total_phys, process: p.working_set_size as u64 })
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn times() -> Option<Times> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn memory() -> Option<Memory> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_use_between_two_readings() {
        let a = Times { total: 1000, idle: 800, process: 10 };
        let b = Times { total: 2000, idle: 1550, process: 60 };
        let (all, me) = cpu_between(a, b).unwrap();
        assert!((all - 25.0).abs() < 0.01);
        assert!((me - 5.0).abs() < 0.01);
        assert!(cpu_between(b, a).is_none());
    }

    #[test]
    fn readings_work_here() {
        if cfg!(any(windows, target_os = "linux")) {
            assert!(times().is_some());
            let m = memory().unwrap();
            assert!(m.total > 0 && m.used <= m.total);
        }
    }

    #[test]
    fn minutes_and_hours_are_averaged() {
        let st = Stats::default();
        let t0 = 1_800_000_000 / 3600 * 3600; // on the hour
        for k in 0..(2 * 3600 / 5 + 13) {
            let at = t0 + k * 5;
            st.push(Sample { at, cpu: Some(if (at / 60) % 2 == 0 { 10.0 } else { 30.0 }), out_bps: 100, out_remote_bps: 40, streams: (k % 3) as usize, ..Default::default() });
        }
        let long = st.long.lock().unwrap();
        // Two whole hours and one minute more are done.
        assert_eq!(long.hours.len(), 2);
        assert_eq!(long.hours[0].at, t0);
        assert!((long.hours[0].cpu.unwrap() - 20.0).abs() < 0.01);
        assert_eq!(long.hours[0].out_remote_bps, 40);
        assert_eq!(long.hours[0].streams, 2);
        assert_eq!(long.minutes.back().unwrap().at, t0 + 7200);
        drop(long);
        let (step, v) = st.range("12h", t0 + 7200 + 65);
        assert_eq!(step, 120);
        assert!(v.first().unwrap().1.is_none(), "before the first start: no values");
        let filled = v.iter().filter(|(_, s)| s.is_some()).count();
        assert_eq!(filled, 61);
        let (_, v) = st.range("7d", t0 + 7200 + 65);
        assert_eq!(v.iter().filter(|(_, s)| s.is_some()).count(), 3, "two hours and the one being collected");
        let (step, _) = st.range("realtime", t0 + 7200 + 65);
        assert_eq!(step, SAMPLE_SECONDS);
    }

    #[test]
    fn agents() {
        assert_eq!(short_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36 Edg/130.0"), "Edge · Windows");
        assert_eq!(short_agent("Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0"), "Firefox · Linux");
        assert_eq!(short_agent("curl/8.4.0"), "curl");
    }
}
