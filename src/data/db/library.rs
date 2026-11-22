//! Libraries and what is in them: sections, items (movies, shows, seasons, episodes, music), their files and streams.

use super::*;

/// What kind of library a section is. Movies and TV shows are kept apart.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SectionKind {
    Movie,
    Show,
    Other,
    /// Songs: artists, albums and tracks.
    Music,
}

impl SectionKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "movie" | "movies" | "1" => Some(Self::Movie),
            "show" | "shows" | "tv" | "tvshows" | "2" => Some(Self::Show),
            "other" | "video" | "videos" | "homevideo" => Some(Self::Other),
            "music" | "artist" | "artists" | "audio" | "8" => Some(Self::Music),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Show => "show",
            Self::Other => "other",
            Self::Music => "music",
        }
    }
    /// The kind of the top level of a library of this kind.
    pub fn top_kind(self) -> ItemKind {
        match self {
            Self::Movie => ItemKind::Movie,
            Self::Show => ItemKind::Show,
            Self::Other => ItemKind::Video,
            Self::Music => ItemKind::Artist,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Section {
    pub id: u64,
    pub name: String,
    pub kind: SectionKind,
    pub paths: Vec<String>,
    pub created_at: u64,
    #[serde(default)]
    pub scanned_at: u64,
    /// "Enable credits detection" for this library (on unless switched off).
    #[serde(default = "yes")]
    pub credits_detection: bool,
    /// "Enable intro detection" for a TV library (on unless switched off; only
    /// used for TV libraries).
    #[serde(default = "yes")]
    pub intro_detection: bool,
    /// Language for this library's movie and show details. Empty = the server's
    /// "Language for details" setting.
    #[serde(default)]
    pub language: String,
    /// How often this library is scanned by itself, in minutes: None = the server's setting, 0 = never.
    #[serde(default)]
    pub scan_interval_minutes: Option<u64>,
}

/// Languages offered for movie and show details.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"), ("de", "Deutsch"), ("fr", "Français"), ("es", "Español"), ("it", "Italiano"),
    ("nl", "Nederlands"), ("pl", "Polski"), ("pt", "Português"), ("sv", "Svenska"), ("da", "Dansk"),
    ("no", "Norsk"), ("fi", "Suomi"), ("cs", "Čeština"), ("hu", "Magyar"), ("ru", "Русский"),
    ("tr", "Türkçe"), ("el", "Ελληνικά"), ("ja", "日本語"), ("ko", "한국어"), ("zh", "中文"),
];

impl Db {
    /// Credits detection for one item: its own choice (on/off), else the library's.
    pub fn credits_on(&self, it: &Item) -> bool {
        match it.extra.get("credits_detection").and_then(|v| v.as_str()) {
            Some("on") => true,
            Some("off") => false,
            _ => self.sections.iter().find(|s| s.id == it.section_id).is_none_or(|s| s.credits_detection),
        }
    }

    /// The language details are fetched in for a library.
    pub fn language_for_section(&self, section_id: u64) -> String {
        self.sections
            .iter()
            .find(|s| s.id == section_id)
            .map(|s| s.language.trim().to_string())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| self.settings.metadata_language.clone())
    }
}

/// The kind of a library entry. Number codes: 1 movie, 2 show, 3 season, 4 episode.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Movie,
    Show,
    Season,
    Episode,
    Video,
    /// Music: an artist has albums, an album has tracks.
    Artist,
    Album,
    Track,
}

impl ItemKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "movie" | "1" => Some(Self::Movie),
            "show" | "2" => Some(Self::Show),
            "season" | "3" => Some(Self::Season),
            "episode" | "4" => Some(Self::Episode),
            "video" | "clip" => Some(Self::Video),
            "artist" | "8" => Some(Self::Artist),
            "album" | "9" => Some(Self::Album),
            "track" | "song" | "10" => Some(Self::Track),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Show => "show",
            Self::Season => "season",
            Self::Episode => "episode",
            Self::Video => "video",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Track => "track",
        }
    }
    pub fn code(self) -> u32 {
        match self {
            Self::Movie => 1,
            Self::Show => 2,
            Self::Season => 3,
            Self::Episode => 4,
            Self::Video => 1,
            Self::Artist => 8,
            Self::Album => 9,
            Self::Track => 10,
        }
    }
    pub fn playable(self) -> bool {
        matches!(self, Self::Movie | Self::Episode | Self::Video | Self::Track)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Stream {
    /// "video", "audio" or "subtitle".
    pub kind: String,
    pub index: u32,
    pub codec: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub channels: u32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub bit_depth: u32,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub title: String,
    /// Subtitle: only the lines in a foreign language ("forced").
    #[serde(default)]
    pub forced: bool,
    /// Subtitle: also describes sounds, for the deaf and hard of hearing (SDH / CC).
    #[serde(default)]
    pub sdh: bool,
    /// ffprobe's profile ("Main 10", "Dolby TrueHD + Dolby Atmos" …).
    #[serde(default)]
    pub profile: String,
    /// Video: color transfer (smpte2084 = HDR10/PQ, arib-std-b67 = HLG).
    #[serde(default)]
    pub color_transfer: String,
    /// Video: HDR10+ dynamic metadata seen in the first frame.
    #[serde(default)]
    pub hdr10plus: bool,
    /// Video: Dolby Vision (configuration record or dvhe / dvh1 / dav1 tag).
    #[serde(default)]
    pub dovi: bool,
    /// Audio: channel layout ("stereo", "5.1(side)" …).
    #[serde(default)]
    pub channel_layout: String,
}

impl Stream {
    pub fn is_hdr(&self) -> bool {
        matches!(self.color_transfer.as_str(), "smpte2084" | "arib-std-b67") || self.hdr10plus || self.dovi
    }
    pub fn is_atmos(&self) -> bool {
        self.profile.to_lowercase().contains("atmos") || self.title.to_lowercase().contains("atmos")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MediaFile {
    pub path: String,
    pub size: u64,
    pub modified: u64,
    /// File extension, lower case (mkv, mp4, avi …).
    pub container: String,
    /// Container name as ffprobe reports it, when probed.
    #[serde(default)]
    pub format_name: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub bitrate: u64,
    #[serde(default)]
    pub streams: Vec<Stream>,
    #[serde(default)]
    pub probed: bool,
    /// Which version of the probe read the streams; older files are read
    /// again on the next scan (PROBE_VERSION 2 added HDR, Atmos and layouts, 3 Dolby Vision).
    #[serde(default)]
    pub probe_version: u32,
    /// Subtitle files found next to the video (srt, ass, vtt …).
    #[serde(default)]
    pub external_subtitles: Vec<String>,
    /// Where end credits start (ms), from chapters named like "Credits".
    #[serde(default)]
    pub credits_ms: Vec<u64>,
    /// Credits detection: which version of the detector looked at this file
    /// (0 = not yet), how it found them ("chapters" / "picture") and how often
    /// it failed (no more tries after more than 3 failures).
    #[serde(default)]
    pub credits_checked: u32,
    #[serde(default)]
    pub credits_source: String,
    #[serde(default)]
    pub credits_errors: u32,
    /// Where the intro runs (start and end, ms; empty = none), found by
    /// comparing the sound of a season's episodes (intro.rs); which version of
    /// the detector looked (0 = not yet) and how often reading the file failed.
    #[serde(default)]
    pub intro_ms: Vec<u64>,
    #[serde(default)]
    pub intro_checked: u32,
    #[serde(default)]
    pub intro_errors: u32,
    /// Where the recap ("Previously on") runs (start and end, ms; empty = none),
    /// found by comparing the start with the episodes before (intro.rs), which
    /// version of that looked (0 = not yet) and how often reading failed.
    #[serde(default)]
    pub recap_ms: Vec<u64>,
    #[serde(default)]
    pub recap_checked: u32,
    #[serde(default)]
    pub recap_errors: u32,
    /// Deep analysis (deep.rs): the speed in kbit/s this file needs at each
    /// connection level, the analysis version, the file date it belongs to,
    /// and how often reading failed.
    #[serde(default)]
    pub required_kbps: Vec<u32>,
    #[serde(default)]
    pub deep_version: u32,
    #[serde(default)]
    pub deep_date: u64,
    #[serde(default)]
    pub deep_errors: u32,
}

/// Bump when the credits detector changes, so files are looked at again.
pub const CREDITS_DETECTOR_VERSION: u32 = 1;
/// Bump when `scanner::probe` reads more about the streams.
pub const PROBE_VERSION: u32 = 3;
/// A file is given up on after more than 3 failed detections.
pub const CREDITS_MAX_ERRORS: u32 = 3;

impl MediaFile {
    pub fn video(&self) -> Option<&Stream> {
        self.streams.iter().find(|s| s.kind == "video")
    }
    pub fn audio(&self) -> Option<&Stream> {
        self.streams
            .iter()
            .filter(|s| s.kind == "audio")
            .max_by_key(|s| s.default as u8)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item {
    pub id: u64,
    pub section_id: u64,
    pub kind: ItemKind,
    #[serde(default)]
    pub parent_id: Option<u64>,
    pub title: String,
    #[serde(default)]
    pub sort_title: String,
    #[serde(default)]
    pub year: Option<u32>,
    /// Season number for seasons, episode number for episodes.
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub rating: Option<f32>,
    #[serde(default)]
    pub content_rating: String,
    #[serde(default)]
    pub studio: String,
    pub added_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub file: Option<MediaFile>,
    /// A movie split over several files ("… - pt1", "… - pt2"): the files after
    /// `file`, in order. They play one after the other as one movie.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<MediaFile>,
    /// A poster picture found next to the media, if any.
    #[serde(default)]
    pub poster: Option<String>,
    /// Fields that plugins added (free form).
    #[serde(default)]
    pub extra: serde_json::Map<String, Value>,
    /// Fields edited by hand through the API; a rescan never overwrites them.
    #[serde(default)]
    pub locked: Vec<String>,
}

impl Item {
    /// A trailer, featurette, deleted scene … of the movie it belongs to
    /// (parent_id). Shown on the movie's page, not as a video of its own.
    pub fn is_extra(&self) -> bool {
        self.kind == ItemKind::Video && self.extra.contains_key("extra_type")
    }
    /// Another version (1080p, 4K …) of the movie or episode in `version_of`: merged into it by
    /// hand, shown on its page as a choice and left out of the lists.
    pub fn version_of(&self) -> Option<u64> {
        self.extra.get("version_of").and_then(|v| v.as_u64())
    }
    /// Left out of the lists: an extra, or a version of another item.
    pub fn hidden(&self) -> bool {
        self.is_extra() || self.version_of().is_some()
    }
    /// The file and every further part, in playing order.
    pub fn all_files(&self) -> Vec<&MediaFile> {
        self.file.iter().chain(self.parts.iter()).collect()
    }
    /// Part `n` (0 = the first file).
    pub fn part(&self, n: usize) -> Option<&MediaFile> {
        self.all_files().get(n).copied()
    }
    /// The file the end credits are in: the last part of a split movie.
    pub fn last_file(&self) -> Option<&MediaFile> {
        self.parts.last().or(self.file.as_ref())
    }
    pub fn last_file_mut(&mut self) -> Option<&mut MediaFile> {
        match self.parts.last_mut() {
            Some(p) => Some(p),
            None => self.file.as_mut(),
        }
    }
    /// Where the last file starts in the whole movie (0 unless it is split).
    pub fn last_file_start_ms(&self) -> u64 {
        if self.parts.is_empty() { return 0; }
        self.all_files().iter().rev().skip(1).map(|f| f.duration_ms).sum()
    }
}
