//! Matching the old server to this one: online ids, file paths, translated folders and suggested folders.

use super::*;

/// The online entry Plex had picked for a movie or show, as (source, id) like a
/// file name tag: from the external ids of the new agent ("imdb://tt0133093",
/// "tmdb://603", "tvdb://81189"), or from the guid of an old agent
/// ("com.plexapp.agents.imdb://tt0133093?lang=en", "...themoviedb://603", "...thetvdb://81189").
pub fn plex_external_id(guid: &str, tags: &[String]) -> Option<(&'static str, String)> {
    fn digits_or_tt(src: &str, v: &str) -> Option<String> {
        let id: String = v.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        let ok = if src == "imdb" { id.len() > 2 && id.starts_with("tt") && id[2..].chars().all(|c| c.is_ascii_digit()) } else { !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) };
        ok.then_some(id)
    }
    let mut found: Vec<(&'static str, String)> = vec![];
    for t in tags {
        if let Some((s, v)) = t.split_once("://") {
            let src = match s.trim().to_lowercase().as_str() { "imdb" => "imdb", "tmdb" => "tmdb", "tvdb" => "tvdb", _ => continue };
            if let Some(id) = digits_or_tt(src, v.trim()) { found.push((src, id)); }
        }
    }
    for (prefix, src) in [("com.plexapp.agents.imdb://", "imdb"), ("com.plexapp.agents.themoviedb://", "tmdb"), ("com.plexapp.agents.thetvdb://", "tvdb")] {
        if let Some(v) = guid.strip_prefix(prefix) {
            if let Some(id) = digits_or_tt(src, v) { found.push((src, id)); }
        }
    }
    // TMDB is the most exact for a movie and works with and without a key; then IMDb, then TheTVDB.
    ["tmdb", "imdb", "tvdb"].iter().find_map(|s| found.iter().find(|(f, _)| f == s).cloned())
}

/// Comparable form of a path (separators and letter case don't matter).
pub fn path_key(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// Translates an old path with the longest matching "from → to" rule.
pub fn map_path(p: &str, maps: &[(String, String)]) -> String {
    let kp = path_key(p);
    let best = maps
        .iter()
        .filter(|(f, _)| !f.trim().is_empty())
        .filter(|(f, _)| {
            let kf = path_key(f);
            kp == kf || kp.starts_with(&format!("{kf}/"))
        })
        .max_by_key(|(f, _)| f.len());
    let Some((f, t)) = best else { return p.to_string() };
    let skip = f.trim_end_matches(['/', '\\']).chars().count();
    let rest: String = p.chars().skip(skip).collect();
    let to = t.trim_end_matches(['/', '\\']);
    let joined = format!("{to}{rest}");
    if to.contains('/') && !to.contains('\\') {
        joined.replace('\\', "/")
    } else if to.contains('\\') {
        joined.replace('/', "\\")
    } else {
        joined
    }
}

/// A folder on this server for an old library folder: the same path if it
/// exists here, else a folder with the same name under /media or next to an
/// existing library folder.
pub(crate) fn suggest(old: &str, ours: &[String]) -> String {
    if Path::new(old).is_dir() {
        return old.to_string();
    }
    let name = old.replace('\\', "/").trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
    if name.is_empty() {
        return String::new();
    }
    let mut places: Vec<PathBuf> = vec![PathBuf::from("/media").join(&name), PathBuf::from("/data").join(&name)];
    for o in ours {
        if path_key(o).ends_with(&format!("/{}", name.to_lowercase())) {
            places.insert(0, PathBuf::from(o));
        }
        if let Some(parent) = Path::new(o).parent() {
            places.push(parent.join(&name));
        }
    }
    places.iter().find(|p| p.is_dir()).map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
}
