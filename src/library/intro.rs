//! TV show intro detection ("Skip intro").
//!
//! How it works:
//! 1. For every episode of a season it reads the sound of the first 600
//!    seconds (decoded to a temporary FLAC file) and
//!    turns it into an audio fingerprint: one 32-bit value per 0.1238 seconds.
//! 2. Every episode is compared with every other one. Both need
//!    more than 161 values (about 20 s). Values whose top 20 bits agree
//!    (`value >> 12`) vote for how far one episode is shifted against the other;
//!    the most common shift is tried, and every other shift with at least 53
//!    votes.
//! 3. Along each shift a window of 137 values (about 17 s) slides over both
//!    episodes; the share of equal bits is the likeness ( counts
//!    `32 - popcount(a ^ b)`). The best window must reach 0.7.
//! 4. From there the match grows backwards and forwards in steps of 8 values,
//!    checking 16 values at a time, as long as the likeness stays above
//!    0.7 - 0.1 (never below 0.1). The shared
//!    part must be longer than 20 seconds in both episodes.
//! 5. Each episode collects the parts it shared with the others. Parts whose
//!    start and end are within 2 seconds of each other count
//!    as the same; a part inside another one also counts for it.
//!    An episode's intro is a part found more often than in 0.2 of the
//!    episodes. When fewer than half of the episodes get an
//!    intro, the season gets none ("Less than half of episodes have an intro").
//! 6. The marker stops 3.5 seconds before the end of the shared sound, and an
//!    episode gets none when the shared sound reaches within 2 seconds of the
//!    end of what was read: then it never really ended. Such an
//!    episode still counts as looked at.
//! 7. Automatic runs take the seasons with an intro from an older version of
//!    the detector first, then those with an episode never looked at, newest
//!    first. Episodes that had no intro are not looked at again.
//!
//! "Previously on" (a recap at the start of an episode) is found with the same
//! fingerprints: the first 300 seconds of an episode are compared in short
//! windows of 24 values (about 3 s) with the whole sound of the one or two
//! episodes before it (`find_recap`). A window counts when its likeness reaches
//! 0.75 somewhere outside the earlier episode's intro and credits. Windows next to
//! each other at the same shift make a clip; a clip longer than 30 s is shared
//! music (an intro), not a recap. Clips less than 12 s apart make a group, and the
//! first group with clips from at least 3 places of the earlier episodes and 12 s
//! of shared sound in all is the recap.
//!
//! The fingerprint is a classic band-energy fingerprint (33 bands between 300 and 2000 Hz, one bit per
//! pair of neighbouring bands, frame 4096 samples at 11025 Hz, a new value
//! every 1365 samples = 0.1238 s), which the same matching works on.

use std::collections::HashMap;
use std::io::Read as _;
use std::process::{Command, Stdio};

/// Seconds of sound read from the start of each episode (600).
pub const SECONDS_ANALYZED: u64 = 600;
/// Seconds per fingerprint value.
pub const ITEM_SECONDS: f32 = 0.1238;
/// An episode needs more than this many values to be compared (0xa1).
pub const MIN_ITEMS: usize = 161;
/// Values in the sliding window (0x89).
pub const WINDOW: usize = 137;
/// Likeness the best window must reach.
pub const MATCH_THRESHOLD: f32 = 0.7;
/// How much lower the likeness may drop while the match grows.
pub const EXTEND_DROP: f32 = 0.1;
/// Values compared at a time, and how far each step grows the match.
pub const EXTEND_BLOCK: usize = 16;
pub const EXTEND_STEP: usize = 8;
/// Shortest intro, seconds.
pub const MIN_INTRO_SECONDS: f32 = 20.0;
/// Start and end within this many seconds: the same intro.
pub const SAME_INTRO_SECONDS: f32 = 2.0;
/// Parts shorter than this are not counted at all.
pub const MIN_PART_SECONDS: f32 = 1.0;
/// An intro must be found in more than this share of the episodes.
pub const MIN_SHARE: f64 = 0.2;
/// Further shifts are tried when they have at least this many votes (0x35).
pub const OFFSET_MIN_HITS: usize = 53;
/// Only the top bits of a value vote for a shift (`>> 0xc`).
pub const KEY_SHIFT: u32 = 12;
/// A value has 32 bits: turns equal bits per value into a share.
pub const BIT_SHARE: f32 = 0.03125;
/// The marker ends this many seconds before the shared sound does.
pub const END_TRIM_SECONDS: f64 = 3.5;
/// Shared sound this close to the end of what was read has no real end.
pub const TAIL_SECONDS: f64 = 2.0;
/// Bump when detection changes, so seasons with an intro are looked at again.
pub const DETECTOR_VERSION: u32 = 2;
/// Bump when the fingerprints themselves change, so they are read again.
const PRINT_VERSION: u32 = 1;

/// Recap ("Previously on") detection, see the top of the file.
pub const RECAP_SEARCH_SECONDS: f32 = 300.0;
pub const RECAP_CLIP: usize = 24;
pub const RECAP_STEP: usize = 4;
pub const RECAP_THRESHOLD: f32 = 0.75;
pub const RECAP_MIN_SECONDS: f32 = 12.0;
pub const RECAP_MIN_PLACES: usize = 3;
pub const RECAP_GAP_SECONDS: f32 = 12.0;
pub const RECAP_LONGEST_CLIP: f32 = 30.0;
/// Two clips whose shifts against an earlier episode differ by more than this come from two places.
pub const RECAP_PLACE_SECONDS: f32 = 10.0;
/// Bump when recap detection changes, so episodes are looked at again.
pub const RECAP_VERSION: u32 = 1;

const SAMPLE_RATE: u32 = 11025;
const FRAME: usize = 4096;
const HOP: usize = 1365;
const BANDS: usize = 33;
const LOW_HZ: f32 = 300.0;
const HIGH_HZ: f32 = 2000.0;

/// The fingerprint of the first minutes of a file's sound.
pub fn fingerprint_file(ffmpeg: &str, path: &str) -> Result<Vec<u32>, String> {
    fingerprint_part(ffmpeg, path, true)
}

/// The fingerprint of a file's whole sound (for the recaps of the episodes after it).
pub fn fingerprint_whole_file(ffmpeg: &str, path: &str) -> Result<Vec<u32>, String> {
    fingerprint_part(ffmpeg, path, false)
}

fn fingerprint_part(ffmpeg: &str, path: &str, head: bool) -> Result<Vec<u32>, String> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
    if head {
        cmd.args(["-t", &SECONDS_ANALYZED.to_string()]);
    }
    let mut child = cmd
        .arg("-i")
        .arg(path)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "s16le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("ffmpeg could not be started: {e}"))?;
    let mut out = child.stdout.take().ok_or("no output from ffmpeg")?;
    let reader = std::thread::spawn(move || {
        let mut buf = vec![];
        let _ = out.read_to_end(&mut buf);
        buf
    });
    // The whole file can mean reading many gigabytes from a network share.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(if head { 300 } else { 1800 });
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let buf = reader.join().unwrap_or_default();
    if status.is_none() {
        return Err("ffmpeg took too long reading the sound".into());
    }
    if buf.len() < FRAME * 2 {
        return Err("ffmpeg could not read the sound of this video".into());
    }
    let samples: Vec<f32> = buf.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    Ok(fingerprint(&samples))
}

/// In-place radix-2 FFT (length a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

/// Fingerprint of mono samples at 11025 Hz: one value per 0.1238 s.
pub fn fingerprint(samples: &[f32]) -> Vec<u32> {
    let window: Vec<f32> = (0..FRAME).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FRAME - 1) as f32).cos()).collect();
    // Band edges as FFT bins, spaced evenly on a log scale.
    let edges: Vec<usize> = (0..=BANDS)
        .map(|k| {
            let hz = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(k as f32 / BANDS as f32);
            (hz * FRAME as f32 / SAMPLE_RATE as f32).round() as usize
        })
        .collect();
    let mut out = vec![];
    let mut prev = [0f32; BANDS];
    let (mut re, mut im) = (vec![0f32; FRAME], vec![0f32; FRAME]);
    let mut start = 0;
    while start + FRAME <= samples.len() {
        for i in 0..FRAME {
            re[i] = samples[start + i] * window[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        let mut e = [0f32; BANDS];
        for (b, band) in e.iter_mut().enumerate() {
            *band = (edges[b]..edges[b + 1].max(edges[b] + 1)).map(|k| re[k] * re[k] + im[k] * im[k]).sum();
        }
        let total: f32 = e.iter().sum();
        let mut v = 0u32;
        if total < 1e-6 {
            // Silence would look the same in every episode; give it values that
            // match nothing (a mix of its position), so quiet stretches never
            // count as a shared intro.
            let mut x = (out.len() as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
            x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            v = (x ^ (x >> 31)) as u32;
        } else {
            for m in 0..BANDS - 1 {
                if (e[m] - e[m + 1]) - (prev[m] - prev[m + 1]) > 0.0 {
                    v |= 1 << m;
                }
            }
        }
        out.push(v);
        prev = e;
        start += HOP;
    }
    out
}

/// Share of equal bits: equal bits per value, times 1/32.
fn likeness(a: &[u32], b: &[u32]) -> f32 {
    let same: u32 = a.iter().zip(b).map(|(x, y)| 32 - (x ^ y).count_ones()).sum();
    same as f32 / a.len().max(1) as f32 * BIT_SHARE
}

/// Shifts (b index − a index) worth trying, most votes first.
fn shifts(a: &[u32], b: &[u32]) -> Vec<i64> {
    let mut at: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, v) in a.iter().enumerate() {
        at.entry(v >> KEY_SHIFT).or_default().push(i);
    }
    let mut votes: HashMap<i64, usize> = HashMap::new();
    for (j, v) in b.iter().enumerate() {
        if let Some(is) = at.get(&(v >> KEY_SHIFT)) {
            // A value that comes back all the time (a held tone) says nothing.
            if is.len() > 100 {
                continue;
            }
            for &i in is {
                *votes.entry(j as i64 - i as i64).or_default() += 1;
            }
        }
    }
    let mut v: Vec<(i64, usize)> = votes.into_iter().collect();
    v.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
    if v.len() > 2 {
        let first = v[0];
        v.retain(|x| x.1 >= OFFSET_MIN_HITS);
        if v.first() != Some(&first) {
            v.insert(0, first);
        }
    }
    v.into_iter().map(|x| x.0).collect()
}

/// A part two episodes share: seconds from the start of each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shared {
    pub a: (f32, f32),
    pub b: (f32, f32),
}

/// The longest stretch two episodes share, if it is long enough to be an
/// intro.
pub fn compare(a: &[u32], b: &[u32]) -> Option<Shared> {
    if a.len() <= MIN_ITEMS || b.len() <= MIN_ITEMS {
        return None;
    }
    let mut best: Option<(f32, i64, usize)> = None;
    for o in shifts(a, b) {
        let from = (-o).max(0) as usize;
        let to = (a.len() as i64).min(b.len() as i64 - o);
        if to - (from as i64) < WINDOW as i64 {
            continue;
        }
        for i in from..=(to as usize - WINDOW) {
            let j = (i as i64 + o) as usize;
            let s = likeness(&a[i..i + WINDOW], &b[j..j + WINDOW]);
            if best.is_none_or(|x| s > x.0) {
                best = Some((s, o, i));
            }
        }
    }
    let (score, o, i) = best?;
    if score < MATCH_THRESHOLD {
        return None;
    }
    let keep = (MATCH_THRESHOLD - EXTEND_DROP).max(0.1);
    let bj = |i: usize| (i as i64 + o) as usize;
    let half = EXTEND_BLOCK - EXTEND_STEP;
    let (mut s, mut e) = (i, i + WINDOW);
    // Backwards: the 16 values around the start, 8 at a time.
    while s >= EXTEND_STEP && bj(s) >= EXTEND_STEP {
        let (x, y) = (s - EXTEND_STEP, bj(s) - EXTEND_STEP);
        if likeness(&a[x..x + EXTEND_BLOCK], &b[y..y + EXTEND_BLOCK]) <= keep {
            break;
        }
        s -= EXTEND_STEP;
    }
    // Forwards: the 16 values around the end.
    while e + EXTEND_STEP <= a.len() && bj(e) + EXTEND_STEP <= b.len() {
        let (x, y) = (e - half, bj(e) - half);
        if likeness(&a[x..x + EXTEND_BLOCK], &b[y..y + EXTEND_BLOCK]) <= keep {
            break;
        }
        e += EXTEND_STEP;
    }
    let secs = |n: usize| n as f32 * ITEM_SECONDS;
    let shared = Shared { a: (secs(s), secs(e)), b: (secs(bj(s)), secs(bj(e))) };
    let long = |p: (f32, f32)| p.1 - p.0 > MIN_INTRO_SECONDS;
    (long(shared.a) && long(shared.b)).then_some(shared)
}

#[derive(Clone, Copy, Debug)]
struct Part {
    start: f32,
    end: f32,
    count: u32,
}

/// Adds a shared part to an episode's list, counting agreements.
fn add_part(list: &mut Vec<Part>, (start, end): (f32, f32)) {
    if end - start < MIN_PART_SECONDS {
        return;
    }
    let mut new = Part { start, end, count: 1 };
    for c in list.iter_mut() {
        if (new.start - c.start).abs() <= SAME_INTRO_SECONDS && (new.end - c.end).abs() <= SAME_INTRO_SECONDS {
            c.count += 1;
            new.count += 1;
        } else if new.start <= c.start && new.end >= c.end {
            c.count += 1;
        } else if c.start <= new.start && new.end <= c.end {
            new.count += 1;
        }
    }
    list.push(new);
}

/// The marker (start, end in ms) for a shared part of an episode of which
/// `length` seconds were read, if it gets one.
fn marker(start: f32, end: f32, length: f64) -> Option<(u64, u64)> {
    let from = (start * 1000.0) as i64;
    let to = ((end as f64 - END_TRIM_SECONDS) * 1000.0) as i64;
    (to > from && length - TAIL_SECONDS >= end as f64).then_some((from as u64, to as u64))
}

/// Whether an episode is due for automatic detection: never looked at, or it
/// has an intro from an older detector.
pub fn is_due(checked: u32, has_intro: bool) -> bool {
    checked == 0 || (checked < DETECTOR_VERSION && has_intro)
}

/// Intros (start, end in ms) of a season's episodes, in the order given; None
/// where an episode has none. `prints` empty = that episode could not be read.
pub fn detect_season(prints: &[Vec<u32>]) -> Vec<Option<(u64, u64)>> {
    let n = prints.len();
    let mut parts: Vec<Vec<Part>> = vec![vec![]; n];
    for i in 0..n {
        for j in i + 1..n {
            if prints[i].is_empty() || prints[j].is_empty() {
                continue;
            }
            if let Some(s) = compare(&prints[i], &prints[j]) {
                add_part(&mut parts[i], s.a);
                add_part(&mut parts[j], s.b);
            }
        }
    }
    let need = n as f64 * MIN_SHARE;
    let mut best: Vec<Option<Part>> = parts
        .into_iter()
        .map(|mut p| {
            // Longest first, then the first one found often enough
            // (sorted by end − start, largest first).
            p.sort_by(|x, y| (y.end - y.start).total_cmp(&(x.end - x.start)));
            p.into_iter().find(|x| x.count as f64 > need)
        })
        .collect();
    if best.iter().filter(|x| x.is_some()).count() < n / 2 {
        best.iter_mut().for_each(|x| *x = None);
    }
    best.into_iter()
        .zip(prints)
        .map(|(p, print)| p.and_then(|x| marker(x.start, x.end, (print.len() as f32 * ITEM_SECONDS) as f64)))
        .collect()
}

/// Where the recap runs in an episode (start and end, ms), if it has one.
/// `head`: the fingerprint of its first minutes; `skip`: parts of it not to look
/// at (its intro, seconds); `earlier`: the whole fingerprints of the episodes
/// before it, each with the parts that don't count (their intro and credits).
pub fn find_recap(head: &[u32], skip: &[(f32, f32)], earlier: &[(&[u32], Vec<(f32, f32)>)]) -> Option<(u64, u64)> {
    let secs = |n: usize| n as f32 * ITEM_SECONDS;
    let inside = |from: usize, to: usize, parts: &[(f32, f32)]| parts.iter().any(|&(s, e)| secs(to) > s && secs(from) < e);
    let limit = ((RECAP_SEARCH_SECONDS / ITEM_SECONDS) as usize).min(head.len());
    // (window in `head`, which earlier episode, where in it − where in `head`)
    let mut hits: Vec<(usize, usize, i64)> = vec![];
    for (n, (print, excluded)) in earlier.iter().enumerate() {
        let mut at: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, v) in print.iter().enumerate() {
            at.entry(v >> KEY_SHIFT).or_default().push(i);
        }
        let mut i = 0;
        while i + RECAP_CLIP <= limit {
            let window = &head[i..i + RECAP_CLIP];
            if !inside(i, i + RECAP_CLIP, skip) {
                let mut votes: HashMap<i64, usize> = HashMap::new();
                for (k, v) in window.iter().enumerate() {
                    let Some(ps) = at.get(&(v >> KEY_SHIFT)) else { continue };
                    // A value that comes back all the time (a held tone) says nothing.
                    if ps.len() > 100 {
                        continue;
                    }
                    for &p in ps {
                        *votes.entry(p as i64 - k as i64).or_default() += 1;
                    }
                }
                let mut best: Option<(f32, i64)> = None;
                for (&c, &count) in &votes {
                    if count < 2 || c < 0 || c as usize + RECAP_CLIP > print.len() || inside(c as usize, c as usize + RECAP_CLIP, excluded) {
                        continue;
                    }
                    let s = likeness(window, &print[c as usize..c as usize + RECAP_CLIP]);
                    if s >= RECAP_THRESHOLD && best.is_none_or(|b| s > b.0) {
                        best = Some((s, c - i as i64));
                    }
                }
                if let Some((_, o)) = best {
                    hits.push((i, n, o));
                }
            }
            i += RECAP_STEP;
        }
    }
    hits.sort_by_key(|h| h.0);
    // Clips: (start, end in `head`, earlier episode, shift).
    let mut clips: Vec<(usize, usize, usize, i64)> = vec![];
    for (i, n, o) in hits {
        if let Some(c) = clips.iter_mut().rev().find(|c| c.2 == n && (c.3 - o).abs() <= 2 && i <= c.1 + RECAP_STEP) {
            c.1 = c.1.max(i + RECAP_CLIP);
            continue;
        }
        clips.push((i, i + RECAP_CLIP, n, o));
    }
    clips.retain(|c| secs(c.1 - c.0) <= RECAP_LONGEST_CLIP);
    clips.sort_by_key(|c| c.0);
    let place_gap = (RECAP_PLACE_SECONDS / ITEM_SECONDS) as i64;
    let judge = |group: &[(usize, usize, usize, i64)]| -> Option<(u64, u64)> {
        // Pieces of one stretch of sound share a shift; clips from other places of
        // the earlier episode have other shifts.
        let mut places: Vec<(usize, i64)> = vec![];
        for c in group {
            if !places.iter().any(|p| p.0 == c.2 && (p.1 - c.3).abs() <= place_gap) {
                places.push((c.2, c.3));
            }
        }
        let (start, end) = (group.first()?.0, group.iter().map(|c| c.1).max()?);
        // Shared sound in all, overlapping clips counted once.
        let (mut covered, mut upto) = (0, start);
        for c in group {
            let from = c.0.max(upto);
            if c.1 > from {
                covered += c.1 - from;
                upto = c.1;
            }
        }
        (places.len() >= RECAP_MIN_PLACES && secs(covered) >= RECAP_MIN_SECONDS).then(|| ((secs(start) * 1000.0) as u64, (secs(end) * 1000.0) as u64))
    };
    let gap = (RECAP_GAP_SECONDS / ITEM_SECONDS) as usize;
    let mut group: Vec<(usize, usize, usize, i64)> = vec![];
    for c in clips {
        let end = group.iter().map(|g| g.1).max().unwrap_or(0);
        if !group.is_empty() && c.0 > end + gap {
            if let Some(found) = judge(&group) {
                return Some(found);
            }
            group.clear();
        }
        group.push(c);
    }
    judge(&group)
}

/// Whether an episode is due for recap detection: not looked at by this
/// version yet, and reading the files did not fail too often.
pub fn recap_due(checked: u32, errors: u32) -> bool {
    checked < RECAP_VERSION && errors <= 3
}

/// Cached fingerprints live in data/cache/intro, one file per video, named
/// after its path, size and change time, so a new episode doesn't mean reading the whole season again.
pub fn cache_name(path: &str, size: u64, modified: u64) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.bytes().chain(size.to_le_bytes()).chain(modified.to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}-v{PRINT_VERSION}.fp")
}

/// The same for the fingerprint of the whole sound.
pub fn cache_name_whole(path: &str, size: u64, modified: u64) -> String {
    cache_name(path, size, modified).replace(".fp", "-whole.fp")
}

pub fn read_cached(file: &std::path::Path) -> Option<Vec<u32>> {
    let b = std::fs::read(file).ok()?;
    Some(b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

pub fn write_cached(file: &std::path::Path, print: &[u32]) {
    if let Some(d) = file.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let bytes: Vec<u8> = print.iter().flat_map(|v| v.to_le_bytes()).collect();
    let _ = std::fs::write(file, bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A little random generator, so the test sounds are the same every time.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
        }
    }

    /// "Music": a few changing tones with a beat, so it has a shape over time.
    fn tune(seconds: f32, seed: u64) -> Vec<f32> {
        let mut r = Rng(seed);
        let n = (seconds * SAMPLE_RATE as f32) as usize;
        let mut notes = vec![];
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if i % (SAMPLE_RATE as usize / 4) == 0 {
                notes = (0..3).map(|_| 300.0 + (r.next() + 1.0) * 800.0).collect();
            }
            let t = i as f32 / SAMPLE_RATE as f32;
            let v: f32 = notes.iter().map(|f| (2.0 * std::f32::consts::PI * f * t).sin()).sum::<f32>() / 3.0;
            out.push(v * 0.5 + r.next() * 0.02);
        }
        out
    }

    #[test]
    fn finds_the_shared_intro() {
        // Three episodes: different openings (12, 40 and 25 s), the same 35 s
        // intro, then different stories.
        let intro = tune(35.0, 7);
        let episode = |cold: f32, seed: u64| {
            let mut s = tune(cold, seed);
            s.extend_from_slice(&intro);
            s.extend(tune(60.0, seed + 100));
            s
        };
        let prints: Vec<Vec<u32>> = [(12.0, 1), (40.0, 2), (25.0, 3)].iter().map(|&(c, s)| fingerprint(&episode(c, s))).collect();
        let found = detect_season(&prints);
        for (f, cold) in found.iter().zip([12.0f32, 40.0, 25.0]) {
            let (s, e) = f.expect("intro found");
            assert!((s as f32 / 1000.0 - cold).abs() < 2.5, "start {s} for cold open {cold}");
            assert!((e as f32 / 1000.0 - (cold + 35.0 - 3.5)).abs() < 2.5, "end {e} for cold open {cold}");
        }
    }

    /// Real files through ffmpeg (skipped when ffmpeg isn't installed): two
    /// "episodes" with different noise around the same 30 s of sound.
    #[test]
    fn finds_the_intro_in_real_files() {
        if Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| !s.success()).unwrap_or(true) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("lichtspiel-intro-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut prints = vec![];
        for (k, cold) in [(1, 15), (2, 50)] {
            let out = dir.join(format!("e{k}.mkv"));
            let graph = format!(
                "anoisesrc=d={cold}:c=pink:seed={k}:a=0.3[a];anoisesrc=d=30:c=pink:seed=99:a=0.3,volume='if(lt(mod(t,1),0.5),1,0.3)':eval=frame[b];anoisesrc=d=40:c=pink:seed={}:a=0.3[c];[a][b][c]concat=n=3:v=0:a=1",
                k + 10
            );
            let ok = Command::new("ffmpeg").args(["-nostdin", "-v", "error", "-y", "-filter_complex", &graph, "-c:a", "aac", "-b:a", "128k"]).arg(&out)
                .stdin(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
            assert!(ok, "ffmpeg made the test file");
            prints.push(fingerprint_file("ffmpeg", &out.to_string_lossy()).expect("sound read"));
        }
        let _ = std::fs::remove_dir_all(&dir);
        let found = detect_season(&prints);
        for (f, cold) in found.iter().zip([15.0f32, 50.0]) {
            let (s, e) = f.expect("intro found in the files");
            assert!((s as f32 / 1000.0 - cold).abs() < 2.5 && (e as f32 / 1000.0 - (cold + 30.0 - 3.5)).abs() < 2.5, "{s}–{e} ms for cold open {cold}");
        }
    }

    #[test]
    fn nothing_shared_means_no_intro() {
        let prints: Vec<Vec<u32>> = (1..=3).map(|s| fingerprint(&tune(90.0, s))).collect();
        assert!(detect_season(&prints).iter().all(|x| x.is_none()));
    }

    #[test]
    fn the_marker_stops_before_the_shared_sound_does() {
        assert_eq!(marker(10.0, 50.0, 600.0), Some((10_000, 46_500)));
        // Too short once trimmed.
        assert_eq!(marker(10.0, 13.5, 600.0), None);
        // Reaches the end of what was read: the sound never really ended.
        assert_eq!(marker(10.0, 599.0, 600.0), None);
        assert_eq!(marker(10.0, 598.0, 600.0), Some((10_000, 594_500)));
    }

    #[test]
    fn only_unseen_episodes_and_old_intros_are_due() {
        assert!(is_due(0, false));
        assert!(is_due(DETECTOR_VERSION - 1, true));
        assert!(!is_due(DETECTOR_VERSION - 1, false));
        assert!(!is_due(DETECTOR_VERSION, true));
    }

    /// A recap: 4 s clips from four places of the episode before, then the
    /// intro (which both episodes have) and the new story.
    #[test]
    fn finds_the_recap_from_the_episode_before() {
        let intro = tune(30.0, 7);
        let mut before = tune(20.0, 50);
        before.extend_from_slice(&intro);
        let story_before = tune(240.0, 51);
        before.extend_from_slice(&story_before);
        let rate = SAMPLE_RATE as usize;
        let mut episode = vec![];
        for at in [20, 90, 150, 210] {
            episode.extend_from_slice(&story_before[at * rate..(at + 4) * rate]);
        }
        let recap_end = episode.len() as f32 / rate as f32;
        episode.extend_from_slice(&intro);
        episode.extend(tune(120.0, 60));
        let head = fingerprint(&episode);
        let whole = fingerprint(&before);
        let (s, e) = find_recap(&head, &[(16.0, 46.0)], &[(&whole, vec![(20.0, 50.0)])]).expect("recap found");
        assert!(s < 1500, "start {s}");
        assert!((e as f32 / 1000.0 - recap_end).abs() < 2.5, "end {e}, recap ends at {recap_end}");
        // Without the intro marks the shared intro alone is no recap (one long clip).
        let mut only_intro = tune(15.0, 70);
        only_intro.extend_from_slice(&intro);
        only_intro.extend(tune(60.0, 71));
        assert_eq!(find_recap(&fingerprint(&only_intro), &[], &[(&whole, vec![])]), None);
        // Nothing shared: none.
        assert_eq!(find_recap(&fingerprint(&tune(120.0, 80)), &[], &[(&whole, vec![])]), None);
    }

    #[test]
    fn same_part_is_counted_once_per_agreement() {
        let mut l = vec![];
        add_part(&mut l, (10.0, 40.0));
        add_part(&mut l, (11.0, 41.0));
        add_part(&mut l, (0.0, 0.5));
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].count, l[1].count), (2, 2));
    }
}
