//! Finds video files in a library's folders, works out movie titles and
//! show / season / episode numbers from the names, and reads each file's
//! container and codecs with ffprobe.

use crate::db::{now, sort_title, Db, Item, ItemKind, MediaFile, Section, SectionKind, Stream};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every video container the scanner picks up. Anything ffmpeg can read
/// can be streamed (directly when the player supports it, transcoded otherwise).
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "3g2", "3gp", "amv", "asf", "avi", "bik", "divx", "dv", "dvr-ms", "evo", "f4v", "flv", "h264",
    "hevc", "m1v", "m2t", "m2ts", "m2v", "m4v", "mk3d", "mkv", "mov", "mp2", "mp4", "mpe", "mpeg",
    "mpg", "mpv", "mts", "mxf", "nsv", "nut", "ogm", "ogv", "qt", "rm", "rmvb", "tod", "trp", "ts",
    "vob", "vro", "webm", "wmv", "wtv", "xvid", "y4m",
];

pub const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub", "idx", "smi"];
pub const POSTER_NAMES: &[&str] = &["poster", "folder", "cover", "show", "movie"];

pub fn is_video(path: &Path) -> bool {
    let ext = ext_of(path);
    VIDEO_EXTENSIONS.contains(&ext.as_str())
}

pub fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 12 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with("@ea") || name == "#recycle" || name == "@eaDir"
        {
            // Hidden files and Synology's own thumbnail / recycle folders.
            continue;
        }
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out, depth + 1),
            Ok(_) if is_video(&p) => {
                // Skip release "sample" clips (name is "sample", ends in "-sample"/".sample",
                // or lies in a "Sample" folder), but not films that merely have the word.
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
                let in_sample_dir = dir.file_name().is_some_and(|d| d.to_string_lossy().eq_ignore_ascii_case("sample"));
                let is_sample = stem == "sample" || stem.ends_with("-sample") || stem.ends_with(".sample") || stem.ends_with("_sample") || in_sample_dir;
                if is_sample && file_size(&p) < 300 * 1024 * 1024 {
                    continue;
                }
                out.push(p)
            }
            _ => {}
        }
    }
}

/// A fingerprint of the video files (names, sizes, change times) in a
/// library's folders. It changes when a file is added, removed, renamed or is
/// still being copied.
pub fn folder_signature(paths: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for root in paths {
        let mut files = vec![];
        walk(Path::new(root), &mut files, 0);
        for f in files {
            f.hash(&mut h);
            file_size(&f).hash(&mut h);
            file_mtime(&f).hash(&mut h);
        }
    }
    h.finish()
}

fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

fn file_mtime(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Words that mark the end of a title in release-style names.
const STOP_WORDS: &[&str] = &[
    "2160p", "1080p", "1080i", "720p", "576p", "480p", "4k", "uhd", "hdr", "hdr10", "dv",
    "bluray", "blu-ray", "bdrip", "brrip", "webrip", "web-dl", "webdl", "web", "hdtv", "dvdrip",
    "dvd", "remux", "x264", "x265", "h264", "h265", "hevc", "xvid", "divx", "aac", "ac3", "dts",
    "atmos", "proper", "repack", "extended", "unrated", "multi", "german", "dl", "10bit",
];

/// Splits the part marker off a movie file name, as Plex names split movies:
/// "The Dark Knight (2008) - pt1" gives ("The Dark Knight (2008)", Some(1)).
/// Words: cd, disc, disk, dvd, part, pt, with the number right after them, after
/// "-", "." or "_" (so a title like "Deathly Hallows Part 2" is left alone).
pub fn split_part(stem: &str) -> (String, Option<u32>) {
    let lower = stem.to_lowercase();
    for word in ["part", "disc", "disk", "dvd", "cd", "pt"] {
        let Some(at) = lower.rfind(word) else { continue };
        let num = &lower[at + word.len()..];
        if num.is_empty() || num.len() > 2 || !num.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let before = &stem[..at];
        if !before.trim_end_matches(' ').ends_with(['.', '_', '-', '–']) {
            continue;
        }
        let title = before.trim_end_matches([' ', '.', '_', '-', '–', '(', '[']).to_string();
        if title.is_empty() {
            continue;
        }
        return (title, num.parse().ok());
    }
    (stem.to_string(), None)
}

/// An id tag in a file or folder name: Plex's "{imdb-tt0068646}", "{tmdb-238}",
/// "{tvdb-81189}", and Jellyfin's "[imdbid-tt0068646]" / "[tmdbid=238]".
/// Gives the source ("imdb", "tmdb" or "tvdb") and the id.
pub fn id_tag(name: &str) -> Option<(&'static str, String)> {
    let lower = name.to_lowercase();
    let mut rest = lower.as_str();
    while let Some(open) = rest.find(['{', '[']) {
        let close_ch = if rest[open..].starts_with('{') { '}' } else { ']' };
        let inner_start = open + 1;
        let Some(len) = rest[inner_start..].find(close_ch) else { break };
        let inner = rest[inner_start..inner_start + len].trim();
        rest = &rest[inner_start + len + 1..];
        let Some(sep) = inner.find(['-', '=']) else { continue };
        let (key, value) = (inner[..sep].trim(), inner[sep + 1..].trim());
        let source = match key {
            "imdb" | "imdbid" => "imdb",
            "tmdb" | "tmdbid" => "tmdb",
            "tvdb" | "tvdbid" => "tvdb",
            _ => continue,
        };
        let ok = match source {
            "imdb" => value.len() > 2 && value.starts_with("tt") && value[2..].chars().all(|c| c.is_ascii_digit()),
            _ => !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()),
        };
        if ok {
            return Some((source, value.to_string()));
        }
    }
    None
}

/// Turns "The.Matrix.1999.1080p.BluRay" or "The Matrix (1999)" into
/// ("The Matrix", Some(1999)).
pub fn parse_title_year(raw: &str) -> (String, Option<u32>) {
    let mut s = raw.to_string();
    if !s.contains(' ') {
        s = s.replace(['.', '_'], " ");
    } else {
        s = s.replace('_', " ");
    }
    // Drop bracketed tags like [1080p] but keep (1999).
    let mut cleaned = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => cleaned.push(c),
            _ => {}
        }
    }
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    // The title ends at the first release tag; the year is the last
    // year-like word before that (a year in brackets wins).
    let mut end = words.len();
    for (i, w) in words.iter().enumerate().skip(1) {
        let bare = w.trim_matches(|c: char| c == '(' || c == ')' || c == '-' || c == ',');
        if STOP_WORDS.contains(&bare.to_lowercase().as_str()) {
            end = i;
            break;
        }
    }
    let mut year_at: Option<(usize, u32, bool)> = None;
    for (i, w) in words.iter().enumerate().take(end).skip(1) {
        let bracketed = w.starts_with('(');
        let bare = w.trim_matches(|c: char| c == '(' || c == ')' || c == '-' || c == ',');
        if bare.len() == 4 {
            if let Ok(y) = bare.parse::<u32>() {
                if (1900..=2100).contains(&y) && !(year_at.is_some_and(|p| p.2) && !bracketed) {
                    year_at = Some((i, y, bracketed));
                }
            }
        }
    }
    let year = year_at.map(|y| y.1);
    let title_words = &words[..year_at.map(|y| y.0).unwrap_or(end)];
    let title = title_words
        .join(" ")
        .trim_matches(|c: char| c == '-' || c == ' ' || c == '(')
        .to_string();
    let title = if title.is_empty() { raw.to_string() } else { title };
    (title, year)
}

/// Finds season and episode numbers: S01E02, s1e2, 1x02, "Episode 5".
pub fn parse_episode(name: &str) -> Option<(u32, u32, usize, usize)> {
    let b = name.as_bytes();
    let lower = name.to_ascii_lowercase();
    let lb = lower.as_bytes();
    // SxxEyy
    for i in 0..lb.len() {
        if lb[i] == b's' && (i == 0 || !lb[i - 1].is_ascii_alphabetic()) {
            let mut j = i + 1;
            let s0 = j;
            while j < lb.len() && lb[j].is_ascii_digit() && j - s0 < 4 {
                j += 1;
            }
            if j == s0 || j >= lb.len() {
                continue;
            }
            let season: u32 = lower[s0..j].parse().ok()?;
            let mut k = j;
            if k < lb.len() && (lb[k] == b' ' || lb[k] == b'.' || lb[k] == b'_') {
                k += 1;
            }
            if k < lb.len() && lb[k] == b'e' {
                let e0 = k + 1;
                let mut e1 = e0;
                while e1 < lb.len() && lb[e1].is_ascii_digit() && e1 - e0 < 4 {
                    e1 += 1;
                }
                if e1 > e0 {
                    let ep: u32 = lower[e0..e1].parse().ok()?;
                    // Skip a second episode in multi-episode files (S01E01E02, S01E01-E02).
                    let mut end = e1;
                    while end < lb.len()
                        && (lb[end] == b'e' || lb[end] == b'-' || lb[end].is_ascii_digit())
                        && end - e1 < 6
                    {
                        end += 1;
                    }
                    return Some((season, ep, i, end));
                }
            }
        }
    }
    // 1x02
    for i in 1..b.len().saturating_sub(1) {
        if lb[i] == b'x' && lb[i - 1].is_ascii_digit() && lb[i + 1].is_ascii_digit() {
            let mut s0 = i;
            while s0 > 0 && lb[s0 - 1].is_ascii_digit() && i - s0 < 2 {
                s0 -= 1;
            }
            if s0 > 0 && lb[s0 - 1].is_ascii_alphanumeric() {
                continue;
            }
            let mut e1 = i + 1;
            while e1 < lb.len() && lb[e1].is_ascii_digit() && e1 - i < 4 {
                e1 += 1;
            }
            if e1 < lb.len() && lb[e1].is_ascii_alphanumeric() {
                continue;
            }
            let season = lower[s0..i].parse().ok()?;
            let ep = lower[i + 1..e1].parse().ok()?;
            return Some((season, ep, s0, e1));
        }
    }
    // "Episode 5" / "Folge 5" (season from the folder)
    for key in ["episode ", "ep ", "folge "] {
        if let Some(p) = lower.find(key) {
            let start = p + key.len();
            let digits: String = lower[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(ep) = digits.parse() {
                return Some((0, ep, p, start + digits.len()));
            }
        }
    }
    None
}

fn season_from_folder(name: &str) -> Option<u32> {
    let lower = name.to_lowercase();
    for key in ["season", "staffel", "series", "s"] {
        if let Some(rest) = lower.strip_prefix(key) {
            let d: String = rest.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = d.parse() {
                return Some(n);
            }
        }
    }
    if lower == "specials" {
        return Some(0);
    }
    None
}

fn tidy(s: &str) -> String {
    let s = if s.contains(' ') { s.to_string() } else { s.replace(['.', '_'], " ") };
    s.trim_matches(|c: char| c == '-' || c == ' ' || c == '.' || c == '_')
        .to_string()
}

/// Reads container, duration and every stream with ffprobe. Returns false
/// when ffprobe is missing or cannot read the file.
pub fn probe(ffprobe: &str, file: &mut MediaFile) -> bool {
    let out = Command::new(ffprobe)
        .args(["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters"])
        .arg(&file.path)
        .output();
    let Ok(out) = out else { return false };
    if !out.status.success() {
        return false;
    }
    let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) else {
        return false;
    };
    let fmt = &v["format"];
    file.format_name = fmt["format_name"].as_str().unwrap_or("").to_string();
    file.duration_ms = fmt["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .map(|d| (d * 1000.0) as u64)
        .unwrap_or(0);
    file.bitrate = fmt["bit_rate"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0);
    file.streams.clear();
    if let Some(streams) = v["streams"].as_array() {
        for s in streams {
            let kind = s["codec_type"].as_str().unwrap_or("");
            let kind = match kind {
                "video" if s["disposition"]["attached_pic"].as_i64() == Some(1) => continue,
                "video" | "audio" | "subtitle" => kind,
                _ => continue,
            };
            let pix = s["pix_fmt"].as_str().unwrap_or("");
            let bit_depth = s["bits_per_raw_sample"]
                .as_str()
                .and_then(|b| b.parse().ok())
                .unwrap_or(if pix.contains("10") {
                    10
                } else if pix.contains("12") {
                    12
                } else if kind == "video" {
                    8
                } else {
                    0
                });
            file.streams.push(Stream {
                kind: kind.into(),
                index: s["index"].as_u64().unwrap_or(0) as u32,
                codec: s["codec_name"].as_str().unwrap_or("").to_string(),
                language: s["tags"]["language"].as_str().unwrap_or("").to_string(),
                channels: s["channels"].as_u64().unwrap_or(0) as u32,
                width: s["width"].as_u64().unwrap_or(0) as u32,
                height: s["height"].as_u64().unwrap_or(0) as u32,
                bit_depth,
                default: s["disposition"]["default"].as_i64() == Some(1),
                title: s["tags"]["title"].as_str().unwrap_or("").to_string(),
                profile: s["profile"].as_str().unwrap_or("").to_string(),
                color_transfer: s["color_transfer"].as_str().unwrap_or("").to_string(),
                hdr10plus: false,
                dovi: s["side_data_list"].to_string().contains("DOVI")
                    || matches!(s["codec_tag_string"].as_str().unwrap_or(""), "dvhe" | "dvh1" | "dav1" | "dva1" | "dvav"),
                channel_layout: s["channel_layout"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    // Credits markers: a
    // chapter whose name says credits marks where they start.
    file.credits_ms = v["chapters"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|c| is_credits_title(c["tags"]["title"].as_str().unwrap_or("")))
                .filter_map(|c| c["start_time"].as_str().and_then(|t| t.parse::<f64>().ok()))
                .map(|t| (t * 1000.0) as u64)
                .collect()
        })
        .unwrap_or_default();
    // HDR10+ metadata sits in the frames, not the stream header: look at the
    // first frame of PQ video only.
    if let Some(v) = file.streams.iter_mut().find(|s| s.kind == "video" && s.color_transfer == "smpte2084") {
        let frames = Command::new(ffprobe)
            .args(["-v", "quiet", "-print_format", "json", "-select_streams", "v:0", "-read_intervals", "%+#1", "-show_frames", "-show_entries", "frame=side_data_list"])
            .arg(&file.path)
            .output();
        if let Ok(o) = frames {
            v.hdr10plus = String::from_utf8_lossy(&o.stdout).contains("2094-40");
        }
    }
    file.probed = true;
    file.probe_version = crate::db::PROBE_VERSION;
    true
}

/// Reads only the chapters of a file and returns where credits start (ms).
/// None when ffprobe can't read the file.
pub fn read_credits(ffprobe: &str, path: &str) -> Option<Vec<u64>> {
    let out = Command::new(ffprobe).args(["-v", "quiet", "-print_format", "json", "-show_chapters"]).arg(path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(
        v["chapters"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|c| is_credits_title(c["tags"]["title"].as_str().unwrap_or("")))
                    .filter_map(|c| c["start_time"].as_str().and_then(|t| t.parse::<f64>().ok()))
                    .map(|t| (t * 1000.0) as u64)
                    .collect()
            })
            .unwrap_or_default(),
    )
}

// ------------------------------------------------------------------ credits in the picture
//
// The end of the video is looked at directly: end credits are
// mostly dark pictures with a little bright text on them, for a while.

/// Small greyscale frames, one per second, of the last part of a video.
/// (160×90: smaller, and the thin letters of credits blur into the background.)
const FRAME_W: usize = 160;
const FRAME_H: usize = 90;

/// What one frame looks like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameKind {
    /// Dark with a little bright text: looks like credits.
    Credits,
    /// (Nearly) black: fits between credit cards.
    Black,
    /// A normal picture.
    Picture,
}

pub fn frame_kind(px: &[u8]) -> FrameKind {
    let n = px.len().max(1) as f32;
    let dark = px.iter().filter(|&&p| p < 40).count() as f32 / n;
    let bright = px.iter().filter(|&&p| p > 100).count() as f32 / n;
    if dark >= 0.9 && bright < 0.003 {
        FrameKind::Black
    } else if dark >= 0.75 && (0.003..=0.2).contains(&bright) {
        FrameKind::Credits
    } else {
        FrameKind::Picture
    }
}

/// The second (counted from the first frame) where the credits begin: the
/// longest stretch of at least `min_len` seconds that is mostly credit-like.
pub fn credits_start(kinds: &[FrameKind], min_len: usize) -> Option<usize> {
    if kinds.len() < min_len {
        return None;
    }
    let v: Vec<f32> = kinds.iter().map(|k| match k { FrameKind::Credits => 1.0, FrameKind::Black => 0.5, FrameKind::Picture => 0.0 }).collect();
    // Average over 9 s, so a single bright frame doesn't break the credits.
    let half = 4usize;
    let smooth: Vec<f32> = (0..v.len())
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half + 1).min(v.len());
            v[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect();
    let mut best: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < smooth.len() {
        if smooth[i] < 0.6 {
            i += 1;
            continue;
        }
        let start = i;
        while i < smooth.len() && smooth[i] >= 0.6 {
            i += 1;
        }
        let len = i - start;
        let has_text = kinds[start..i].iter().filter(|k| **k == FrameKind::Credits).count() * 3 >= len;
        if len >= min_len && has_text && best.is_none_or(|(_, l)| len >= l) {
            best = Some((start, len));
        }
    }
    // Start at the first frame that really shows credits.
    best.map(|(s, l)| (s..s + l).find(|&k| kinds[k] == FrameKind::Credits).unwrap_or(s))
}

/// Looks at the picture of the last part of a video (the last 30 %, between
/// 1 and 15 minutes) and returns where the end credits start (ms), None when
/// there are none, or an error when ffmpeg can't read the file.
pub fn detect_credits_in_video(ffmpeg: &str, path: &str, duration_ms: u64) -> Result<Option<u64>, String> {
    use std::io::Read as _;
    use std::process::Stdio;
    if duration_ms < 60_000 {
        return Ok(None);
    }
    let window = (duration_ms * 3 / 10).clamp(60_000, 15 * 60_000).min(duration_ms);
    let start_ms = duration_ms - window;
    let mut child = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-skip_frame", "nokey", "-ss", &format!("{:.3}", start_ms as f64 / 1000.0), "-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-an", "-sn", "-dn", "-vf", &format!("fps=1,scale={FRAME_W}:{FRAME_H},format=gray"), "-f", "rawvideo", "pipe:1"])
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
    // Only key frames are decoded, so this is quick; still, never wait forever.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
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
    let frame = FRAME_W * FRAME_H;
    if status.is_none() {
        return Err("ffmpeg took too long reading the video".into());
    }
    if buf.len() < frame * 10 {
        return Err("ffmpeg could not read the picture of this video".into());
    }
    let kinds: Vec<FrameKind> = buf.chunks_exact(frame).map(frame_kind).collect();
    Ok(credits_start(&kinds, 20).map(|s| start_ms + s as u64 * 1000))
}

/// Credits for one file: chapters named like "Credits" first, else the
/// picture itself. Returns the markers and how they were found, or an error.
pub fn find_credits(ffmpeg: &str, ffprobe: &str, path: &str, duration_ms: u64, use_picture: bool) -> Result<(Vec<u64>, &'static str), String> {
    let chapters = read_credits(ffprobe, path);
    if let Some(c) = chapters.as_ref().filter(|c| !c.is_empty()) {
        return Ok((c.clone(), "chapters"));
    }
    if !use_picture {
        return chapters.map(|c| (c, "chapters")).ok_or_else(|| "ffprobe could not read this file".into());
    }
    match detect_credits_in_video(ffmpeg, path, duration_ms)? {
        Some(ms) => Ok((vec![ms], "picture")),
        None => Ok((vec![], "picture")),
    }
}

/// Chapter names that mean "end credits" (a few languages).
pub fn is_credits_title(t: &str) -> bool {
    let t = t.to_lowercase();
    ["credits", "credit roll", "end titles", "abspann", "générique", "generique", "créditos", "titoli di coda", "aftiteling"]
        .iter()
        .any(|w| t.contains(w))
}

fn find_poster(dir: &Path, stem: &str) -> Option<String> {
    for ext in ["jpg", "jpeg", "png", "webp"] {
        let own = dir.join(format!("{stem}.{ext}"));
        if own.exists() {
            return Some(own.to_string_lossy().into());
        }
        let own2 = dir.join(format!("{stem}-poster.{ext}"));
        if own2.exists() {
            return Some(own2.to_string_lossy().into());
        }
        for n in POSTER_NAMES {
            let p = dir.join(format!("{n}.{ext}"));
            if p.exists() {
                return Some(p.to_string_lossy().into());
            }
        }
    }
    None
}

fn find_subtitles(dir: &Path, stem: &str) -> Vec<String> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if SUBTITLE_EXTENSIONS.contains(&ext_of(&p).as_str()) && name.starts_with(stem) {
                out.push(p.to_string_lossy().into());
            }
        }
    }
    out.sort();
    out
}

/// What a file turned out to be.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub kind: ItemKind,
    pub title: String,
    pub year: Option<u32>,
    pub show: Option<(String, Option<u32>)>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

pub fn classify(section: &Section, root: &Path, path: &Path) -> Parsed {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let rel = path.strip_prefix(root).unwrap_or(path);
    let comps: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    match section.kind {
        SectionKind::Movie => {
            // "Movie (2008) - pt1": the title without the part marker.
            let (stem, _) = split_part(&stem);
            // Prefer the folder name when the file has a generic name.
            let (mut title, mut year) = parse_title_year(&stem);
            if comps.len() >= 2 {
                let folder = &comps[comps.len() - 2];
                let (ft, fy) = parse_title_year(folder);
                let generic = stem.len() < 4
                    || ["movie", "film", "video", "main", "feature"]
                        .contains(&stem.to_lowercase().as_str());
                if generic || (year.is_none() && fy.is_some()) {
                    title = ft;
                    year = fy.or(year);
                }
            }
            Parsed { kind: ItemKind::Movie, title, year, show: None, season: None, episode: None }
        }
        SectionKind::Show => {
            let show_folder = if comps.len() >= 2 { comps[0].clone() } else { String::new() };
            let season_folder = if comps.len() >= 3 {
                season_from_folder(&comps[comps.len() - 2])
            } else {
                None
            };
            let ep = parse_episode(&stem);
            let (season, episode, title) = match ep {
                Some((s, e, start, end)) => {
                    let season = if s == 0 && season_folder.is_some() { season_folder.unwrap() } else if s == 0 { 1 } else { s };
                    let after = tidy(stem.get(end..).unwrap_or(""));
                    let after = after.trim_start_matches(['-', ' ', '.']).to_string();
                    let (after, _) = parse_title_year(&after);
                    let _ = start;
                    let title = if after.trim().is_empty() { format!("Episode {e}") } else { after };
                    (season, e, title)
                }
                None => (season_folder.unwrap_or(1), 0, tidy(&stem)),
            };
            let show_raw = if !show_folder.is_empty() {
                show_folder
            } else if let Some((_, _, start, _)) = ep {
                tidy(&stem[..start])
            } else {
                tidy(&stem)
            };
            let (show_title, show_year) = parse_title_year(&show_raw);
            Parsed {
                kind: ItemKind::Episode,
                title,
                year: None,
                show: Some((show_title, show_year)),
                season: Some(season),
                episode: Some(episode),
            }
        }
        SectionKind::Other => {
            Parsed { kind: ItemKind::Video, title: tidy(&stem), year: None, show: None, season: None, episode: None }
        }
    }
}

/// Things that changed during a scan, reported to plugins.
#[derive(Default, Debug)]
pub struct ScanReport {
    pub added: Vec<u64>,
    pub updated: Vec<u64>,
    pub removed: Vec<u64>,
    pub files_seen: usize,
}

/// One file to look at, gathered before the lock on the library is taken.
struct Found {
    path: PathBuf,
    root: PathBuf,
    size: u64,
    mtime: u64,
}

/// Scans one library. The slow part (walking folders, ffprobe) happens
/// without holding the library lock; `with_db` is called for short updates.
pub fn scan_section<F>(section: &Section, ffprobe: &str, mut with_db: F) -> ScanReport
where
    F: FnMut(&mut dyn FnMut(&mut Db)),
{
    let mut found = vec![];
    for root in &section.paths {
        let root = PathBuf::from(root);
        let mut files = vec![];
        walk(&root, &mut files, 0);
        for f in files {
            found.push(Found { size: file_size(&f), mtime: file_mtime(&f), path: f, root: root.clone() });
        }
    }
    let mut report = ScanReport { files_seen: found.len(), ..Default::default() };

    // Which files are new or changed?
    let mut known: HashMap<String, (u64, u64, u64)> = HashMap::new();
    // Files read by an older probe: only their streams are read again.
    let mut stale: HashSet<String> = HashSet::new();
    with_db(&mut |db: &mut Db| {
        for it in db.items.values() {
            if it.section_id == section.id {
                if let Some(f) = &it.file {
                    known.insert(f.path.clone(), (it.id, f.size, f.modified));
                    if f.probed && f.probe_version < crate::db::PROBE_VERSION {
                        stale.insert(f.path.clone());
                    }
                }
            }
        }
    });

    let seen: HashSet<String> = found.iter().map(|f| f.path.to_string_lossy().to_string()).collect();
    // Shows and seasons of this library by what find_or_make matches on, so
    // a new episode doesn't search the whole library twice.
    let mut containers: Option<HashMap<ContainerKey, u64>> = None;
    for f in &found {
        let key = f.path.to_string_lossy().to_string();
        let changed = match known.get(&key) {
            Some((_, size, mtime)) => *size != f.size || *mtime != f.mtime,
            None => true,
        };
        if !changed {
            if stale.contains(&key) {
                let mut m = MediaFile { path: key.clone(), ..Default::default() };
                if probe(ffprobe, &mut m) {
                    let id = known[&key].0;
                    with_db(&mut |db: &mut Db| {
                        if let Some(f) = db.items.get_mut(&id).and_then(|i| i.file.as_mut()) {
                            f.streams = m.streams.clone();
                            f.probe_version = m.probe_version;
                        }
                    });
                }
            }
            continue;
        }
        let mut media = MediaFile {
            path: key.clone(),
            size: f.size,
            modified: f.mtime,
            container: ext_of(&f.path),
            ..Default::default()
        };
        probe(ffprobe, &mut media);
        let dir = f.path.parent().unwrap_or(Path::new("."));
        let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        media.external_subtitles = find_subtitles(dir, &stem);
        let poster = find_poster(dir, &stem);
        let parsed = classify(section, &f.root, &f.path);
        let show_poster = if section.kind == SectionKind::Show {
            let rel = f.path.strip_prefix(&f.root).ok();
            rel.and_then(|r| r.components().next())
                .map(|c| f.root.join(c.as_os_str()))
                .and_then(|d| find_poster(&d, "__none__"))
        } else {
            None
        };

        with_db(&mut |db: &mut Db| {
            let t = now();
            let parent = if let Some((show_title, show_year)) = &parsed.show {
                let cache = containers.get_or_insert_with(|| container_index(db, section.id));
                let show_id = find_or_make(db, cache, section.id, ItemKind::Show, None, show_title, *show_year, None, show_poster.clone());
                let season_no = parsed.season.unwrap_or(1);
                let season_title = if season_no == 0 { "Specials".to_string() } else { format!("Season {season_no}") };
                Some(find_or_make(db, cache, section.id, ItemKind::Season, Some(show_id), &season_title, None, Some(season_no), None))
            } else {
                None
            };
            let existing = known.get(&key).map(|k| k.0);
            match existing.and_then(|id| db.items.get_mut(&id)) {
                Some(item) => {
                    let duration = media.duration_ms;
                    item.file = Some(media.clone());
                    item.duration_ms = duration;
                    item.updated_at = t;
                    if !item.locked.contains(&"title".to_string()) {
                        item.title = parsed.title.clone();
                        item.sort_title = sort_title(&parsed.title);
                    }
                    item.parent_id = parent;
                    item.index = parsed.episode;
                    if poster.is_some() {
                        item.poster = poster.clone();
                    }
                    report.updated.push(item.id);
                }
                None => {
                    let id = db.new_id();
                    db.items.insert(
                        id,
                        Item {
                            id,
                            section_id: section.id,
                            kind: parsed.kind,
                            parent_id: parent,
                            title: parsed.title.clone(),
                            sort_title: sort_title(&parsed.title),
                            year: parsed.year,
                            index: parsed.episode,
                            summary: String::new(),
                            tagline: String::new(),
                            genres: vec![],
                            rating: None,
                            content_rating: String::new(),
                            studio: String::new(),
                            added_at: t,
                            updated_at: t,
                            duration_ms: media.duration_ms,
                            file: Some(media.clone()),
                            poster: poster.clone(),
                            extra: Default::default(),
                            locked: vec![],
                        },
                    );
                    report.added.push(id);
                }
            }
        });
    }

    // Remove what disappeared, then empty seasons and shows.
    with_db(&mut |db: &mut Db| {
        let gone: Vec<u64> = db
            .items
            .values()
            .filter(|i| i.section_id == section.id)
            .filter(|i| i.file.as_ref().map(|f| !seen.contains(&f.path)).unwrap_or(false))
            .map(|i| i.id)
            .collect();
        for id in &gone {
            db.items.remove(id);
        }
        report.removed.extend(gone);
        // Which items have children (all libraries: a child is found by its parent only).
        let children = |db: &Db| {
            let mut m: HashMap<u64, Vec<u64>> = HashMap::new();
            for i in db.items.values() {
                if let Some(p) = i.parent_id {
                    m.entry(p).or_default().push(i.id);
                }
            }
            m
        };
        for _ in 0..2 {
            let kids = children(db);
            let empty: Vec<u64> = db
                .items
                .values()
                .filter(|i| i.section_id == section.id && !i.kind.playable() && !kids.contains_key(&i.id))
                .map(|i| i.id)
                .collect();
            for id in empty {
                db.items.remove(&id);
            }
        }
        // Shows and seasons take the newest episode's date, so they rise in
        // "recently added", and the length of all their episodes.
        let kids = children(db);
        fn below(db: &Db, kids: &HashMap<u64, Vec<u64>>, id: u64, newest: &mut Option<u64>, total: &mut u64) {
            for c in kids.get(&id).into_iter().flatten() {
                let Some(it) = db.items.get(c) else { continue };
                if it.kind.playable() {
                    *newest = (*newest).max(Some(it.added_at));
                    *total += it.duration_ms;
                } else {
                    below(db, kids, *c, newest, total);
                }
            }
        }
        let containers: Vec<u64> = db
            .items
            .values()
            .filter(|i| i.section_id == section.id && !i.kind.playable())
            .map(|i| i.id)
            .collect();
        for id in containers {
            let (mut newest, mut total) = (None, 0u64);
            below(db, &kids, id, &mut newest, &mut total);
            let Some(it) = db.items.get(&id) else { continue };
            let updated = newest.map_or(it.updated_at, |n| it.updated_at.max(n));
            // Only what really changed is marked for saving.
            if updated != it.updated_at || total != it.duration_ms {
                if let Some(it) = db.items.get_mut(&id) {
                    it.updated_at = updated;
                    it.duration_ms = total;
                }
            }
        }
        if let Some(s) = db.sections.iter_mut().find(|s| s.id == section.id) {
            s.scanned_at = now();
        }
    });
    report
}

#[allow(clippy::too_many_arguments)]
fn find_or_make(
    db: &mut Db,
    cache: &mut HashMap<ContainerKey, u64>,
    section: u64,
    kind: ItemKind,
    parent: Option<u64>,
    title: &str,
    year: Option<u32>,
    index: Option<u32>,
    poster: Option<String>,
) -> u64 {
    let key = sort_title(title);
    let matches = |i: &Item| {
        i.section_id == section
            && i.kind == kind
            && i.parent_id == parent
            && match kind {
                ItemKind::Season => i.index == index,
                _ => i.sort_title == key,
            }
    };
    let ck = container_key(kind, parent, index, &key);
    // The cached id is checked against the item itself (it may have been
    // renamed or removed since); only then is the whole library searched.
    let cached = cache.get(&ck).and_then(|id| db.items.get(id)).filter(|i| matches(i));
    let found = cached.or_else(|| db.items.values().find(|i| matches(i)));
    if let Some(it) = found {
        cache.insert(ck, it.id);
        let id = it.id;
        if it.poster.is_none() && poster.is_some() {
            if let Some(it) = db.items.get_mut(&id) {
                it.poster = poster;
            }
        }
        return id;
    }
    let id = db.new_id();
    let t = now();
    db.items.insert(
        id,
        Item {
            id,
            section_id: section,
            kind,
            parent_id: parent,
            title: title.to_string(),
            sort_title: key,
            year,
            index,
            summary: String::new(),
            tagline: String::new(),
            genres: vec![],
            rating: None,
            content_rating: String::new(),
            studio: String::new(),
            added_at: t,
            updated_at: t,
            duration_ms: 0,
            file: None,
            poster,
            extra: Default::default(),
            locked: vec![],
        },
    );
    cache.insert(ck, id);
    id
}

/// What find_or_make tells shows and seasons apart by: a season by its
/// number under its show, anything else by its sort title.
type ContainerKey = (&'static str, Option<u64>, Option<u32>, String);

fn container_key(kind: ItemKind, parent: Option<u64>, index: Option<u32>, sort_title: &str) -> ContainerKey {
    match kind {
        ItemKind::Season => (kind.as_str(), parent, index, String::new()),
        _ => (kind.as_str(), parent, None, sort_title.to_string()),
    }
}

/// The shows and seasons of a library, by their [`ContainerKey`].
fn container_index(db: &Db, section: u64) -> HashMap<ContainerKey, u64> {
    let mut m = HashMap::new();
    for i in db.items.values().filter(|i| i.section_id == section && !i.kind.playable()) {
        // The first match wins, as in a search through the library.
        m.entry(container_key(i.kind, i.parent_id, i.index, &i.sort_title)).or_insert(i.id);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plex_split_parts_and_id_tags() {
        assert_eq!(split_part("The Dark Knight (2008) - pt1"), ("The Dark Knight (2008)".into(), Some(1)));
        assert_eq!(split_part("The Dark Knight (2008) - cd2"), ("The Dark Knight (2008)".into(), Some(2)));
        assert_eq!(split_part("Movie.2008.part3"), ("Movie.2008".into(), Some(3)));
        assert_eq!(split_part("Deathly Hallows Part 2"), ("Deathly Hallows Part 2".into(), None));
        assert_eq!(split_part("Apt 2"), ("Apt 2".into(), None));
        assert_eq!(split_part("Script1"), ("Script1".into(), None));
        assert_eq!(id_tag("Batman Begins (2005) {imdb-tt0372784}"), Some(("imdb", "tt0372784".into())));
        assert_eq!(id_tag("Avatar (2009) {edition-Extended} {tmdb-19995}"), Some(("tmdb", "19995".into())));
        assert_eq!(id_tag("Show [tvdbid=81189]"), Some(("tvdb", "81189".into())));
        assert_eq!(id_tag("Film {imdb-12}"), None);
        assert_eq!(id_tag("Film {edition-Final Cut}"), None);
        assert_eq!(parse_title_year("Batman Begins (2005) {imdb-tt0372784}"), ("Batman Begins".into(), Some(2005)));
    }
    #[test]
    fn credits_in_picture() {
        let frame = |dark: u8, text_px: usize| {
            let mut f = vec![dark; FRAME_W * FRAME_H];
            for p in f.iter_mut().take(text_px) {
                *p = 230;
            }
            f
        };
        assert_eq!(frame_kind(&frame(10, 400)), FrameKind::Credits);
        assert_eq!(frame_kind(&frame(5, 0)), FrameKind::Black);
        assert_eq!(frame_kind(&frame(120, 0)), FrameKind::Picture);
        // 60 s of film, 5 s black, 40 s of credits with one bright flash, 10 s black.
        let mut k = vec![FrameKind::Picture; 60];
        k.extend([FrameKind::Black; 5]);
        k.extend([FrameKind::Credits; 20]);
        k.push(FrameKind::Picture);
        k.extend([FrameKind::Credits; 19]);
        k.extend([FrameKind::Black; 10]);
        assert_eq!(credits_start(&k, 20), Some(65));
        // A dark scene of a few seconds is not credits.
        let mut short = vec![FrameKind::Picture; 60];
        short.extend([FrameKind::Credits; 8]);
        short.extend([FrameKind::Picture; 30]);
        assert_eq!(credits_start(&short, 20), None);
    }
    #[test]
    fn titles() {
        assert_eq!(parse_title_year("The.Matrix.1999.1080p.BluRay.x264"), ("The Matrix".into(), Some(1999)));
        assert_eq!(parse_title_year("Blade Runner 2049 (2017)"), ("Blade Runner 2049".into(), Some(2017)));
        assert_eq!(parse_title_year("Up (2009)"), ("Up".into(), Some(2009)));
    }
    #[test]
    fn episodes() {
        assert_eq!(parse_episode("Show - S01E02 - Title").map(|e| (e.0, e.1)), Some((1, 2)));
        assert_eq!(parse_episode("show.s10e05.720p").map(|e| (e.0, e.1)), Some((10, 5)));
        assert_eq!(parse_episode("Show 3x07 Name").map(|e| (e.0, e.1)), Some((3, 7)));
    }
}
