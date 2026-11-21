//! Walking a library's folders: which files are videos, .lichtspielignore rules, and a cheap signature to notice changes.

use super::*;

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
pub(super) struct IgnoreRules {
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

pub(super) fn ignored(rules: &[IgnoreRules], path: &Path) -> bool {
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
pub(super) struct Listed {
    pub(super) path: PathBuf,
    pub(super) size: u64,
    pub(super) mtime: u64,
    /// A link to a file somewhere else.
    pub(super) link: bool,
}

fn mtime_of(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

pub(super) fn walk(dir: &Path, out: &mut Vec<Listed>, depth: usize, audio: bool, live: Option<(u64, usize)>) {
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
                if crate::library::music::is_audio(&p) {
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
/// still being copied, and when a file appears in or leaves a folder with videos
/// (the folder's change time: a subtitle file put next to a video).
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
        let dirs: std::collections::BTreeSet<&Path> = files.iter().filter_map(|f| f.path.parent()).collect();
        for d in dirs {
            file_mtime(d).hash(&mut h);
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
