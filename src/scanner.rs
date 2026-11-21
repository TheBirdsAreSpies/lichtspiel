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

/// The file that lists what a scan skips in its folder and below, one pattern
/// per line ("# …" is a comment): "Samples/" or "*.sample.mkv" match a name at
/// any depth, "Extras/2019/*" a path from that folder, "*" any characters but
/// "/", "**" any characters, "?" one. Upper and lower case count the same.
pub const IGNORE_FILE: &str = ".lichtspielignore";

/// The patterns of one ignore file and the folder it applies to.
struct IgnoreRules {
    base: PathBuf,
    patterns: Vec<String>,
}

fn read_ignore(dir: &Path) -> Option<IgnoreRules> {
    let text = std::fs::read_to_string(dir.join(IGNORE_FILE)).ok()?;
    let patterns: Vec<String> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.replace('\\', "/").trim_start_matches("./").trim_start_matches('/').trim_end_matches('/').to_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    Some(IgnoreRules { base: dir.to_path_buf(), patterns })
}

/// Shell-style match: "*" (not across "/"), "**" (across "/"), "?".
pub fn glob_match(pat: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') if p.get(1) == Some(&b'*') => {
                let rest = p[2..].strip_prefix(b"/").unwrap_or(&p[2..]);
                (0..=t.len()).any(|i| go(rest, &t[i..])) || (0..=t.len()).any(|i| go(&p[2..], &t[i..]))
            }
            Some(b'*') => (0..=t.len()).take_while(|&i| i == 0 || t[i - 1] != b'/').any(|i| go(&p[1..], &t[i..])),
            Some(b'?') => !t.is_empty() && t[0] != b'/' && go(&p[1..], &t[1..]),
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    go(pat.as_bytes(), text.as_bytes())
}

fn ignored(rules: &[IgnoreRules], path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    rules.iter().any(|r| {
        let Ok(rel) = path.strip_prefix(&r.base) else { return false };
        let rel = rel.to_string_lossy().replace('\\', "/").to_lowercase();
        r.patterns.iter().any(|p| if p.contains('/') { glob_match(p, &rel) } else { glob_match(p, &name) })
    })
}

/// `live`: the library this walk is a scan of, and how many files earlier folders gave; the progress
/// then says how many videos are found so far and which folder it is in.
/// A file the walk found. Size and change time come with the folder listing, so there is no
/// extra round trip per file (what makes walking a network share slow).
struct Listed {
    path: PathBuf,
    size: u64,
    mtime: u64,
    /// A link to a file somewhere else.
    link: bool,
}

fn mtime_of(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

fn walk(dir: &Path, out: &mut Vec<Listed>, depth: usize, audio: bool, live: Option<(u64, usize)>) {
    walk_in(dir, out, depth, &mut vec![], &mut vec![], audio, live);
}

/// Like `walk`, and lists the ignore files it read (`ignore_files`).
fn walk_in(dir: &Path, out: &mut Vec<Listed>, depth: usize, rules: &mut Vec<IgnoreRules>, ignore_files: &mut Vec<PathBuf>, audio: bool, live: Option<(u64, usize)>) {
    if depth > 12 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    if let Some((section, before)) = live {
        step(section, "listing", before + out.len(), 0, &dir.to_string_lossy());
    }
    let mut entries: Vec<_> = rd.flatten().collect();
    // Only a folder that has an ignore file is asked for it (no failed open in every folder).
    let own = if entries.iter().any(|e| e.file_name() == IGNORE_FILE) { read_ignore(dir) } else { None };
    if own.is_some() {
        ignore_files.push(dir.join(IGNORE_FILE));
    }
    let pushed = own.map(|r| rules.push(r)).is_some();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with("@ea") || name == "#recycle" || name == "@eaDir"
        {
            // Hidden files and Synology's own thumbnail / recycle folders.
            continue;
        }
        let p = e.path();
        if ignored(rules, &p) {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => walk_in(&p, out, depth + 1, rules, ignore_files, audio, live),
            Ok(t) if audio => {
                if crate::music::is_audio(&p) {
                    let m = e.metadata().ok();
                    out.push(Listed { size: m.as_ref().map(|m| m.len()).unwrap_or(0), mtime: m.as_ref().map(mtime_of).unwrap_or(0), path: p, link: t.is_symlink() })
                }
            }
            Ok(t) if is_video(&p) => {
                // Skip release "sample" clips (name is "sample", ends in "-sample"/".sample",
                // or lies in a "Sample" folder), but not films that merely have the word.
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
                let in_sample_dir = dir.file_name().is_some_and(|d| d.to_string_lossy().eq_ignore_ascii_case("sample"));
                let is_sample = stem == "sample" || stem.ends_with("-sample") || stem.ends_with(".sample") || stem.ends_with("_sample") || in_sample_dir;
                let m = e.metadata().ok();
                let size = m.as_ref().map(|m| m.len()).unwrap_or(0);
                if is_sample && size < 300 * 1024 * 1024 {
                    continue;
                }
                out.push(Listed { size, mtime: m.as_ref().map(mtime_of).unwrap_or(0), path: p, link: t.is_symlink() })
            }
            _ => {}
        }
    }
    if pushed {
        rules.pop();
    }
}

/// A fingerprint of the video files (names, sizes, change times) in a
/// library's folders. It changes when a file is added, removed, renamed or is
/// still being copied.
pub fn folder_signature(paths: &[String], audio: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for root in paths {
        let (mut files, mut ignores) = (vec![], vec![]);
        walk_in(Path::new(root), &mut files, 0, &mut vec![], &mut ignores, audio, None);
        // A changed ignore file counts as a change too.
        for f in &files {
            f.path.hash(&mut h);
            f.size.hash(&mut h);
            f.mtime.hash(&mut h);
        }
        for f in &ignores {
            f.hash(&mut h);
            file_size(f).hash(&mut h);
            file_mtime(f).hash(&mut h);
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

/// Words of an edition or a release that are not part of the title, wherever they stand.
const EDITION_WORDS: &[&str] = &["theatrical", "uncut", "remastered", "restored", "imax", "hybrid", "internal", "ac3d", "dubbed", "webhd", "hdrip", "complete", "ws", "fs"];

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
    let s = raw.replace('_', " ");
    // Drop bracketed tags like [1080p] or {edition-Final Cut} but keep (1999).
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
    // Dots are word separators when the name has no spaces (checked after the tags are gone,
    // so a space inside "{edition-Final Cut}" doesn't keep "Apocalypse.Now.1979" in one piece).
    let cleaned = cleaned.trim().to_string();
    let cleaned = if cleaned.contains(' ') { cleaned } else { cleaned.replace('.', " ") };
    let words: Vec<&str> = cleaned.split_whitespace().enumerate()
        .filter(|(i, w)| *i == 0 || !EDITION_WORDS.contains(&w.trim_matches(|c: char| c == '(' || c == ')' || c == '-' || c == ',').to_lowercase().as_str()))
        .map(|(_, w)| w).collect();
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
    // Library-style names put the article last ("Affe im Menschen, Der"): the title is "Der Affe im Menschen".
    let title = crate::metadata::move_article_front(&title).unwrap_or(title);
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
    probe_tags(ffprobe, file).is_some()
}

/// Like [`probe`], and gives back the file's tags (title, artist, album …) too.
pub fn probe_tags(ffprobe: &str, file: &mut MediaFile) -> Option<Value> {
    let out = Command::new(ffprobe)
        .args(["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters"])
        .arg(&file.path)
        .output();
    let Ok(out) = out else { return None };
    if !out.status.success() {
        return None;
    }
    let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) else {
        return None;
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
                forced: subtitle_flags(s).0,
                sdh: subtitle_flags(s).1,
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
    Some(crate::music::tags_of(&v))
}

/// Every chapter of a file: (start ms, end ms, title). Untitled ones are named "Chapter N".
pub fn read_chapters(ffprobe: &str, path: &str) -> Option<Vec<(u64, u64, String)>> {
    let out = Command::new(ffprobe).args(["-v", "quiet", "-print_format", "json", "-show_chapters"]).arg(path).output().ok()?;
    if !out.status.success() { return None; }
    Some(parse_chapters(&serde_json::from_slice::<Value>(&out.stdout).ok()?))
}

/// (forced, for the hard of hearing) of a subtitle stream, from its disposition or its title.
pub fn subtitle_flags(s: &Value) -> (bool, bool) {
    if s["codec_type"].as_str() != Some("subtitle") { return (false, false); }
    let title = s["tags"]["title"].as_str().unwrap_or("").to_lowercase();
    let word = |w: &str| title.split(|c: char| !c.is_alphanumeric()).any(|x| x == w);
    let forced = s["disposition"]["forced"].as_i64() == Some(1) || title.contains("forced") || title.contains("erzwungen") || title.contains("forcé");
    let sdh = s["disposition"]["hearing_impaired"].as_i64() == Some(1) || word("sdh") || word("cc") || title.contains("hearing") || title.contains("schwerhörig") || title.contains("hörgeschädigt");
    (forced, sdh)
}

pub fn parse_chapters(v: &Value) -> Vec<(u64, u64, String)> {
    let secs = |c: &Value, k: &str| c[k].as_str().and_then(|t| t.parse::<f64>().ok()).map(|t| (t * 1000.0).max(0.0) as u64);
    v["chapters"].as_array().map(|a| a.iter().enumerate().filter_map(|(i, c)| {
        let (s, e) = (secs(c, "start_time")?, secs(c, "end_time").unwrap_or(0));
        let title = c["tags"]["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from).unwrap_or_else(|| format!("Chapter {}", i + 1));
        Some((s, e.max(s), title))
    }).collect()).unwrap_or_default()
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
    if duration_ms < 60_000 {
        return Ok(None);
    }
    let full = (duration_ms * 3 / 10).clamp(60_000, 15 * 60_000).min(duration_ms);
    // Most credits are short: look at the last 5 minutes first, and only when the credits reach back to the
    // start of that window (they may be longer) or the video is longer than that, look at the whole window.
    const QUICK: u64 = 5 * 60_000;
    if full > QUICK {
        let start_ms = duration_ms - QUICK;
        match credits_in_window(ffmpeg, path, start_ms)? {
            Some(ms) if ms > start_ms + 30_000 => return Ok(Some(ms)),
            None => return Ok(None),
            Some(_) => {}
        }
    }
    credits_in_window(ffmpeg, path, duration_ms - full)
}

/// The credits start found in the picture from `start_ms` to the end of the file.
fn credits_in_window(ffmpeg: &str, path: &str, start_ms: u64) -> Result<Option<u64>, String> {
    use std::io::Read as _;
    use std::process::Stdio;
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

pub fn find_poster(dir: &Path, stem: &str) -> Option<String> {
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

/// Where plugins keep the subtitles they fetch, for media folders that cannot be written to
/// (`<data>/subtitles`): below it the video's own folder is mirrored. Set at start.
pub static SUBTITLE_STORE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// A video folder's place in the subtitle store: "/media/Movies/X" → "<store>/media/Movies/X",
/// "C:\Movies\X" → "<store>/C/Movies/X".
pub fn store_dir(store: &Path, dir: &Path) -> PathBuf {
    let mut out = store.to_path_buf();
    for c in dir.components() {
        match c {
            std::path::Component::Prefix(p) => {
                let d: String = p.as_os_str().to_string_lossy().chars().filter(|c| !matches!(c, '\\' | '/' | '?' | '.' | ':')).collect();
                if !d.is_empty() { out.push(d); }
            }
            std::path::Component::Normal(n) => out.push(n),
            _ => {}
        }
    }
    out
}

fn find_subtitles(dir: &Path, stem: &str) -> Vec<String> {
    find_subtitles_in(dir, stem, SUBTITLE_STORE.get().map(PathBuf::as_path))
}

/// The subtitle files of a video: next to it, and in its folder of the subtitle store.
fn find_subtitles_in(dir: &Path, stem: &str, store: Option<&Path>) -> Vec<String> {
    let mut out = vec![];
    for d in std::iter::once(dir.to_path_buf()).chain(store.map(|s| store_dir(s, dir))) {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if SUBTITLE_EXTENSIONS.contains(&ext_of(&p).as_str()) && name.starts_with(stem) {
                    out.push(p.to_string_lossy().into());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Looks for the subtitle files of a video file again (after a plugin saved one).
pub fn refresh_subtitles(f: &mut MediaFile, store: &Path) -> bool {
    let p = Path::new(&f.path);
    let (Some(dir), Some(stem)) = (p.parent(), p.file_stem()) else { return false };
    let now = find_subtitles_in(dir, &stem.to_string_lossy(), Some(store));
    if now == f.external_subtitles { return false; }
    f.external_subtitles = now;
    true
}

/// Folders for a movie's extras, as Plex names them, and the kind of extra.
pub const EXTRA_FOLDERS: &[(&str, &str)] = &[
    ("behind the scenes", "behindthescenes"), ("deleted scenes", "deleted"), ("featurettes", "featurette"),
    ("interviews", "interview"), ("scenes", "scene"), ("shorts", "short"), ("trailers", "trailer"),
    ("other", "other"), ("extras", "other"),
];
/// The kind of extra an extras folder holds, by its folder name
/// ("^trailer.?$", "^deleted.?(scene.?)?$", "^behind.?the.?scenes$",
/// "^featurette.?$", "^interview.?$", "^scene.?$", "^short.?$", "^other.?$",
/// "\bextras?\b"); ".?" is any one character, so "Trailers" and "Trailer_" count too.
pub fn extra_folder_kind(name: &str) -> Option<&'static str> {
    let n = name.trim().to_lowercase();
    // The pattern's words in order, with at most one character of any kind
    // between them (and after the last one when it ends in "").
    fn m(s: &str, parts: &[&str]) -> bool {
        let Some(rest) = s.strip_prefix(parts[0]) else { return false };
        if parts.len() == 1 {
            return rest.is_empty();
        }
        if m(rest, &parts[1..]) {
            return true;
        }
        let mut c = rest.chars();
        c.next().is_some() && m(c.as_str(), &parts[1..])
    }
    let found = [
        (&["trailer", ""][..], "trailer"),
        (&["deleted", ""][..], "deleted"),
        (&["deleted", "scene", ""][..], "deleted"),
        (&["behind", "the", "scenes"][..], "behindthescenes"),
        (&["featurette", ""][..], "featurette"),
        (&["interview", ""][..], "interview"),
        (&["scene", ""][..], "scene"),
        (&["short", ""][..], "short"),
        (&["other", ""][..], "other"),
    ];
    if let Some((_, kind)) = found.iter().find(|(parts, _)| m(&n, parts)) {
        return Some(kind);
    }
    // "\bextras?\b": the word "extra" or "extras" anywhere in the name.
    n.split(|c: char| !c.is_alphanumeric()).any(|w| w == "extra" || w == "extras").then_some("other")
}

/// File name endings for extras next to the movie ("Avatar (2009)-trailer.mkv").
pub const EXTRA_SUFFIXES: &[&str] = &["behindthescenes", "deleted", "featurette", "interview", "scene", "short", "trailer", "other"];

/// A file that looks like an extra of a movie.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraFile {
    /// trailer, behindthescenes, deleted, featurette, interview, scene, short, other
    pub kind: &'static str,
    /// The folder the movie it belongs to is in.
    pub movie_dir: PathBuf,
    /// Its own name, shown on the movie's page.
    pub title: String,
    /// For "Movie (2009)-trailer": "Movie (2009)", to tell movies in one folder apart.
    pub prefix: Option<String>,
}

/// Whether a file is an extra: it lies in an extras folder inside a movie's
/// folder ("Avatar (2009)/Trailers/Teaser.mkv"), or its name ends in an extra
/// word ("Avatar (2009)-trailer.mkv"). An extras folder right in the library
/// folder does not count (that may be a folder of movies called "Shorts").
pub fn extra_of(root: &Path, path: &Path) -> Option<ExtraFile> {
    let rel = path.strip_prefix(root).ok()?;
    let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let stem = path.file_stem()?.to_string_lossy().to_string();
    for i in 1..comps.len().saturating_sub(1) {
        let name = comps[i].to_lowercase();
        if let Some(kind) = EXTRA_FOLDERS.iter().find(|(f, _)| *f == name).map(|x| x.1).or_else(|| extra_folder_kind(&name)) {
            let movie_dir = comps[..i].iter().fold(root.to_path_buf(), |p, c| p.join(c));
            return Some(ExtraFile { kind, movie_dir, title: tidy(&stem), prefix: None });
        }
    }
    let lower = stem.to_lowercase();
    // A file called just "trailer" or "movie-trailer" is the trailer of the
    // movie in the same folder.
    if lower == "trailer" || lower == "movie-trailer" {
        return Some(ExtraFile { kind: "trailer", movie_dir: path.parent()?.to_path_buf(), title: "Trailer".into(), prefix: None });
    }
    for kind in EXTRA_SUFFIXES {
        let Some(before) = lower.strip_suffix(kind).and_then(|b| b.strip_suffix('-')) else { continue };
        let prefix = stem[..before.len()].trim_end().to_string();
        if prefix.is_empty() {
            continue;
        }
        let title = match *kind {
            "trailer" => "Trailer",
            "behindthescenes" => "Behind the scenes",
            "deleted" => "Deleted scene",
            "featurette" => "Featurette",
            "interview" => "Interview",
            "scene" => "Scene",
            "short" => "Short",
            _ => "Extra",
        };
        return Some(ExtraFile { kind, movie_dir: path.parent()?.to_path_buf(), title: title.into(), prefix: Some(prefix) });
    }
    None
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
    /// A song: who, which album, which number.
    pub music: Option<crate::music::MusicInfo>,
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
            Parsed { kind: ItemKind::Movie, title, year, show: None, season: None, episode: None, music: None }
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
                music: None,
            }
        }
        SectionKind::Other => {
            Parsed { kind: ItemKind::Video, title: tidy(&stem), year: None, show: None, season: None, episode: None, music: None }
        }
        SectionKind::Music => {
            let m = crate::music::info(root, path, None);
            Parsed { kind: ItemKind::Track, title: m.title.clone(), year: m.year, show: None, season: None, episode: m.track, music: Some(m) }
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

/// How far a running scan is, per library: shown in the top bar of the web app.
#[derive(Clone, Default)]
pub struct ScanStep {
    /// "listing" (walking the folders; `done` = videos found so far, `current` = the folder), "reading" (file by file) or "finishing".
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
    /// The file it is on.
    pub current: String,
}
static PROGRESS: std::sync::Mutex<std::collections::BTreeMap<u64, ScanStep>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

pub fn progress_of(section: u64) -> Option<ScanStep> {
    PROGRESS.lock().unwrap().get(&section).cloned()
}

fn step(section: u64, phase: &'static str, done: usize, total: usize, current: &str) {
    PROGRESS.lock().unwrap().insert(section, ScanStep { phase, done, total, current: current.to_string() });
}

/// Takes the library out of the progress list when its scan ends (also on an early return).
struct ProgressGuard(u64);
impl Drop for ProgressGuard {
    fn drop(&mut self) {
        PROGRESS.lock().unwrap().remove(&self.0);
    }
}

/// Scans one library. The slow part (walking folders, ffprobe) happens
/// without holding the library lock; `with_db` is called for short updates.
/// (The server scans through `scan_section_in`; this whole-library form is what the tests use.)
#[cfg(test)]
pub fn scan_section<F>(section: &Section, ffprobe: &str, with_db: F) -> ScanReport
where
    F: FnMut(&mut dyn FnMut(&mut Db)),
{
    scan_section_in(section, ffprobe, None, with_db)
}

/// Like [`scan_section`], but with `only` set just that folder (inside one of the library's folders) is
/// looked at: files elsewhere are neither added nor taken out.
pub fn scan_section_in<F>(section: &Section, ffprobe: &str, only: Option<&Path>, mut with_db: F) -> ScanReport
where
    F: FnMut(&mut dyn FnMut(&mut Db)),
{
    let _progress = ProgressGuard(section.id);
    step(section.id, "listing", 0, 0, "");
    let mut found = vec![];
    // The same folders the server sends files from (all libraries), so links between libraries keep working.
    let mut folders: Vec<String> = vec![];
    with_db(&mut |db: &mut Db| folders = db.sections.iter().flat_map(|s| s.paths.clone()).collect());
    for root in &section.paths {
        let root = PathBuf::from(root);
        if only.is_some_and(|o| !o.starts_with(&root)) { continue; }
        let mut files = vec![];
        walk(only.unwrap_or(&root), &mut files, 0, section.kind == SectionKind::Music, Some((section.id, found.len())));
        // A link to a file outside the libraries would let anyone who can see the library read that file.
        for f in files {
            if f.link && !crate::server::inside_folders(&folders, &f.path) { elog!("{} is a link to a file outside the libraries and is skipped", f.path.display()); continue; }
            found.push(Found { size: f.size, mtime: f.mtime, path: f.path, root: root.clone() });
        }
    }
    let mut report = ScanReport { files_seen: found.len(), ..Default::default() };

    // Which files are new or changed?
    let mut known: HashMap<String, (u64, u64, u64)> = HashMap::new();
    // The further parts each known movie has (path, size, change time).
    let mut known_parts: HashMap<u64, Vec<(String, u64, u64)>> = HashMap::new();
    // Known extras: item id -> (kind of extra, movie it belongs to).
    let mut known_extras: HashMap<u64, (String, Option<u64>)> = HashMap::new();
    // Files read by an older probe: only their streams are read again.
    let mut stale: HashSet<String> = HashSet::new();
    with_db(&mut |db: &mut Db| {
        for it in db.items.values() {
            if it.section_id == section.id {
                if !it.parts.is_empty() {
                    known_parts.insert(it.id, it.parts.iter().map(|f| (f.path.clone(), f.size, f.modified)).collect());
                }
                if it.is_extra() {
                    known_extras.insert(it.id, (it.extra["extra_type"].as_str().unwrap_or("").to_string(), it.parent_id));
                }
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
    let key_of = |f: &Found| f.path.to_string_lossy().to_string();

    // Movies split over files ("… - pt1", "… - pt2" in one folder) become one
    // movie: the first part is its file, the others are its further parts.
    let mut parts_of: HashMap<String, Vec<usize>> = HashMap::new();
    let mut later_parts: HashSet<String> = HashSet::new();
    let mut extras: Vec<(usize, ExtraFile)> = vec![];
    let mut main: Vec<usize> = vec![];
    {
        let mut groups: HashMap<(PathBuf, String), Vec<(u32, usize)>> = HashMap::new();
        for (i, f) in found.iter().enumerate() {
            if matches!(section.kind, SectionKind::Movie | SectionKind::Show) {
                if let Some(x) = extra_of(&f.root, &f.path) {
                    // TV extras are left out (they would turn into odd episodes).
                    if section.kind == SectionKind::Movie {
                        extras.push((i, x));
                    }
                    continue;
                }
            }
            main.push(i);
            if section.kind == SectionKind::Movie {
                let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if let (base, Some(n)) = split_part(&stem) {
                    let dir = f.path.parent().unwrap_or(Path::new("")).to_path_buf();
                    groups.entry((dir, base.to_lowercase())).or_default().push((n, i));
                }
            }
        }
        for (_, mut g) in groups {
            if g.len() < 2 {
                continue;
            }
            g.sort();
            // Plex stacks up to 8 parts.
            g.truncate(8);
            let first = key_of(&found[g[0].1]);
            for (_, i) in &g[1..] {
                later_parts.insert(key_of(&found[*i]));
            }
            parts_of.insert(first, g[1..].iter().map(|(_, i)| *i).collect());
        }
        main.retain(|i| !later_parts.contains(&key_of(&found[*i])));
    }

    let total_files = main.len() + extras.len();
    let mut done_files = 0usize;
    step(section.id, "reading", 0, total_files, "");

    // Shows and seasons of this library by what find_or_make matches on, so
    // a new episode doesn't search the whole library twice.
    let mut containers: Option<HashMap<ContainerKey, u64>> = None;
    // First the movies, shows and videos; then the extras, which need the movie
    // they belong to. An extra whose movie isn't found is read as a movie itself.
    for pass in 0..2 {
        let list: Vec<(usize, Option<(&'static str, u64, String)>)> = if pass == 0 {
            main.iter().map(|i| (*i, None)).collect()
        } else {
            let mut v = vec![];
            // The library's movies by folder (id, file name without the part), in id order: one pass, not one per extra.
            let mut by_dir: HashMap<PathBuf, Vec<(u64, String)>> = HashMap::new();
            with_db(&mut |db: &mut Db| {
                for m in db.items.values().filter(|m| m.section_id == section.id && m.kind == ItemKind::Movie) {
                    let Some(f) = &m.file else { continue };
                    let p = Path::new(&f.path);
                    let Some(dir) = p.parent() else { continue };
                    let stem = p.file_stem().map(|s| split_part(&s.to_string_lossy()).0.to_lowercase()).unwrap_or_default();
                    by_dir.entry(dir.to_path_buf()).or_default().push((m.id, stem));
                }
            });
            for (i, x) in &extras {
                let movies = by_dir.get(&x.movie_dir).map(Vec::as_slice).unwrap_or_default();
                let parent = match &x.prefix {
                    Some(pre) => movies.iter().find(|m| m.1 == pre.to_lowercase()).or(if movies.len() == 1 { movies.first() } else { None }),
                    None => movies.first(),
                };
                v.push((*i, parent.map(|m| (x.kind, m.0, x.title.clone()))));
            }
            v
        };
        for (idx, as_extra) in list {
            let f = &found[idx];
            done_files += 1;
            step(section.id, "reading", done_files, total_files, &f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            let key = key_of(f);
            let parts_here: Vec<&Found> = parts_of.get(&key).map(|v| v.iter().map(|i| &found[*i]).collect()).unwrap_or_default();
            let existing_id = known.get(&key).map(|k| k.0);
            let mut changed = match known.get(&key) {
                Some((_, size, mtime)) => *size != f.size || *mtime != f.mtime,
                None => true,
            };
            if let Some(id) = existing_id {
                // The parts changed (one added, removed or replaced)?
                let now_parts: Vec<(String, u64, u64)> = parts_here.iter().map(|p| (key_of(p), p.size, p.mtime)).collect();
                if known_parts.get(&id).cloned().unwrap_or_default() != now_parts {
                    changed = true;
                }
                // Became an extra, or stopped being one?
                match (&as_extra, known_extras.get(&id)) {
                    (Some((kind, parent, _)), Some((k, p))) => {
                        if *kind != k.as_str() || Some(*parent) != *p {
                            let (kind, parent) = (*kind, *parent);
                            with_db(&mut |db: &mut Db| {
                                if let Some(it) = db.items.get_mut(&id) {
                                    it.parent_id = Some(parent);
                                    it.extra.insert("extra_type".into(), Value::from(kind));
                                }
                            });
                        }
                    }
                    (None, None) => {}
                    _ => changed = true,
                }
            }
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
            let read = |f: &Found| {
                let mut media = MediaFile {
                    path: f.path.to_string_lossy().to_string(),
                    size: f.size,
                    modified: f.mtime,
                    container: ext_of(&f.path),
                    ..Default::default()
                };
                probe(ffprobe, &mut media);
                media
            };
            let mut music_tags = None;
            let mut media = if section.kind == SectionKind::Music {
                let mut m = MediaFile { path: f.path.to_string_lossy().to_string(), size: f.size, modified: f.mtime, container: ext_of(&f.path), ..Default::default() };
                music_tags = probe_tags(ffprobe, &mut m);
                m
            } else { read(f) };
            let parts: Vec<MediaFile> = parts_here.iter().map(|p| read(p)).collect();
            let duration = media.duration_ms + parts.iter().map(|p| p.duration_ms).sum::<u64>();
            let dir = f.path.parent().unwrap_or(Path::new("."));
            let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            media.external_subtitles = find_subtitles(dir, &stem);
            let poster = if as_extra.is_some() { None } else { find_poster(dir, &stem) };
            let parsed = match &as_extra {
                Some((_, _, title)) => Parsed { kind: ItemKind::Video, title: title.clone(), year: None, show: None, season: None, episode: None, music: None },
                None if section.kind == SectionKind::Music => {
                    let m = crate::music::info(&f.root, &f.path, music_tags.as_ref());
                    Parsed { kind: ItemKind::Track, title: m.title.clone(), year: m.year, show: None, season: None, episode: m.track, music: Some(m) }
                }
                None => classify(section, &f.root, &f.path),
            };
            // Cover pictures: the album's folder, and the artist's folder one level up.
            let (album_cover, artist_cover) = if section.kind == SectionKind::Music {
                let rel = f.path.strip_prefix(&f.root).ok();
                let artist_dir = rel.filter(|r| r.components().count() >= 3).and_then(|r| r.components().next()).map(|c| f.root.join(c.as_os_str()));
                (find_poster(dir, "__none__"), artist_dir.and_then(|d| find_poster(&d, "__none__")))
            } else { (None, None) };
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
                let parent = if let Some((_, movie, _)) = &as_extra {
                    Some(*movie)
                } else if let Some(m) = &parsed.music {
                    let cache = containers.get_or_insert_with(|| container_index(db, section.id));
                    let artist = find_or_make(db, cache, section.id, ItemKind::Artist, None, &m.artist, None, None, artist_cover.clone());
                    Some(find_or_make(db, cache, section.id, ItemKind::Album, Some(artist), &m.album, m.year, None, album_cover.clone()))
                } else if let Some((show_title, show_year)) = &parsed.show {
                    let cache = containers.get_or_insert_with(|| container_index(db, section.id));
                    let show_id = find_or_make(db, cache, section.id, ItemKind::Show, None, show_title, *show_year, None, show_poster.clone());
                    let season_no = parsed.season.unwrap_or(1);
                    let season_title = if season_no == 0 { "Specials".to_string() } else { format!("Season {season_no}") };
                    Some(find_or_make(db, cache, section.id, ItemKind::Season, Some(show_id), &season_title, None, Some(season_no), None))
                } else {
                    None
                };
                let id = match existing_id.and_then(|id| db.items.get_mut(&id)) {
                    Some(item) => {
                        item.file = Some(media.clone());
                        item.parts = parts.clone();
                        item.duration_ms = duration;
                        item.updated_at = t;
                        item.kind = parsed.kind;
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
                        item.id
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
                                duration_ms: duration,
                                file: Some(media.clone()),
                                parts: parts.clone(),
                                poster: poster.clone(),
                                extra: Default::default(),
                                locked: vec![],
                            },
                        );
                        report.added.push(id);
                        id
                    }
                };
                if let (Some(m), Some(it)) = (&parsed.music, db.items.get_mut(&id)) {
                    it.genres = m.genres.clone();
                    match m.disc { Some(d) => it.extra.insert("disc".into(), Value::from(d)), None => it.extra.remove("disc") };
                    let album = it.parent_id;
                    // The album takes the year and genres of its songs.
                    if let Some(a) = album.and_then(|a| db.items.get_mut(&a)) {
                        if a.year.is_none() { a.year = m.year; }
                        if a.genres.is_empty() { a.genres = m.genres.clone(); }
                        // A release is a live album, single ... as soon as one song says so.
                        if a.extra.get("release").and_then(|r| r.as_str()).is_none_or(|r| r == "album") { a.extra.insert("release".into(), Value::from(m.release)); }
                    }
                }
                if let Some(it) = db.items.get_mut(&id) {
                    match &as_extra {
                        Some((kind, _, _)) => {
                            it.extra.insert("extra_type".into(), Value::from(*kind));
                        }
                        None => {
                            it.extra.remove("extra_type");
                        }
                    }
                }
            });
        }
    }

    step(section.id, "finishing", total_files, total_files, "");
    // Remove what disappeared (or, with "Empty trash automatically" off, keep it
    // as unavailable), then empty seasons and shows. A library folder that cannot
    // be read (a drive or share not mounted) never empties anything.
    let offline: Vec<PathBuf> = section.paths.iter().map(PathBuf::from)
        .filter(|r| std::fs::read_dir(r).map(|mut d| d.next().is_none()).unwrap_or(true))
        .collect();
    with_db(&mut |db: &mut Db| {
        // A file swapped for another of the same movie or episode (a better version, a new name): the
        // new file moves into the old title, so watched state, ratings, playlists and details stay.
        for (old, new) in adopt_replacements(db, section.id, &seen, &later_parts, &offline, only) {
            let was_new = report.added.contains(&new);
            report.added.retain(|i| *i != new);
            report.updated.retain(|i| *i != new);
            if !was_new { report.removed.push(new); }
            if !report.updated.contains(&old) { report.updated.push(old); }
        }
        let auto = db.settings.empty_trash_automatically;
        let mut gone = vec![];
        let mut trash = vec![];
        let mut back = vec![];
        for i in db.items.values().filter(|i| i.section_id == section.id) {
            let Some(f) = i.file.as_ref() else { continue };
            if only.is_some_and(|o| !Path::new(&f.path).starts_with(o)) { continue; }
            if later_parts.contains(&f.path) {
                // Now a later part of a split movie.
                gone.push(i.id);
            } else if !seen.contains(&f.path) {
                let unreachable = offline.iter().any(|r| Path::new(&f.path).starts_with(r));
                if auto && !unreachable { gone.push(i.id) } else if !i.extra.contains_key(MISSING) { trash.push(i.id) }
            } else if i.extra.contains_key(MISSING) {
                back.push(i.id);
            }
        }
        for id in &gone {
            db.items.remove(id);
        }
        for id in &trash {
            if let Some(it) = db.items.get_mut(id) { it.extra.insert(MISSING.into(), Value::from(now())); }
        }
        for id in &back {
            if let Some(it) = db.items.get_mut(id) { it.extra.remove(MISSING); }
        }
        report.removed.extend(gone);
        report.removed.extend(prune_empty(db, section.id));
        // Shows and seasons take the newest episode's date, so they rise in
        // "recently added", and the length of all their episodes.
        let kids = children_map(db);
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
        auto_merge_versions(db, section.id);
        if let Some(s) = db.sections.iter_mut().find(|s| s.id == section.id) {
            s.scanned_at = now();
        }
    });
    report
}

/// Titles whose file is gone (not in `seen`, on a reachable folder) that another title of the
/// library now stands in for: a movie with the same title and year (or the only other movie in the
/// gone file's own folder), an episode with the same season and number. The other title's files
/// move into the old one and the other title goes; what someone already did with the new one
/// (watched, rated) moves along where the old one has nothing. Returns (old, new) ids.
fn adopt_replacements(db: &mut Db, section: u64, seen: &HashSet<String>, later_parts: &HashSet<String>, offline: &[PathBuf], only: Option<&Path>) -> Vec<(u64, u64)> {
    let roots: Vec<PathBuf> = db.sections.iter().filter(|s| s.id == section).flat_map(|s| s.paths.iter().map(PathBuf::from)).collect();
    let present = |i: &Item| i.file.as_ref().is_some_and(|f| seen.contains(&f.path)) && !i.extra.contains_key(MISSING);
    let candidate = |i: &Item| i.section_id == section && matches!(i.kind, ItemKind::Movie | ItemKind::Episode) && !i.is_extra();
    let olds: Vec<u64> = db.items.values()
        .filter(|i| candidate(i) && i.version_of().is_none())
        .filter(|i| i.file.as_ref().is_some_and(|f| {
            let p = Path::new(&f.path);
            !seen.contains(&f.path) && !later_parts.contains(&f.path) && !p.is_file()
                && only.is_none_or(|o| p.starts_with(o)) && !offline.iter().any(|r| p.starts_with(r))
        }))
        .map(|i| i.id).collect();
    let dir_of = |i: &Item| i.file.as_ref().and_then(|f| Path::new(&f.path).parent().map(Path::to_path_buf));
    let mut out = vec![];
    let mut taken: HashSet<u64> = HashSet::new();
    for old_id in olds {
        let Some(old) = db.items.get(&old_id).cloned() else { continue };
        let others: Vec<&Item> = db.items.values()
            .filter(|i| candidate(i) && i.id != old_id && !taken.contains(&i.id) && present(i) && i.kind == old.kind)
            // A version of a different title is not a stand-in.
            .filter(|i| i.version_of().is_none_or(|v| v == old_id))
            .collect();
        let pick = if old.kind == ItemKind::Episode {
            others.iter().filter(|i| old.parent_id.is_some() && i.parent_id == old.parent_id && old.index.is_some() && i.index == old.index).map(|i| i.id).min()
        } else {
            let same_year = |i: &Item| old.year.is_none() || i.year.is_none() || i.year == old.year;
            let by_title = others.iter().filter(|i| i.title.to_lowercase() == old.title.to_lowercase() && same_year(i)).map(|i| i.id).min();
            // A movie in its own folder (not a library folder itself): the one other movie there.
            let by_dir = || {
                let dir = dir_of(&old).filter(|d| !roots.iter().any(|r| r == d))?;
                let here: Vec<u64> = others.iter().filter(|i| dir_of(i).as_ref() == Some(&dir)).map(|i| i.id).collect();
                (here.len() == 1).then(|| here[0])
            };
            by_title.or_else(by_dir)
        };
        let Some(new_id) = pick else { continue };
        let Some(new) = db.items.remove(&new_id) else { continue };
        taken.insert(new_id);
        if let Some(it) = db.items.get_mut(&old_id) {
            it.file = new.file;
            it.parts = new.parts;
            it.duration_ms = new.duration_ms;
            it.updated_at = now();
            it.extra.remove(MISSING);
            if it.poster.is_none() { it.poster = new.poster; }
        }
        // Its extras and its own versions now belong to the old title.
        let kids: Vec<u64> = db.items.values().filter(|i| i.parent_id == Some(new_id) || i.version_of() == Some(new_id)).map(|i| i.id).collect();
        for k in kids {
            if let Some(it) = db.items.get_mut(&k) {
                if it.parent_id == Some(new_id) { it.parent_id = Some(old_id); }
                if it.version_of() == Some(new_id) { it.extra.insert("version_of".into(), Value::from(old_id)); }
            }
        }
        let suffix = format!(":{new_id}");
        let moved: Vec<String> = db.views.keys().filter(|k| k.ends_with(&suffix)).cloned().collect();
        for k in moved {
            let Some(v) = db.views.remove(&k) else { continue };
            let to = format!("{}:{old_id}", &k[..k.len() - suffix.len()]);
            if db.views.get(&to).is_none() { db.views.insert(to, v); }
        }
        out.push((old_id, new_id));
    }
    out
}

/// Several files of one movie (or one episode) become one title with several versions:
/// the same title and year in a movie library, the same season and episode number in a show. What
/// was split apart by hand (`no_merge`) stays alone, and files with an `{edition-…}` stay titles of
/// their own. Returns how many files were put under another.
fn auto_merge_versions(db: &mut Db, section: u64) -> usize {
    let mut groups: HashMap<(bool, Option<u64>, String, Option<u32>), Vec<u64>> = HashMap::new();
    for i in db.items.values() {
        if i.section_id != section || !matches!(i.kind, ItemKind::Movie | ItemKind::Episode) || i.is_extra() { continue; }
        if i.file.is_none() { continue; }
        if i.extra.contains_key(MISSING) || i.extra.contains_key("no_merge") { continue; }
        if !crate::query::item_edition(i).is_empty() { continue; }
        let key = if i.kind == ItemKind::Movie {
            (true, None, i.title.to_lowercase(), i.year)
        } else {
            let (Some(p), Some(n)) = (i.parent_id, i.index) else { continue };
            (false, Some(p), String::new(), Some(n))
        };
        groups.entry(key).or_default().push(i.id);
    }
    let mut moves: Vec<(u64, u64)> = vec![];
    for (_, mut ids) in groups {
        if ids.len() < 2 { continue; }
        ids.sort();
        let is_main = |id: &u64| db.items.get(id).is_some_and(|i| i.version_of().is_none());
        // The title that already has versions stays the main one; else the oldest.
        let main = ids.iter().copied().find(|m| is_main(m) && ids.iter().any(|o| db.items.get(o).is_some_and(|x| x.version_of() == Some(*m))))
            .or_else(|| ids.iter().copied().find(is_main));
        let Some(main) = main else { continue };
        moves.extend(ids.iter().copied().filter(|id| *id != main && is_main(id)).map(|id| (id, main)));
    }
    let mut n = 0;
    for (id, main) in moves {
        // What it had as versions moves along.
        let moved: Vec<u64> = db.items.values().filter(|x| x.version_of() == Some(id)).map(|x| x.id).collect();
        for i in std::iter::once(id).chain(moved) {
            if let Some(it) = db.items.get_mut(&i) { it.extra.insert("version_of".into(), Value::from(main)); }
        }
        n += 1;
    }
    n
}

/// Marks an item whose file is gone from disk but kept (the library's trash).
pub const MISSING: &str = "missing_since";

/// Which items have children (all libraries: a child is found by its parent only).
fn children_map(db: &Db) -> HashMap<u64, Vec<u64>> {
    let mut m: HashMap<u64, Vec<u64>> = HashMap::new();
    for i in db.items.values() {
        if let Some(p) = i.parent_id {
            m.entry(p).or_default().push(i.id);
        }
    }
    m
}

/// Takes out seasons and shows left without episodes, and extras whose movie
/// is gone. Returns what was taken out.
pub fn prune_empty(db: &mut Db, section: u64) -> Vec<u64> {
    let mut out = vec![];
    let orphans: Vec<u64> = db.items.values()
        .filter(|i| i.section_id == section && i.is_extra() && i.parent_id.is_some_and(|p| !db.items.contains_key(&p)))
        .map(|i| i.id).collect();
    for id in orphans {
        db.items.remove(&id);
        out.push(id);
    }
    // Versions whose main item is gone show up on their own again.
    let loose: Vec<u64> = db.items.values().filter(|i| i.version_of().is_some_and(|p| !db.items.contains_key(&p))).map(|i| i.id).collect();
    for id in loose {
        if let Some(i) = db.items.get_mut(&id) { i.extra.remove("version_of"); }
    }
    for _ in 0..2 {
        let kids = children_map(db);
        let empty: Vec<u64> = db.items.values()
            .filter(|i| i.section_id == section && !i.kind.playable() && !kids.contains_key(&i.id))
            .map(|i| i.id).collect();
        for id in empty {
            db.items.remove(&id);
            out.push(id);
        }
    }
    out
}

/// "Empty trash": takes the unavailable items of a library out for good (those
/// whose file is still not there), then the seasons and shows left empty.
pub fn empty_trash(db: &mut Db, section: u64) -> Vec<u64> {
    let gone: Vec<u64> = db.items.values()
        .filter(|i| i.section_id == section && i.extra.contains_key(MISSING))
        .filter(|i| i.file.as_ref().is_none_or(|f| !Path::new(&f.path).is_file()))
        .map(|i| i.id).collect();
    for id in &gone {
        db.items.remove(id);
    }
    let mut out = gone;
    out.extend(prune_empty(db, section));
    out
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
            parts: vec![],
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
    fn trailing_article_moves_to_the_front() {
        assert_eq!(parse_title_year("Affe im Menschen, Der").0, "Der Affe im Menschen");
        assert_eq!(parse_title_year("Affe im Menschen, Der (1990)"), ("Der Affe im Menschen".to_string(), Some(1990)));
        assert_eq!(parse_title_year("Simpsons, The").0, "The Simpsons");
        assert_eq!(parse_title_year("Hallo, Welt (2001)").0, "Hallo, Welt");
    }

    #[test]
    fn subtitle_kinds_are_told_apart() {
        let j = |t: &str| serde_json::from_str::<Value>(t).unwrap();
        assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","disposition":{"forced":1}}"#)), (true, false));
        assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","disposition":{"hearing_impaired":1}}"#)), (false, true));
        assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","tags":{"title":"English SDH"}}"#)), (false, true));
        assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","tags":{"title":"Deutsch (Erzwungen)"}}"#)), (true, false));
        assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","tags":{"title":"Descriptive"}}"#)), (false, false));
        assert_eq!(subtitle_flags(&j(r#"{"codec_type":"audio","disposition":{"forced":1}}"#)), (false, false));
    }

    #[test]
    fn chapters_are_read_with_titles() {
        let v: Value = serde_json::from_str(r#"{"chapters":[{"start_time":"0.000000","end_time":"60.5","tags":{"title":"Intro"}},{"start_time":"60.5","end_time":"120","tags":{}}]}"#).unwrap();
        assert_eq!(parse_chapters(&v), vec![(0, 60500, "Intro".to_string()), (60500, 120000, "Chapter 2".to_string())]);
    }

    #[test]
    fn chapters_of_a_real_file() {
        let dir = std::env::temp_dir().join("lichtspiel-chapter-test");
        let _ = std::fs::create_dir_all(&dir);
        let meta = dir.join("m.txt");
        std::fs::write(&meta, ";FFMETADATA1
[CHAPTER]
TIMEBASE=1/1000
START=0
END=3000
title=One
[CHAPTER]
TIMEBASE=1/1000
START=3000
END=6000
title=Two
").unwrap();
        let f = dir.join("c.mkv");
        let ok = Command::new("ffmpeg").args(["-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=160x90:rate=10:duration=6", "-i"]).arg(&meta)
            .args(["-map_metadata", "1", "-c:v", "libx264", "-preset", "ultrafast"]).arg(&f).status().map(|s| s.success()).unwrap_or(false);
        if !ok { return; } // no ffmpeg here
        let ch = read_chapters("ffprobe", &f.to_string_lossy()).unwrap();
        assert_eq!(ch.iter().map(|c| (c.0, c.2.as_str())).collect::<Vec<_>>(), vec![(0, "One"), (3000, "Two")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignore_patterns() {
        assert!(glob_match("*.sample.mkv", "film.sample.mkv"));
        assert!(!glob_match("*.mkv", "a/b.mkv"));
        assert!(glob_match("extras/**", "extras/2019/a.mkv"));
        assert!(glob_match("**/b.mkv", "a/x/b.mkv"));
        assert!(glob_match("c?1", "cd1"));
    }

    #[test]
    fn scan_follows_plex_naming() {
        let root = std::env::temp_dir().join(format!("lichtspiel-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files = [
            "Avatar (2009)/Avatar (2009).mkv",
            "Avatar (2009)/Avatar (2009)-trailer.mkv",
            "Avatar (2009)/Featurettes/Making of.mkv",
            "The Dark Knight (2008)/The Dark Knight (2008) - pt2.mkv",
            "The Dark Knight (2008)/The Dark Knight (2008) - pt1.mkv",
            "Shorts/Some Short (2010).mkv",
            "Skip me/Hidden (2001).mkv",
            "Avatar (2009)/junk.sample.mkv.part.mkv",
            "Lonely/Trailers/Teaser (2015).mkv",
        ];
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        std::fs::write(root.join(IGNORE_FILE), "# test\nSkip me/\n*.part.mkv\n").unwrap();
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let section = db.sections[0].clone();
        let scan = |db: &mut Db| { scan_section(&section, "no-such-ffprobe", |f| f(db)); };
        scan(&mut db);
        let titles = |db: &Db| {
            let mut v: Vec<(String, usize, Option<String>)> = db.items.values().map(|i| {
                let parent = i.parent_id.and_then(|p| db.items.get(&p)).map(|p| p.title.clone());
                (format!("{}:{}", if i.is_extra() { i.extra["extra_type"].as_str().unwrap() } else { i.kind.as_str() }, i.title), i.parts.len(), parent)
            }).collect();
            v.sort();
            v
        };
        let want = vec![
            ("featurette:Making of".to_string(), 0, Some("Avatar".to_string())),
            ("movie:Avatar".into(), 0, None),
            ("movie:Some Short".into(), 0, None),
            // An extras folder without a movie next to it: read as a movie.
            ("movie:Teaser".into(), 0, None),
            ("movie:The Dark Knight".into(), 1, None),
            ("trailer:Trailer".into(), 0, Some("Avatar".to_string())),
        ];
        assert_eq!(titles(&db), want);
        let knight = db.items.values().find(|i| i.title == "The Dark Knight").unwrap();
        assert!(knight.file.as_ref().unwrap().path.ends_with("pt1.mkv"));
        // A second scan changes nothing.
        let ids: Vec<u64> = db.items.keys().copied().collect();
        scan(&mut db);
        assert_eq!(titles(&db), want);
        assert_eq!(db.items.keys().copied().collect::<Vec<_>>(), ids);
        // A third part shows up: still one movie, now with two further parts.
        std::fs::write(root.join("The Dark Knight (2008)/The Dark Knight (2008) - pt3.mkv"), b"x").unwrap();
        scan(&mut db);
        assert_eq!(db.items.values().find(|i| i.title == "The Dark Knight").unwrap().parts.len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn files_of_one_movie_become_versions() {
        let root = std::env::temp_dir().join(format!("lichtspiel-versions-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in ["Exit 8 (2025)/Exit.8.2025.1080p.WebRip.mkv", "Exit 8 (2025)/Exit.8.2025.1080p.BD.mkv", "Heat (1995)/Heat (1995).mkv",
                  "Dune (2021)/Dune (2021) {edition-Extended}.mkv", "Dune (2021)/Dune (2021).mkv"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let section = db.sections[0].clone();
        let scan = |db: &mut Db| { scan_section(&section, "no-such-ffprobe", |f| f(db)); };
        scan(&mut db);
        let shown = |db: &Db| { let mut v: Vec<String> = db.items.values().filter(|i| !i.hidden()).map(|i| i.title.clone()).collect(); v.sort(); v };
        // Exit 8 is one title with two versions; Heat is alone; the {edition} file stays a title of its own.
        assert_eq!(shown(&db), vec!["Dune", "Dune", "Exit 8", "Heat"]);
        let main = db.items.values().find(|i| i.title == "Exit 8" && !i.hidden()).unwrap().id;
        assert_eq!(db.items.values().filter(|i| i.version_of() == Some(main)).count(), 1);
        // A second scan changes nothing; one split by hand stays apart.
        scan(&mut db);
        assert_eq!(shown(&db), vec!["Dune", "Dune", "Exit 8", "Heat"]);
        let v = db.items.values().find(|i| i.version_of() == Some(main)).unwrap().id;
        let it = db.items.get_mut(&v).unwrap();
        it.extra.remove("version_of");
        it.extra.insert("no_merge".into(), serde_json::json!(true));
        scan(&mut db);
        assert_eq!(shown(&db), vec!["Dune", "Dune", "Exit 8", "Exit 8", "Heat"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn links_to_files_outside_the_library_are_skipped() {
        let root = std::env::temp_dir().join(format!("lichtspiel-links-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lib = root.join("lib");
        for f in ["Heat (1995)/Heat (1995).mkv", "Inside (2002)/x", "Leak (2001)/x"] {
            std::fs::create_dir_all(lib.join(f).parent().unwrap()).unwrap();
        }
        std::fs::write(lib.join("Heat (1995)/Heat (1995).mkv"), b"x").unwrap();
        std::fs::write(root.join("secret.mkv"), b"x").unwrap();
        std::os::unix::fs::symlink(root.join("secret.mkv"), lib.join("Leak (2001)/Leak (2001).mkv")).unwrap();
        std::os::unix::fs::symlink(lib.join("Heat (1995)/Heat (1995).mkv"), lib.join("Inside (2002)/Inside (2002).mkv")).unwrap();
        // A link into another library is fine: that file can be seen there anyway.
        let other = root.join("other");
        std::fs::create_dir_all(other.join("Kids (2000)")).unwrap();
        std::fs::create_dir_all(lib.join("Curated (2003)")).unwrap();
        std::fs::write(other.join("Kids (2000)/Kids (2000).mkv"), b"x").unwrap();
        std::os::unix::fs::symlink(other.join("Kids (2000)/Kids (2000).mkv"), lib.join("Curated (2003)/Curated (2003).mkv")).unwrap();
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [lib.to_string_lossy()], "created_at": 0},
                {"id": 2, "name": "Andere", "kind": "movie", "paths": [other.to_string_lossy()], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let section = db.sections[0].clone();
        scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
        let mut titles: Vec<String> = db.items.values().map(|i| i.title.clone()).collect();
        titles.sort();
        assert_eq!(titles, vec!["Curated", "Heat", "Inside"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn subtitles_in_the_store_are_found_next_to_those_beside_the_video() {
        let root = std::env::temp_dir().join(format!("lichtspiel-substore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (media, store) = (root.join("media").join("Movies").join("Heat (1995)"), root.join("data").join("subtitles"));
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("Heat (1995).mkv"), b"x").unwrap();
        std::fs::write(media.join("Heat (1995).en.srt"), b"x").unwrap();
        let mirrored = store_dir(&store, &media);
        assert!(mirrored.starts_with(&store) && mirrored.ends_with(Path::new("media").join("Movies").join("Heat (1995)")), "{}", mirrored.display());
        std::fs::create_dir_all(&mirrored).unwrap();
        std::fs::write(mirrored.join("Heat (1995).de.srt"), b"x").unwrap();
        std::fs::write(mirrored.join("Other (2000).de.srt"), b"x").unwrap();
        let found = find_subtitles_in(&media, "Heat (1995)", Some(&store));
        let _ = std::fs::remove_dir_all(&root);
        let names: Vec<String> = found.iter().map(|p| Path::new(p).file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names.len(), 2, "{found:?}");
        assert!(names.contains(&"Heat (1995).de.srt".to_string()) && names.contains(&"Heat (1995).en.srt".to_string()));
        #[cfg(windows)]
        assert_eq!(store_dir(Path::new("S"), Path::new(r"C:\Movies\X")), Path::new("S").join("C").join("Movies").join("X"));
    }

    /// A library in a temporary folder with these files, scanned once, and a scan function.
    fn library_with(name: &str, kind: &str, files: &[&str], auto: bool) -> (PathBuf, Db) {
        let root = std::env::temp_dir().join(format!("lichtspiel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let mut settings = crate::db::Settings::default();
        settings.empty_trash_automatically = auto;
        let db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": settings,
            "sections": [{"id": 1, "name": "L", "kind": kind, "paths": [root.to_string_lossy()], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        (root, db)
    }

    fn watched(db: &mut Db, id: u64) {
        db.views.insert(format!("1:{id}"), crate::db::ViewState { view_count: 1, finished: true, ..Default::default() });
    }

    fn listed(db: &Db, kind: ItemKind) -> Vec<(u64, String, bool)> {
        let mut v: Vec<(u64, String, bool)> = db.items.values().filter(|i| i.kind == kind && i.version_of().is_none())
            .map(|i| (i.id, i.file.as_ref().map(|f| Path::new(&f.path).file_name().unwrap().to_string_lossy().to_string()).unwrap_or_default(), i.extra.contains_key(MISSING))).collect();
        v.sort();
        v
    }

    #[test]
    fn a_movie_file_swapped_for_another_keeps_its_title_and_watched_state() {
        for auto in [false, true] {
            let (root, mut db) = library_with("swap", "movie", &["The Invite (2023)/The Invite (2023).mkv", "Heat (1995).mkv"], auto);
            let section = db.sections[0].clone();
            let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
            scan(&mut db);
            let id = listed(&db, ItemKind::Movie).into_iter().find(|t| t.1.starts_with("The Invite")).unwrap().0;
            watched(&mut db, id);
            // Swapped at once, with another name (a better version).
            std::fs::remove_file(root.join("The Invite (2023)/The Invite (2023).mkv")).unwrap();
            std::fs::write(root.join("The Invite (2023)/The.Invite.2023.2160p.mkv"), b"xx").unwrap();
            let r = scan(&mut db);
            assert_eq!(listed(&db, ItemKind::Movie).iter().filter(|t| t.0 == id).map(|t| (t.1.as_str(), t.2)).collect::<Vec<_>>(), vec![("The.Invite.2023.2160p.mkv", false)], "auto {auto}");
            assert_eq!(listed(&db, ItemKind::Movie).len(), 2);
            assert!(r.added.is_empty() && r.updated.contains(&id), "{r:?}");
            assert_eq!(db.views.get(&format!("1:{id}")).map(|v| v.view_count), Some(1));
            // Copied in first, the old one deleted later: the same.
            std::fs::write(root.join("The Invite (2023)/The Invite (2023) 1080p.mkv"), b"xxx").unwrap();
            scan(&mut db);
            std::fs::remove_file(root.join("The Invite (2023)/The.Invite.2023.2160p.mkv")).unwrap();
            let r = scan(&mut db);
            assert_eq!(listed(&db, ItemKind::Movie).iter().filter(|t| t.0 == id).map(|t| (t.1.as_str(), t.2)).collect::<Vec<_>>(), vec![("The Invite (2023) 1080p.mkv", false)], "auto {auto}");
            assert_eq!(db.items.values().filter(|i| i.kind == ItemKind::Movie).count(), 2, "no version left behind");
            assert_eq!(r.removed.len(), 1, "the title the new file had for a while goes: {r:?}");
            assert_eq!(db.views.get(&format!("1:{id}")).map(|v| v.view_count), Some(1));
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn another_movie_in_the_same_library_folder_is_not_taken_for_a_swap() {
        let (root, mut db) = library_with("noswap", "movie", &["Heat (1995).mkv"], false);
        let section = db.sections[0].clone();
        let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
        scan(&mut db);
        let id = listed(&db, ItemKind::Movie)[0].0;
        watched(&mut db, id);
        std::fs::remove_file(root.join("Heat (1995).mkv")).unwrap();
        std::fs::write(root.join("Ronin (1998).mkv"), b"x").unwrap();
        let r = scan(&mut db);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(r.added.len(), 1);
        let t = listed(&db, ItemKind::Movie);
        assert_eq!(t.iter().map(|t| (t.1.as_str(), t.2)).collect::<Vec<_>>(), vec![("Heat (1995).mkv", true), ("Ronin (1998).mkv", false)]);
        assert!(db.views.get(&format!("1:{}", r.added[0])).is_none());
    }

    #[test]
    fn an_episode_file_swapped_for_another_keeps_its_watched_state() {
        let (root, mut db) = library_with("epswap", "show", &["Dark/Season 1/Dark - S01E01.mkv", "Dark/Season 1/Dark - S01E02.mkv"], false);
        let section = db.sections[0].clone();
        let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
        scan(&mut db);
        let id = listed(&db, ItemKind::Episode).into_iter().find(|t| t.1.contains("E01")).unwrap().0;
        watched(&mut db, id);
        std::fs::remove_file(root.join("Dark/Season 1/Dark - S01E01.mkv")).unwrap();
        std::fs::write(root.join("Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv"), b"xx").unwrap();
        scan(&mut db);
        let _ = std::fs::remove_dir_all(&root);
        let t = listed(&db, ItemKind::Episode);
        assert_eq!(t.len(), 2);
        assert!(t.contains(&(id, "Dark.S01E01.Secrets.1080p.mkv".into(), false)), "{t:?}");
        assert_eq!(db.views.get(&format!("1:{id}")).map(|v| v.view_count), Some(1));
    }

    #[test]
    fn trash_keeps_gone_files_until_emptied() {
        let root = std::env::temp_dir().join(format!("lichtspiel-trash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in ["Avatar (2009)/Avatar (2009).mkv", "Avatar (2009)/Avatar (2009)-trailer.mkv", "Heat (1995)/Heat (1995).mkv"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let mut settings = crate::db::Settings::default();
        settings.empty_trash_automatically = false;
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": settings,
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let section = db.sections[0].clone();
        let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
        scan(&mut db);
        assert_eq!(db.items.len(), 3);
        let missing = |db: &Db| { let mut v: Vec<String> = db.items.values().filter(|i| i.extra.contains_key(MISSING)).map(|i| i.title.clone()).collect(); v.sort(); v };
        // Gone from disk: kept, marked unavailable.
        std::fs::remove_file(root.join("Avatar (2009)/Avatar (2009).mkv")).unwrap();
        assert!(scan(&mut db).removed.is_empty());
        assert_eq!(missing(&db), vec!["Avatar"]);
        // Back again: available again.
        std::fs::write(root.join("Avatar (2009)/Avatar (2009).mkv"), b"x").unwrap();
        scan(&mut db);
        assert!(missing(&db).is_empty());
        // Gone again and the trash emptied: the movie and its trailer leave.
        std::fs::remove_file(root.join("Avatar (2009)/Avatar (2009).mkv")).unwrap();
        scan(&mut db);
        assert_eq!(empty_trash(&mut db, 1).len(), 2);
        assert_eq!(db.items.values().map(|i| i.title.clone()).collect::<Vec<_>>(), vec!["Heat"]);
        // A library folder that cannot be read empties nothing, even with the automatic setting.
        db.settings.empty_trash_automatically = true;
        std::fs::remove_dir_all(&root).unwrap();
        scan(&mut db);
        assert_eq!(missing(&db), vec!["Heat"]);
    }

    #[test]
    fn music_library_makes_artists_albums_and_tracks() {
        let root = std::env::temp_dir().join(format!("lichtspiel-music-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in ["Queen/Opera/02 - Death on Two Legs.mp3", "Queen/Opera/01 - Intro.mp3", "Queen/Jazz/01 - Mustapha.flac", "Blur/Blur/03 - Song 2.ogg", "notes.txt"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Musik", "kind": "music", "paths": [root.to_string_lossy()], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let section = db.sections[0].clone();
        let r = scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
        assert_eq!(r.files_seen, 4);
        let count = |db: &Db, k: ItemKind| db.items.values().filter(|i| i.kind == k).count();
        assert_eq!((count(&db, ItemKind::Artist), count(&db, ItemKind::Album), count(&db, ItemKind::Track)), (2, 3, 4));
        let opera = db.items.values().find(|i| i.kind == ItemKind::Album && i.title == "Opera").unwrap();
        let songs: Vec<String> = db.children(opera.id).iter().map(|t| t.title.clone()).collect();
        assert_eq!(songs, vec!["Intro", "Death on Two Legs"]);
        // The filters of a music library know the kind of release.
        let tr = db.children(opera.id)[0].clone();
        assert!(matches!(crate::query::field(&db, 0, &tr, "release"), crate::query::FieldValue::Text(v) if v == vec!["album".to_string()]));
        let albums: Vec<&Item> = db.items.values().filter(|i| i.kind == ItemKind::Album).collect();
        let f = crate::query::section_filters(&db, 0, &albums, ItemKind::Album, true, false);
        assert!(f.iter().any(|x| x["filter"] == "release" && x["values"][0]["value"] == "album" && x["values"][0]["count"] == 3));
        // Playing an artist means all its songs, in album order.
        let queen = db.items.values().find(|i| i.kind == ItemKind::Artist && i.title == "Queen").unwrap();
        assert_eq!(db.leaves(queen.id).len(), 3);
        // Taking a file away leaves no empty album behind.
        std::fs::remove_file(root.join("Queen/Jazz/01 - Mustapha.flac")).unwrap();
        scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
        assert_eq!((count(&db, ItemKind::Artist), count(&db, ItemKind::Album), count(&db, ItemKind::Track)), (2, 2, 3));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn partial_scan_only_touches_its_folder() {
        let root = std::env::temp_dir().join(format!("lichtspiel-partial-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in ["Avatar (2009)/Avatar (2009).mkv", "Heat (1995)/Heat (1995).mkv"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let section = db.sections[0].clone();
        assert_eq!(section.scan_interval_minutes, None);
        let only = root.join("Heat (1995)");
        // Only Heat is looked at: Avatar is not added yet.
        let r = scan_section_in(&section, "no-such-ffprobe", Some(&only), |f| f(&mut db));
        assert_eq!(r.files_seen, 1);
        assert_eq!(db.items.values().map(|i| i.title.clone()).collect::<Vec<_>>(), vec!["Heat"]);
        // A full scan finds both; then a file gone from the other folder is not noticed by a partial scan.
        scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
        assert_eq!(db.items.len(), 2);
        std::fs::remove_file(root.join("Avatar (2009)/Avatar (2009).mkv")).unwrap();
        let r = scan_section_in(&section, "no-such-ffprobe", Some(&only), |f| f(&mut db));
        assert!(r.removed.is_empty());
        assert_eq!(db.items.len(), 2);
        // The full scan sees it.
        assert_eq!(scan_section(&section, "no-such-ffprobe", |f| f(&mut db)).removed.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A rescan with nothing changed of 5,000 movie folders, 3,000 of them with a trailer: the time the
    /// library is locked (all of it, and the longest single stretch).
    /// `cargo test --release --no-default-features -- --ignored --nocapture rescan_lock_cost`
    #[test]
    #[ignore]
    fn rescan_lock_cost() {
        let root = std::env::temp_dir().join(format!("lichtspiel-rescan-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for n in 0..5000 {
            let dir = root.join(format!("Film {n} ({})", 1950 + n % 70));
            std::fs::create_dir_all(dir.join("Trailers")).unwrap();
            std::fs::write(dir.join(format!("Film {n} ({}).mkv", 1950 + n % 70)), b"x").unwrap();
            if n % 5 < 3 { std::fs::write(dir.join("Trailers").join("Trailer.mkv"), b"x").unwrap(); }
        }
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let section = db.sections[0].clone();
        scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
        assert_eq!(db.items.values().filter(|i| i.is_extra()).count(), 3000);
        let (mut sums, mut longest) = (vec![], vec![]);
        for _ in 0..5 {
            let (mut sum, mut top) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
            scan_section(&section, "no-such-ffprobe", |f| {
                let t0 = std::time::Instant::now();
                f(&mut db);
                let d = t0.elapsed();
                sum += d;
                top = top.max(d);
            });
            sums.push(sum);
            longest.push(top);
        }
        sums.sort();
        longest.sort();
        println!("rescan_lock_cost: library locked {:?} in all, longest stretch {:?} (medians of 5)", sums[2], longest[2]);
        assert_eq!(db.items.values().filter(|i| i.is_extra()).count(), 3000);
        let _ = std::fs::remove_dir_all(&root);
    }

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
        assert_eq!(parse_title_year("Apocalypse.Now.1979.Final.Cut.Remastered.German.AC3.DL.1080p.BluRay.x265-FuN {edition-Final Cut}"), ("Apocalypse Now".into(), Some(1979)));
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
        assert_eq!(parse_title_year("Abyss.-.Abgrund.des.Todes.THEATRICAL.1989.German.DL.AC3D.1080p.WebHD.x265-FuN"), ("Abyss - Abgrund des Todes".into(), Some(1989)));
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

#[cfg(test)]
mod trailer_tests {
    use super::*;

    #[test]
    fn original_extra_folders_and_trailer_names() {
        for (name, kind) in [("Trailers", "trailer"), ("trailer", "trailer"), ("Trailer_", "trailer"), ("Deleted Scenes", "deleted"), ("deleted", "deleted"),
            ("Behind the Scenes", "behindthescenes"), ("behind-the-scenes", "behindthescenes"), ("Featurettes", "featurette"), ("Interviews", "interview"),
            ("Scenes", "scene"), ("Shorts", "short"), ("Other", "other"), ("Bonus Extras", "other"), ("extra", "other")] {
            assert_eq!(extra_folder_kind(name), Some(kind), "{name}");
        }
        for name in ["Trailer Park Boys", "Shortcuts", "Scenery", "Extraordinary", "Others Here"] {
            assert_eq!(extra_folder_kind(name), None, "{name}");
        }
        let root = Path::new("/m");
        let x = extra_of(root, Path::new("/m/Heat (1995)/trailer.mkv")).expect("trailer.mkv");
        assert_eq!((x.kind, x.prefix.clone()), ("trailer", None));
        assert_eq!(x.movie_dir, Path::new("/m/Heat (1995)"));
        assert_eq!(extra_of(root, Path::new("/m/Heat (1995)/movie-trailer.mp4")).map(|x| x.kind), Some("trailer"));
        assert_eq!(extra_of(root, Path::new("/m/Heat (1995)/Trailer_/Teaser.mkv")).map(|x| x.kind), Some("trailer"));
        assert_eq!(extra_of(root, Path::new("/m/Heat (1995)/Heat (1995)-trailer.mkv")).map(|x| x.kind), Some("trailer"));
        assert!(extra_of(root, Path::new("/m/Heat (1995)/Heat (1995).mkv")).is_none());
    }
}

#[cfg(test)]
mod watch_bench {
    /// How long one look of folder watching takes on 50,000 files (500 folders of 100).
    /// Run with `cargo test --release folder_signature_on_50k_files -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn folder_signature_on_50k_files() {
        let root = std::env::temp_dir().join("lichtspiel-watch-bench");
        if !root.join("done").exists() {
            for d in 0..500 {
                let dir = root.join(format!("Show {d}")).join("Season 1");
                std::fs::create_dir_all(&dir).unwrap();
                for f in 0..100 { std::fs::write(dir.join(format!("Show {d} S01E{f:03}.mkv")), b"").unwrap(); }
            }
            std::fs::write(root.join("done"), b"").unwrap();
        }
        let paths = vec![root.to_string_lossy().to_string()];
        let _ = super::folder_signature(&paths, false);
        let t = std::time::Instant::now();
        for _ in 0..5 { let _ = super::folder_signature(&paths, false); }
        eprintln!("folder_signature over 50,000 files: {:.0} ms per look", t.elapsed().as_secs_f64() * 1000.0 / 5.0);
    }
}
