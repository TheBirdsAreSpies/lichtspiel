//! Music libraries: what an audio file is (artist, album, track number …),
//! read from its tags, or from the folder and file names when it has none.
//! The artist / album / track items themselves are made by the scanner.

use serde_json::Value;
use std::path::Path;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wav", "wma", "aif", "aiff", "ape", "wv", "alac", "dsf", "dff", "mka", "mpc",
];

pub fn is_audio(path: &Path) -> bool {
    let ext = crate::scanner::ext_of(path);
    AUDIO_EXTENSIONS.contains(&ext.as_str())
}

/// What is known about one song.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MusicInfo {
    pub title: String,
    /// Who the album is filed under (the album artist, else the artist).
    pub artist: String,
    pub album: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub year: Option<u32>,
    pub genres: Vec<String>,
    /// album, ep, single, live, compilation or soundtrack
    pub release: &'static str,
}

pub const UNKNOWN_ARTIST: &str = "Unknown Artist";
pub const UNKNOWN_ALBUM: &str = "Unknown Album";

/// A tag by name, ignoring case (ffprobe gives FLAC / Ogg tags as they are written).
fn tag(tags: &Value, name: &str) -> Option<String> {
    let obj = tags.as_object()?;
    obj.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .and_then(|(_, v)| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A tag whose name is matched without case, spaces or underscores ("MusicBrainz Album Type").
fn tag_loose(tags: &Value, name: &str) -> Option<String> {
    let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let want = norm(name);
    tags.as_object()?.iter().find(|(k, _)| norm(k) == want).and_then(|(_, v)| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// What kind of release an album is: from the tags (MusicBrainz release type, compilation flag),
/// else from words in its title ("Live at …", "(Single)", "EP", "Soundtrack", "Greatest Hits").
pub fn release_type(tags: &Value, album: &str) -> &'static str {
    let by_word = |s: &str| -> Option<&'static str> {
        let s = s.to_lowercase();
        let has = |w: &str| s.contains(w);
        let ends = |w: &str| s.ends_with(w);
        Some(if has("(live") || has("[live") || has(" live ") || ends(" live") || has("unplugged") || s.starts_with("live ") { "live" }
            else if has("soundtrack") || has("(ost") || has("[ost") || ends(" ost") || has("original score") || has("motion picture") { "soundtrack" }
            else if has("(single") || has("[single") || ends(" single") { "single" }
            else if has("(ep)") || has("[ep]") || ends(" ep") { "ep" }
            else if has("greatest hits") || has("best of") || has("the very best") || has("anthology") || has("compilation") { "compilation" }
            else { return None })
    };
    if let Some(t) = tag_loose(tags, "musicbrainz album type").or_else(|| tag_loose(tags, "releasetype")) {
        let t = t.to_lowercase();
        for (w, r) in [("live", "live"), ("soundtrack", "soundtrack"), ("compilation", "compilation"), ("single", "single"), ("ep", "ep")] {
            if t.split(['/', ';', ',', ' ']).any(|p| p == w) { return r; }
        }
    }
    if tag_loose(tags, "compilation").is_some_and(|c| c == "1") { return "compilation"; }
    by_word(album).unwrap_or("album")
}

/// "3/12" or "03" -> 3.
fn number(s: &str) -> Option<u32> {
    s.split('/').next()?.trim().parse::<u32>().ok().filter(|n| *n > 0)
}

/// "2019-05-03", "2019" -> 2019.
fn year_of(s: &str) -> Option<u32> {
    let y: u32 = s.trim().get(..4)?.parse().ok()?;
    (1000..=2999).contains(&y).then_some(y)
}

/// "01 - Song", "1. Song", "01-Song", "2-05 Song" -> (track, "Song").
pub fn split_track_number(stem: &str) -> (Option<u32>, String) {
    let s = stem.trim();
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 3 {
        return (None, s.to_string());
    }
    let mut rest = s[digits.len()..].trim_start();
    // "2-05 Song": the first number is the disc.
    let mut n: u32 = digits.parse().unwrap_or(0);
    if let Some(r) = rest.strip_prefix('-') {
        let d2: String = r.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
        if !d2.is_empty() && d2.len() <= 3 && r.trim_start()[d2.len()..].starts_with([' ', '.', '_']) {
            n = d2.parse().unwrap_or(n);
            rest = r.trim_start()[d2.len()..].trim_start();
        }
    }
    let rest = rest.trim_start_matches(['-', '.', '_', ' ']).trim();
    if rest.is_empty() { return (None, s.to_string()); }
    ((n > 0).then_some(n), rest.to_string())
}

/// Works out a song from its tags (`tags`: the ffprobe format and stream tags merged) and its
/// place below the library folder (`root/Artist/Album/01 - Song.mp3`).
pub fn info(root: &Path, path: &Path, tags: Option<&Value>) -> MusicInfo {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let (file_track, file_title) = split_track_number(&stem);
    // Folders: .../Artist/Album/file. With one folder it is the album, with none there is nothing.
    let folder_album = (comps.len() >= 2).then(|| comps[comps.len() - 2].clone());
    let folder_artist = (comps.len() >= 3).then(|| comps[comps.len() - 3].clone());
    let null = Value::Null;
    let t = tags.unwrap_or(&null);
    let album = tag(t, "album").or(folder_album).unwrap_or_else(|| UNKNOWN_ALBUM.into());
    let mut genres: Vec<String> = tag(t, "genre").map(|g| g.split([';', '/']).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    genres.dedup();
    MusicInfo {
        title: tag(t, "title").unwrap_or(file_title),
        artist: tag(t, "album_artist").or_else(|| tag(t, "albumartist")).or_else(|| tag(t, "artist")).or(folder_artist).unwrap_or_else(|| UNKNOWN_ARTIST.into()),
        album: album.clone(),
        track: tag(t, "track").and_then(|s| number(&s)).or(file_track),
        disc: tag(t, "disc").or_else(|| tag(t, "discnumber")).and_then(|s| number(&s)),
        year: tag(t, "date").or_else(|| tag(t, "year")).or_else(|| tag(t, "originaldate")).and_then(|s| year_of(&s)),
        release: release_type(t, &album),
        genres,
    }
}

/// The tags of an ffprobe answer: the container's, plus the audio stream's (Ogg keeps them there).
pub fn tags_of(v: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(streams) = v["streams"].as_array() {
        for s in streams.iter().filter(|s| s["codec_type"] == "audio") {
            if let Some(o) = s["tags"].as_object() { for (k, x) in o { out.insert(k.clone(), x.clone()); } }
        }
    }
    if let Some(o) = v["format"]["tags"].as_object() { for (k, x) in o { out.insert(k.clone(), x.clone()); } }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn track_numbers_in_names() {
        assert_eq!(split_track_number("01 - Intro"), (Some(1), "Intro".into()));
        assert_eq!(split_track_number("7. Seven Nation Army"), (Some(7), "Seven Nation Army".into()));
        assert_eq!(split_track_number("2-05 Song"), (Some(5), "Song".into()));
        assert_eq!(split_track_number("1979"), (None, "1979".into()));
        assert_eq!(split_track_number("Song"), (None, "Song".into()));
    }

    #[test]
    fn info_from_tags_and_folders() {
        let root = Path::new("/m");
        // Only folders and the name.
        let i = info(root, Path::new("/m/Queen/A Night at the Opera/03 - Love of My Life.flac"), None);
        assert_eq!((i.artist.as_str(), i.album.as_str(), i.title.as_str(), i.track), ("Queen", "A Night at the Opera", "Love of My Life", Some(3)));
        // Tags win; the album artist beats the artist; tag names in any case.
        let tags = json!({"TITLE": "Song 2", "ARTIST": "Blur feat. X", "ALBUM_ARTIST": "Blur", "album": "Blur", "track": "2/14", "disc": "1/1", "date": "1997-02-10", "genre": "Rock; Indie"});
        let i = info(root, Path::new("/m/x/01.mp3"), Some(&tags));
        assert_eq!((i.artist.as_str(), i.album.as_str(), i.title.as_str(), i.track, i.disc, i.year), ("Blur", "Blur", "Song 2", Some(2), Some(1), Some(1997)));
        assert_eq!(i.genres, vec!["Rock", "Indie"]);
        // A file straight in the library folder.
        let i = info(root, Path::new("/m/Loose Song.mp3"), None);
        assert_eq!((i.artist.as_str(), i.album.as_str(), i.title.as_str()), (UNKNOWN_ARTIST, UNKNOWN_ALBUM, "Loose Song"));
    }

    #[test]
    fn release_types() {
        let none = json!({});
        assert_eq!(release_type(&none, "A Night at the Opera"), "album");
        assert_eq!(release_type(&none, "Alive 2007"), "album");
        assert_eq!(release_type(&none, "Live at Wembley"), "live");
        assert_eq!(release_type(&none, "Nevermind (Live)"), "live");
        assert_eq!(release_type(&none, "Hello [Single]"), "single");
        assert_eq!(release_type(&none, "Blue EP"), "ep");
        assert_eq!(release_type(&none, "Greatest Hits"), "compilation");
        assert_eq!(release_type(&none, "Drive (Original Motion Picture Soundtrack)"), "soundtrack");
        assert_eq!(release_type(&json!({"MusicBrainz Album Type": "album/live"}), "Plain"), "live");
        assert_eq!(release_type(&json!({"COMPILATION": "1"}), "Plain"), "compilation");
    }

    #[test]
    fn audio_files_are_known() {
        assert!(is_audio(Path::new("a.FLAC")) && is_audio(Path::new("b.m4a")));
        assert!(!is_audio(Path::new("a.mkv")) && !is_audio(Path::new("a.jpg")));
    }
}
