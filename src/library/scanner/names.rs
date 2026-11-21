//! Reading titles, years, editions, parts and season / episode numbers from file and folder names.

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
    let title = crate::library::metadata::move_article_front(&title).unwrap_or(title);
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

pub(super) fn season_from_folder(name: &str) -> Option<u32> {
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

pub(super) fn tidy(s: &str) -> String {
    let s = if s.contains(' ') { s.to_string() } else { s.replace(['.', '_'], " ") };
    s.trim_matches(|c: char| c == '-' || c == ' ' || c == '.' || c == '_')
        .to_string()
}
