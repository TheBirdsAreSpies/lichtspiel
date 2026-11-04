//! Deep analysis: how fast a connection really has to be to play a video
//! without stopping.
//!
//! The bitrate of a file is only its average. A loud action scene can need far
//! more than that for a while, and a player that has only a small store of
//! video ahead of it runs dry there. So this reads every
//! packet of the file (nothing is decoded), keeps their sizes and times, and
//! plays them back on paper: for each connection speed level it searches for
//! the lowest speed at which the store never runs empty.
//!
//! The result is one speed per level. When a stream has a speed limit, it
//! decides whether the file can be sent as it is or has to be converted.

use crate::db::MediaFile;
use std::collections::HashMap;
use std::io::BufRead as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Files analyzed by an older version count as not analyzed.
pub const VERSION: u32 = 6;
/// Connection speed levels in kbit/s; a speed is worked out for each one.
pub const BANDWIDTH_LEVELS: [u32; 8] = [5000, 10000, 25000, 50000, 75000, 100000, 250000, 500000];
/// A player's store: this many bytes per kbit/s of its level (7.2 seconds of it).
pub const BUFFER_PER_KBPS: i64 = 900;
/// Seconds of connection the store is given before playback starts.
pub const START_SECONDS: f64 = 5.0;
/// Packets of a stream closer together than this count as one.
pub const SAME_MOMENT_SECONDS: f64 = 0.01;
/// A stream gets this much more than the speed worked out.
pub const SPEED_MARGIN: f64 = 1.05;
/// Without an analysis a file is taken to need this many times its average bitrate.
pub const UNMEASURED_FACTOR: u32 = 2;
/// One file may take this long.
pub const TIME_LIMIT_SECONDS: u64 = 3600;
/// The result for a file whose packets don't fit any speed.
const NO_SPEED: u32 = i32::MAX as u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Packet {
    pub size: u32,
    pub stream: u32,
    pub time: f64,
}

#[derive(Clone, Copy)]
struct Track {
    /// Latest time seen, first time seen and time of the latest packet kept.
    time: f64,
    first: f64,
    kept: f64,
}

/// The packets of a file, sorted by time.
#[derive(Default, Debug)]
pub struct Reading {
    pub packets: Vec<Packet>,
    pub bytes: u64,
    pub start: f64,
    pub end: f64,
}

#[derive(Default)]
struct Collector {
    reading: Reading,
    tracks: HashMap<u32, Track>,
    seen_time: bool,
}

impl Collector {
    fn add(&mut self, stream: u32, time: Option<f64>, duration: f64, size: u32) {
        let track = self.tracks.entry(stream).or_insert(Track { time: f64::MAX, first: f64::MAX, kept: f64::MAX });
        // A packet without a time belongs to the moment of the one before it.
        let at = time.unwrap_or(track.time);
        if at != f64::MAX {
            track.time = at;
            if track.first == f64::MAX {
                track.first = at;
            }
        }
        let r = &mut self.reading;
        r.bytes += size as u64;
        if let Some(t) = time {
            r.start = if self.seen_time { r.start.min(t) } else { t };
            r.end = if self.seen_time { r.end.max(t + duration) } else { t + duration };
            self.seen_time = true;
        }
        if at < track.kept || track.kept + SAME_MOMENT_SECONDS <= at || r.packets.is_empty() {
            track.kept = at;
            r.packets.push(Packet { size, stream, time: at });
        } else if let Some(p) = r.packets.iter_mut().rev().find(|p| p.stream == stream) {
            p.size += size;
        }
    }

    fn finish(mut self) -> Reading {
        let earliest = self.tracks.values().map(|t| t.first).fold(f64::MAX, f64::min);
        for p in self.reading.packets.iter_mut().filter(|p| p.time == f64::MAX) {
            let first = self.tracks.get(&p.stream).map_or(f64::MAX, |t| t.first);
            p.time = if first != f64::MAX { first } else if earliest != f64::MAX { earliest } else { 0.0 };
        }
        self.reading.packets.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.reading
    }
}

/// One line of ffprobe: stream, pts, dts, duration, size.
fn parse_line(line: &str) -> Option<(u32, Option<f64>, f64, u32)> {
    let c: Vec<&str> = line.trim().split(',').collect();
    if c.len() < 5 {
        return None;
    }
    let (pts, dts) = (c[1].parse().ok(), c[2].parse().ok());
    Some((c[0].parse().ok()?, dts.or(pts), c[3].parse().unwrap_or(0.0), c[4].parse().ok()?))
}

fn collect(lines: impl Iterator<Item = String>) -> Reading {
    let mut c = Collector::default();
    for (stream, time, duration, size) in lines.filter_map(|l| parse_line(&l)) {
        c.add(stream, time, duration, size);
    }
    c.finish()
}

/// Reads the size and time of every packet of a file.
pub fn read_packets(ffprobe: &str, path: &str) -> Result<Reading, String> {
    let mut child = Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "packet=stream_index,pts_time,dts_time,duration_time,size", "-of", "csv=p=0"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("ffprobe could not be started: {e}"))?;
    let out = child.stdout.take().ok_or("no output from ffprobe")?;
    let reader = std::thread::spawn(move || collect(std::io::BufReader::new(out).lines().map_while(Result::ok)));
    let deadline = Instant::now() + Duration::from_secs(TIME_LIMIT_SECONDS);
    let finished = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let reading = reader.join().map_err(|_| "reading the packets failed")?;
    if !finished {
        return Err("reading the file took too long".into());
    }
    if reading.packets.is_empty() {
        return Err("ffprobe found no packets in this file".into());
    }
    Ok(reading)
}

/// Plays the packets on paper: true when a store of `buffer` bytes filled at
/// `kbps` never runs empty.
fn fits(kbps: u32, buffer: i64, packets: &[Packet]) -> bool {
    let per_second = kbps as f64 * 1000.0 * 0.125;
    let mut fill = ((per_second * START_SECONDS) as i64).min(buffer);
    let mut last = packets.first().map_or(0.0, |p| p.time);
    for p in packets {
        if last < p.time {
            fill = (((p.time - last) * per_second) as i64 + fill).min(buffer);
            last = p.time;
        }
        fill -= p.size as i64;
        if fill < 0 {
            return false;
        }
    }
    true
}

/// The lowest speed that fits: doubling first, then closing in.
fn lowest_speed(buffer: i64, packets: &[Packet]) -> u32 {
    let (mut low, mut high) = (0u32, 1u32);
    while !fits(high, buffer, packets) {
        if high == NO_SPEED {
            return NO_SPEED;
        }
        low = high;
        high = high.saturating_mul(2).min(NO_SPEED);
    }
    while low + 1 < high {
        let middle = (low + high) / 2;
        if fits(middle, buffer, packets) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// What deep analysis found out about a file.
#[derive(Debug, PartialEq)]
pub struct Measured {
    pub required_kbps: Vec<u32>,
    pub duration_ms: u64,
    pub bitrate: u64,
}

pub fn measure(r: &Reading) -> Measured {
    let required_kbps = BANDWIDTH_LEVELS.iter().map(|l| lowest_speed(*l as i64 * BUFFER_PER_KBPS, &r.packets)).collect();
    let duration_ms = ((r.end - r.start) * 1000.0).max(0.0) as u64;
    let bitrate = if duration_ms > 0 { (r.bytes as f64 * 8.0 * 1000.0 / duration_ms as f64) as u64 } else { 0 };
    Measured { required_kbps, duration_ms, bitrate }
}

pub fn analyze_file(ffprobe: &str, path: &str) -> Result<Measured, String> {
    Ok(measure(&read_packets(ffprobe, path)?))
}

/// Puts the result into the file's record.
pub fn apply(f: &mut MediaFile, r: Measured) {
    f.required_kbps = r.required_kbps;
    if r.duration_ms > 0 {
        f.duration_ms = r.duration_ms;
        f.bitrate = r.bitrate;
    }
    f.deep_version = VERSION;
    f.deep_date = f.modified;
    f.deep_errors = 0;
}

/// The analysis is there and belongs to the file as it is now.
pub fn is_current(f: &MediaFile) -> bool {
    f.deep_version >= VERSION && f.deep_date == f.modified
}

/// How fast a connection of `limit_kbps` has to be for this file to play
/// without stopping: the speed of the highest level the connection reaches,
/// or for a file not analyzed twice its bitrate. 0 = nothing is known.
pub fn required_kbps(f: &MediaFile, limit_kbps: u32) -> u32 {
    let reached = BANDWIDTH_LEVELS.iter().filter(|l| **l <= limit_kbps).count();
    let measured = if is_current(f) && reached > 0 { f.required_kbps.get(reached - 1).copied().filter(|k| *k != NO_SPEED) } else { None };
    let needed = measured.unwrap_or((f.bitrate / 1000) as u32 * UNMEASURED_FACTOR);
    (needed as f64 * SPEED_MARGIN).ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `seconds` of steady video at `kbps`, 25 packets a second.
    fn steady(kbps: u32, from: f64, seconds: u32) -> Vec<Packet> {
        (0..seconds * 25).map(|n| Packet { size: kbps * 125 / 25, stream: 0, time: from + n as f64 / 25.0 }).collect()
    }

    #[test]
    fn a_steady_file_needs_about_its_bitrate() {
        let k = lowest_speed(5000 * BUFFER_PER_KBPS, &steady(4000, 0.0, 60));
        assert!((3500..=4000).contains(&k), "{k}");
    }

    #[test]
    fn a_short_loud_scene_needs_more_than_the_average() {
        let mut p = steady(2000, 0.0, 120);
        p.extend(steady(40000, 120.0, 20));
        p.extend(steady(2000, 140.0, 120));
        let small = lowest_speed(5000 * BUFFER_PER_KBPS, &p);
        let large = lowest_speed(100000 * BUFFER_PER_KBPS, &p);
        // The average is under 5000: a small store has to get far more through
        // the connection, a store that holds the scene gets by on less.
        assert!(small > 25000, "{small}");
        assert!(large < small, "{large} {small}");
    }

    #[test]
    fn a_packet_larger_than_the_store_fits_no_speed() {
        let p = [Packet { size: 10_000_000, stream: 0, time: 0.0 }];
        assert_eq!(lowest_speed(5000 * BUFFER_PER_KBPS, &p), NO_SPEED);
    }

    #[test]
    fn packets_of_one_moment_are_kept_together() {
        let lines = ["0,1.000,1.000,0.040,100", "1,1.000,1.000,0.020,50", "0,1.005,1.005,0.040,100", "0,1.100,1.100,0.040,100", "0,N/A,N/A,N/A,7", "x"];
        let r = collect(lines.iter().map(|s| s.to_string()));
        let sizes: Vec<(u32, u32)> = r.packets.iter().map(|p| (p.stream, p.size)).collect();
        // The packet without a time joins the one before it.
        assert_eq!(sizes, vec![(0, 200), (1, 50), (0, 107)]);
        assert_eq!(r.bytes, 357);
        assert!((r.end - 1.14).abs() < 1e-9 && r.start == 1.0);
    }

    #[test]
    fn the_needed_speed_follows_the_level_the_connection_reaches() {
        let mut f = MediaFile { bitrate: 3_000_000, modified: 9, ..Default::default() };
        // Not analyzed: twice the bitrate, plus the margin.
        assert_eq!(required_kbps(&f, 8000), 6300);
        f.required_kbps = vec![9000, 7000, 5000, 4000, 3900, 3800, 3700, 3600];
        f.deep_version = VERSION;
        f.deep_date = 9;
        assert_eq!(required_kbps(&f, 8000), 9450);
        assert_eq!(required_kbps(&f, 30000), 5250);
        // Below the lowest level nothing was measured.
        assert_eq!(required_kbps(&f, 4000), 6300);
        // The file changed: the analysis no longer counts.
        f.modified = 10;
        assert!(!is_current(&f));
        assert_eq!(required_kbps(&f, 30000), 6300);
    }

    /// A real file through ffprobe (skipped when ffmpeg isn't installed).
    #[test]
    fn reads_a_real_file() {
        if Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| !s.success()).unwrap_or(true) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("lichtspiel-deep-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("v.mkv");
        let ok = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=d=6:s=320x240:r=25", "-f", "lavfi", "-i", "sine=d=6", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac"])
            .arg(&file).stdin(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        assert!(ok, "ffmpeg made the test file");
        let size = std::fs::metadata(&file).unwrap().len();
        let r = read_packets("ffprobe", &file.to_string_lossy()).expect("packets read");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(r.packets.len() > 150, "{}", r.packets.len());
        assert!(r.packets.windows(2).all(|w| w[0].time <= w[1].time));
        assert!(r.bytes > size / 2 && r.bytes <= size, "{} of {size}", r.bytes);
        assert!((r.end - r.start - 6.0).abs() < 0.3, "{} to {}", r.start, r.end);
        let m = measure(&r);
        assert_eq!(m.required_kbps.len(), BANDWIDTH_LEVELS.len());
        assert!(m.required_kbps.iter().all(|k| *k > 0 && *k != NO_SPEED));
        assert!(m.bitrate > 0 && (m.duration_ms as i64 - 6000).abs() < 300);
    }
}
