//! Files next to a video: posters and external subtitles (also those kept in the subtitle store).

use super::*;

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

pub(super) fn find_subtitles(dir: &Path, stem: &str) -> Vec<String> {
    find_subtitles_in(dir, stem, SUBTITLE_STORE.get().map(PathBuf::as_path))
}

/// The subtitle files of a video: next to it, and in its folder of the subtitle store.
pub(super) fn find_subtitles_in(dir: &Path, stem: &str, store: Option<&Path>) -> Vec<String> {
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

/// The subtitle files of the folders looked at during one scan, so a season's episodes read their
/// folder once: folder -> (file name, path).
pub(super) type SubtitleCache = HashMap<PathBuf, Vec<(String, String)>>;

/// `find_subtitles` with the folders read once per scan.
pub(super) fn find_subtitles_cached(cache: &mut SubtitleCache, dir: &Path, stem: &str) -> Vec<String> {
    let store = SUBTITLE_STORE.get().map(|s| store_dir(s, dir));
    let mut out = vec![];
    for d in std::iter::once(dir.to_path_buf()).chain(store) {
        let list = cache.entry(d.clone()).or_insert_with(|| {
            let Ok(rd) = std::fs::read_dir(&d) else { return vec![] };
            rd.flatten().map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
                .filter(|(_, p)| SUBTITLE_EXTENSIONS.contains(&ext_of(p).as_str()))
                .map(|(n, p)| (n, p.to_string_lossy().to_string())).collect()
        });
        out.extend(list.iter().filter(|(n, _)| n.starts_with(stem)).map(|(_, p)| p.clone()));
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
