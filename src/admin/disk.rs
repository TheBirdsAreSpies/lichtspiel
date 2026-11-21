//! Free space on the disks that hold the libraries and the data folder, and `disk.low` for plugins
//! (push-notify sends it on) when one of them is nearly full.

use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// How often the disks are looked at. Asking for free space does not wake a sleeping disk.
const CHECK_MINUTES: u64 = 30;
/// A disk that was reported counts as full again only after it had this many points more free.
const MARGIN_PERCENT: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Space {
    pub free: u64,
    pub total: u64,
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast)]
pub fn space(path: &Path) -> Option<Space> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let unit = if s.f_frsize > 0 { s.f_frsize as u64 } else { s.f_bsize as u64 };
    let total = (s.f_blocks as u64).saturating_mul(unit);
    (total > 0).then(|| Space { free: (s.f_bavail as u64).saturating_mul(unit), total })
}

#[cfg(windows)]
pub fn space(path: &Path) -> Option<Space> {
    use std::os::windows::ffi::OsStrExt;
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) } == 0 || total == 0 {
        return None;
    }
    Some(Space { free: avail, total })
}

#[cfg(not(any(unix, windows)))]
pub fn space(_path: &Path) -> Option<Space> {
    None
}

/// One disk and what is on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Disk {
    /// Library names, and "data" for the server's data folder.
    pub holds: Vec<String>,
    pub paths: Vec<String>,
    pub space: Space,
}

impl Disk {
    pub fn free_percent(&self) -> f64 {
        self.space.free as f64 * 100.0 / self.space.total.max(1) as f64
    }
    /// The same disk from one check to the next.
    fn key(&self) -> String {
        self.paths.join("\n")
    }
    pub fn to_json(&self) -> Value {
        json!({"holds": self.holds, "paths": self.paths, "free_bytes": self.space.free, "total_bytes": self.space.total,
            "free_percent": (self.free_percent() * 10.0).round() / 10.0})
    }
}

/// Puts places (what, folder) on the disks they are on. Folders on one disk give the same numbers
/// when asked at the same moment, so equal numbers are one disk. Folders that can't be read are left out.
pub fn disks(places: &[(String, String)], space: impl Fn(&Path) -> Option<Space>) -> Vec<Disk> {
    let mut out: Vec<Disk> = vec![];
    for (what, path) in places {
        let Some(s) = space(Path::new(path)) else { continue };
        match out.iter_mut().find(|d| d.space == s) {
            Some(d) => {
                if !d.holds.contains(what) { d.holds.push(what.clone()); }
                if !d.paths.contains(path) { d.paths.push(path.clone()); }
            }
            None => out.push(Disk { holds: vec![what.clone()], paths: vec![path.clone()], space: s }),
        }
    }
    out
}

/// The disks to tell about now (below `percent` free and not told yet), and forgets the ones
/// that have room again, so they are told about once more when they fill up the next time.
pub fn to_report<'a>(disks: &'a [Disk], percent: u32, told: &mut HashSet<String>) -> Vec<&'a Disk> {
    if percent == 0 {
        told.clear();
        return vec![];
    }
    let limit = percent as f64;
    let mut out = vec![];
    for d in disks {
        let p = d.free_percent();
        if p < limit {
            if told.insert(d.key()) { out.push(d); }
        } else if p >= limit + MARGIN_PERCENT {
            told.remove(&d.key());
        }
    }
    out
}

/// The libraries' folders and the data folder (with the transcoder's folder when it is elsewhere).
pub fn places(app: &crate::server::App) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = {
        let db = app.db.lock().unwrap();
        db.sections.iter().flat_map(|s| s.paths.iter().map(|p| (s.name.clone(), p.clone()))).collect()
    };
    out.push(("data".into(), app.data_dir.to_string_lossy().to_string()));
    out.push(("data".into(), app.transcode_dir().to_string_lossy().to_string()));
    out
}

/// Looks at the disks now and then: keeps what it found for the dashboard and fires `disk.low`.
pub fn start(app: &Arc<crate::server::App>) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut told = HashSet::new();
        std::thread::sleep(Duration::from_secs(60));
        loop {
            let found = disks(&places(&app), space);
            let percent = app.settings().disk_warn_percent;
            for d in to_report(&found, percent, &mut told) {
                elog!("disk nearly full: {:.1} % free on the disk of {} ({})", d.free_percent(), d.holds.join(", "), d.paths.join(", "));
                let mut data = d.to_json();
                data["threshold_percent"] = json!(percent);
                app.fire("disk.low", data);
            }
            *app.disks.lock().unwrap() = found;
            std::thread::sleep(Duration::from_secs(CHECK_MINUTES * 60));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(what: &str, path: &str) -> (String, String) {
        (what.into(), path.into())
    }

    #[test]
    fn folders_on_one_disk_are_one() {
        let sp = |p: &Path| match p.to_str().unwrap() {
            "/a/movies" | "/a/shows" => Some(Space { free: 10, total: 100 }),
            "/data" => Some(Space { free: 50, total: 200 }),
            _ => None,
        };
        let d = disks(&[place("Movies", "/a/movies"), place("Shows", "/a/shows"), place("data", "/data"), place("Gone", "/x")], sp);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].holds, vec!["Movies", "Shows"]);
        assert_eq!(d[0].paths, vec!["/a/movies", "/a/shows"]);
        assert_eq!(d[1].holds, vec!["data"]);
        assert_eq!(d[0].free_percent(), 10.0);
    }

    #[test]
    fn told_once_until_there_is_room_again() {
        let disk = |free| vec![Disk { holds: vec!["Movies".into()], paths: vec!["/m".into()], space: Space { free, total: 100 } }];
        let mut told = HashSet::new();
        assert_eq!(to_report(&disk(4), 5, &mut told).len(), 1);
        assert!(to_report(&disk(3), 5, &mut told).is_empty(), "only once");
        assert!(to_report(&disk(6), 5, &mut told).is_empty(), "within the margin: still counts as told");
        assert!(to_report(&disk(4), 5, &mut told).is_empty());
        assert!(to_report(&disk(8), 5, &mut told).is_empty(), "room again");
        assert_eq!(to_report(&disk(4), 5, &mut told).len(), 1, "full again: told again");
        assert!(to_report(&disk(1), 0, &mut told).is_empty(), "0 = off");
    }

    #[test]
    fn this_computer_has_a_disk() {
        if cfg!(any(unix, windows)) {
            let s = space(&std::env::temp_dir()).expect("free space of the temp folder");
            assert!(s.total > 0 && s.free <= s.total);
        }
    }
}
