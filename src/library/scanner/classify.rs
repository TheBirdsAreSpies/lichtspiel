//! What a file is: a movie, an episode or an extra (trailers, featurettes …), from its place and name.

use super::*;

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
    pub music: Option<crate::library::music::MusicInfo>,
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
            let m = crate::library::music::info(root, path, None);
            Parsed { kind: ItemKind::Track, title: m.title.clone(), year: m.year, show: None, season: None, episode: m.track, music: Some(m) }
        }
    }
}
