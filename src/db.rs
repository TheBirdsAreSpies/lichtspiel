//! The server's stored data: libraries, items, users, invites, playlists,
//! watch state and plugin settings. All of it is held in memory; it is kept
//! in an SQLite file (see crate::store), which a save updates row by row.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use crate::store::Tracked;
use std::path::Path;

/// Today's date (UTC) as YYYY-MM-DD.
pub fn today() -> String {
    // Days since 1970 → civil date (H. Hinnant's algorithm).
    let z = (now() / 86400) as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    if getrandom::getrandom(&mut buf).is_err() {
        // Fall back to time-based noise; only used if the OS refuses randomness.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        for (i, b) in buf.iter_mut().enumerate() {
            *b = ((t >> ((i % 16) * 8)) as u8) ^ (i as u8).wrapping_mul(31);
        }
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// What kind of library a section is. Movies and TV shows are kept apart.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SectionKind {
    Movie,
    Show,
    Other,
}

impl SectionKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "movie" | "movies" | "1" => Some(Self::Movie),
            "show" | "shows" | "tv" | "tvshows" | "2" => Some(Self::Show),
            "other" | "video" | "videos" | "homevideo" => Some(Self::Other),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Show => "show",
            Self::Other => "other",
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
    /// Language for this library's movie and show details. Empty = the server's
    /// "Language for details" setting.
    #[serde(default)]
    pub language: String,
}

/// Languages offered for movie and show details.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"), ("de", "Deutsch"), ("fr", "Français"), ("es", "Español"), ("it", "Italiano"),
    ("nl", "Nederlands"), ("pl", "Polski"), ("pt", "Português"), ("sv", "Svenska"), ("da", "Dansk"),
    ("no", "Norsk"), ("fi", "Suomi"), ("cs", "Čeština"), ("hu", "Magyar"), ("ru", "Русский"),
    ("tr", "Türkçe"), ("el", "Ελληνικά"), ("ja", "日本語"), ("ko", "한국어"), ("zh", "中文"),
];

impl Db {
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
}

impl ItemKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "movie" | "1" => Some(Self::Movie),
            "show" | "2" => Some(Self::Show),
            "season" | "3" => Some(Self::Season),
            "episode" | "4" => Some(Self::Episode),
            "video" | "clip" => Some(Self::Video),
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
        }
    }
    pub fn code(self) -> u32 {
        match self {
            Self::Movie => 1,
            Self::Show => 2,
            Self::Season => 3,
            Self::Episode => 4,
            Self::Video => 1,
        }
    }
    pub fn playable(self) -> bool {
        matches!(self, Self::Movie | Self::Episode | Self::Video)
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct User {
    pub id: u64,
    pub username: String,
    #[serde(default)]
    pub email: String,
    pub password_hash: String,
    pub salt: String,
    pub admin: bool,
    pub created_at: u64,
    /// None = every library; Some(list) = only those library ids.
    #[serde(default)]
    pub allowed_sections: Option<Vec<u64>>,
    #[serde(default)]
    pub invited_by: Option<u64>,
    #[serde(default)]
    pub allow_transcode: Option<bool>,
    /// May delete media files from disk (when the server allows media deletion;
    /// admins always may then).
    #[serde(default)]
    pub allow_delete: bool,
    /// May download (None = yes, "Allow downloads"); admins always may.
    #[serde(default)]
    pub allow_download: Option<bool>,
    /// The left menu as this person arranged it: {order: ["s12", "playlists", …], hidden: [...]}.
    #[serde(default)]
    pub sidebar: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Invite {
    pub code: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub note: String,
    pub created_by: u64,
    pub created_at: u64,
    pub expires_at: u64,
    #[serde(default)]
    pub allowed_sections: Option<Vec<u64>>,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub used_by: Option<u64>,
    #[serde(default)]
    pub used_at: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Token {
    pub token: String,
    pub user_id: u64,
    pub created_at: u64,
    #[serde(default)]
    pub device: String,
}

/// One condition of a smart playlist (or of a library filter).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rule {
    pub field: String,
    #[serde(default = "default_op")]
    pub op: String,
    #[serde(default)]
    pub value: Value,
}

fn default_op() -> String {
    "is".into()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SmartQuery {
    /// true = every rule must match; false = any rule.
    #[serde(default = "yes")]
    pub match_all: bool,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// e.g. "added_at:desc" or "title" (several separated by commas).
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub limit: Option<usize>,
    /// Only look in these libraries (empty = all the user may see).
    #[serde(default)]
    pub sections: Vec<u64>,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PlaylistKind {
    /// Items picked by hand, in the order chosen.
    Manual,
    /// Items chosen by rules, refreshed every time it is opened.
    Smart,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Playlist {
    pub id: u64,
    /// 0 = a default playlist the server provides for everyone.
    pub owner_id: u64,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub kind: PlaylistKind,
    #[serde(default)]
    pub items: Vec<u64>,
    #[serde(default)]
    pub query: Option<SmartQuery>,
    #[serde(default)]
    pub default_key: Option<String>,
    #[serde(default)]
    pub shared: bool,
    #[serde(default)]
    pub pinned: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Playlist {
    pub fn is_default(&self) -> bool {
        self.default_key.is_some()
    }
}

/// Per user, per item: where they stopped, how often watched, their rating.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ViewState {
    #[serde(default)]
    pub view_offset_ms: u64,
    #[serde(default)]
    pub view_count: u32,
    #[serde(default)]
    pub last_viewed_at: u64,
    #[serde(default)]
    pub rating: Option<f32>,
    #[serde(default)]
    pub favorite: bool,
    /// On this person's watchlist since (unix seconds); 0 = not on it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub watchlist_at: u64,
    /// Set once the played threshold was passed in the current viewing,
    /// so one viewing counts once.
    #[serde(default)]
    pub finished: bool,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One viewing in the play history: counted when the played threshold is
/// passed, or marked watched by hand (`manual`).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayEvent {
    pub user_id: u64,
    pub item_id: u64,
    pub at: u64,
    #[serde(default)]
    pub manual: bool,
}

/// A movie or episode someone asked to download, in a quality. Kept until
/// they remove it; a converted copy is deleted `download_keep_days` after it
/// was made.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Download {
    pub id: u64,
    pub user_id: u64,
    pub item_id: u64,
    /// "original" or one of the quality names (transcode::QUALITIES).
    pub quality: String,
    /// queued | converting | ready | failed
    pub state: String,
    /// The original file is sent: asked for, or it already fits the quality.
    #[serde(default)]
    pub original: bool,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub error: String,
    pub created_at: u64,
    #[serde(default)]
    pub finished_at: u64,
}

/// The play history keeps at most this many viewings (the oldest go first).
pub const HISTORY_MAX: usize = 20000;

/// "Report a problem" on a movie, show or episode; the owner sees the list.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProblemReport {
    pub id: u64,
    pub user_id: u64,
    pub item_id: u64,
    /// playback | audio | subtitles | video | metadata | other
    pub kind: String,
    #[serde(default)]
    pub note: String,
    pub at: u64,
    #[serde(default)]
    pub resolved: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PluginState {
    pub enabled: bool,
    #[serde(default)]
    pub settings: serde_json::Map<String, Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    /// Empty = the computer's name (see `display_name`).
    pub server_name: String,
    /// "none", "vaapi", "qsv" or "nvenc".
    pub hw_accel: String,
    pub ffmpeg: String,
    pub ffprobe: String,
    pub scan_interval_minutes: u64,
    /// Percent of a video that counts as watched.
    pub watched_threshold_percent: u32,
    pub hls_segment_seconds: u32,
    pub max_transcodes: usize,
    pub invite_default_days: u64,
    /// Address shown in invite links (empty = use the request's Host).
    #[serde(default)]
    pub public_url: String,
    /// "automatic", "speed", "quality" or "max".
    #[serde(default = "auto_quality")]
    pub transcoder_quality: String,
    /// Media not watched in this many weeks leaves Continue Watching.
    #[serde(default = "cw_weeks")]
    pub continue_watching_weeks: u64,
    #[serde(default = "cw_limit")]
    pub continue_watching_limit: usize,
    /// How long an HLS request waits for the transcoder to write data.
    #[serde(default = "seg_timeout")]
    pub segmented_transcoder_timeout_seconds: u64,
    /// "Scan my library periodically" — off by default.
    #[serde(default)]
    pub scheduled_scans_enabled: bool,
    /// x264 preset used for every software transcode ("veryfast" by default).
    #[serde(default = "h264_preset")]
    pub h264_preset: String,
    /// Lowest (best) x264 CRF the quality formula may reach (16 by default).
    #[serde(default = "min_crf")]
    pub h264_minimum_crf: u32,
    /// Season premieres always appear in Continue Watching.
    #[serde(default = "yes")]
    pub continue_watching_include_premieres: bool,
    /// Look up plot, cast, posters … online for new items.
    #[serde(default = "yes")]
    pub metadata_enabled: bool,
    /// Language for online details, e.g. "en" or "de".
    #[serde(default = "lang")]
    pub metadata_language: String,
    /// Optional free key from themoviedb.org; without it Wikipedia/Wikidata and TVmaze are used.
    #[serde(default)]
    pub tmdb_api_key: String,
    /// Optional free key from omdbapi.com; with it movie and show plots are IMDb's.
    #[serde(default)]
    pub omdb_api_key: String,
    /// When a video counts as watched:
    /// 0 at the threshold, 1 at the final credits marker, 2 at the first credits
    /// marker, 3 at whichever comes first of threshold and first credits marker.
    #[serde(default = "played_at")]
    pub played_at_behaviour: u32,
    /// Hardware encoders use constant quality (-qp / -q from the quality formula)
    /// instead of a bitrate. A choice (off = bitrate).
    #[serde(default)]
    pub hw_constant_quality: bool,
    /// "Generate credits video markers": "never", "scheduled" or "asap" (default:
    /// as a scheduled task and when media is added).
    #[serde(default = "asap")]
    pub credits_markers_behaviour: String,
    /// How many actor photos are saved per movie or show.
    #[serde(default = "cast_photos")]
    pub cast_photos_limit: usize,
    /// Port the server listens on when no --port / LICHTSPIEL_PORT is given
    /// (used after a restart).
    #[serde(default = "port")]
    pub port: u16,
    /// Notice new, changed and removed files in library folders and scan them
    /// ("Scan my library automatically"; on by default).
    #[serde(default = "yes")]
    pub watch_folders: bool,
    /// When a file has no "Credits" chapter, look at the picture for end credits.
    #[serde(default = "yes")]
    pub credits_from_picture: bool,
    /// Fill in IMDb ratings from IMDb's own ratings file (about 9 MB, weekly).
    #[serde(default = "yes")]
    pub imdb_ratings_file: bool,
    /// "Empty trash automatically after every scan": files gone from disk leave
    /// the library at once. Off: they stay as unavailable until "Empty trash".
    #[serde(default = "yes")]
    pub empty_trash_automatically: bool,
    /// "Allow media deletion": the owner (and people allowed to) may delete
    /// files from disk from the "…" menu.
    #[serde(default = "yes")]
    pub allow_media_deletion: bool,
    /// Converted downloads are deleted this many days after they were made (0 = kept).
    #[serde(default = "keep_days")]
    pub download_keep_days: u64,
}

fn keep_days() -> u64 {
    7
}

fn port() -> u16 {
    crate::server::DEFAULT_PORT
}

fn cast_photos() -> usize {
    20
}

fn asap() -> String {
    "asap".into()
}

/// The choices for "Generate credits video markers".
pub const CREDITS_MARKER_CHOICES: &[(&str, &str)] = &[
    ("never", "never"),
    ("scheduled", "as a scheduled task"),
    ("asap", "as a scheduled task and when media is added"),
];

fn played_at() -> u32 {
    3
}

/// The "Video play completion behaviour" choices.
pub const PLAYED_AT_CHOICES: &[(u32, &str)] = &[
    (0, "at selected threshold percentage"),
    (1, "at final credits marker position"),
    (2, "at first credits marker position"),
    (3, "earliest between threshold percent and first credits marker"),
];

/// Position (ms) at which a video of `duration` ms counts as watched, given its
/// credits markers (start times, ms). Without markers the threshold is used.
pub fn watched_at_ms(behaviour: u32, threshold_percent: u32, duration: u64, credits: &[u64]) -> u64 {
    let threshold = duration * threshold_percent.clamp(1, 100) as u64 / 100;
    let first = credits.iter().copied().filter(|&c| c > 0 && c < duration).min();
    let last = credits.iter().copied().filter(|&c| c > 0 && c < duration).max();
    match (behaviour, first, last) {
        (1, _, Some(l)) => l,
        (2, Some(f), _) => f,
        (3, Some(f), _) => f.min(threshold),
        _ => threshold,
    }
}

impl Db {
    /// Credits markers of an item: set by hand (extra.credits_ms), else from
    /// chapters, but only when credits markers are generated at all (server
    /// setting not "never" and the library's credits detection on).
    pub fn credits_markers(&self, it: &Item) -> Vec<u64> {
        if let Some(a) = it.extra.get("credits_ms").and_then(|v| v.as_array()) {
            return a.iter().filter_map(|v| v.as_u64()).collect();
        }
        let library_on = self.sections.iter().find(|s| s.id == it.section_id).is_none_or(|s| s.credits_detection);
        if self.settings.credits_markers_behaviour == "never" || !library_on {
            return vec![];
        }
        // Found in the last file; counted from the start of the whole movie.
        let start = it.last_file_start_ms();
        it.last_file().map(|f| f.credits_ms.iter().map(|c| c + start).collect()).unwrap_or_default()
    }
}

fn auto_quality() -> String {
    "automatic".into()
}
fn h264_preset() -> String {
    "veryfast".into()
}
fn min_crf() -> u32 {
    16
}
fn lang() -> String {
    "en".into()
}

/// The "Video played threshold" choices (percent).
pub const PLAYED_THRESHOLDS: &[u32] = &[70, 75, 80, 85, 90, 95];
fn cw_weeks() -> u64 {
    16
}
fn cw_limit() -> usize {
    40
}
fn seg_timeout() -> u64 {
    20
}

/// The scan interval choices, in minutes (15 minutes … 1 day).
pub const SCAN_INTERVALS: &[(u64, &str)] = &[
    (15, "every 15 minutes"),
    (30, "every 30 minutes"),
    (60, "hourly"),
    (120, "every 2 hours"),
    (360, "every 6 hours"),
    (720, "every 12 hours"),
    (1440, "daily"),
];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            server_name: crate::server::APP_NAME.into(),
            // Use the hardware when it works.
            hw_accel: "auto".into(),
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            scan_interval_minutes: 60,
            watched_threshold_percent: 90,
            hls_segment_seconds: 6,
            // 0 = no limit.
            max_transcodes: 0,
            invite_default_days: 7,
            public_url: String::new(),
            transcoder_quality: auto_quality(),
            continue_watching_weeks: cw_weeks(),
            continue_watching_limit: cw_limit(),
            segmented_transcoder_timeout_seconds: seg_timeout(),
            scheduled_scans_enabled: false,
            h264_preset: h264_preset(),
            h264_minimum_crf: min_crf(),
            continue_watching_include_premieres: true,
            metadata_enabled: true,
            metadata_language: lang(),
            tmdb_api_key: String::new(),
            omdb_api_key: String::new(),
            // Whichever comes first: the threshold or the first credits marker.
            played_at_behaviour: played_at(),
            hw_constant_quality: false,
            credits_markers_behaviour: asap(),
            cast_photos_limit: cast_photos(),
            port: port(),
            watch_folders: true,
            credits_from_picture: true,
            imdb_ratings_file: true,
            empty_trash_automatically: true,
            allow_media_deletion: true,
            download_keep_days: keep_days(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Db {
    pub version: u32,
    pub machine_id: String,
    pub next_id: u64,
    pub settings: Settings,
    pub sections: Vec<Section>,
    pub items: Tracked<u64, Item>,
    pub users: Vec<User>,
    pub invites: Vec<Invite>,
    pub tokens: Vec<Token>,
    pub playlists: Vec<Playlist>,
    /// key "user_id:item_id"
    pub views: Tracked<String, ViewState>,
    pub plugins: BTreeMap<String, PluginState>,
    #[serde(default)]
    pub history: Vec<PlayEvent>,
    #[serde(default)]
    pub reports: Vec<ProblemReport>,
    #[serde(default)]
    pub downloads: Vec<Download>,
}

impl Default for Db {
    fn default() -> Self {
        let mut db = Db {
            version: 1,
            machine_id: random_hex(16),
            next_id: 1,
            settings: Settings::default(),
            sections: vec![],
            items: Tracked::default(),
            users: vec![],
            invites: vec![],
            tokens: vec![],
            playlists: vec![],
            views: Tracked::default(),
            plugins: BTreeMap::new(),
            history: vec![],
            reports: vec![],
            downloads: vec![],
        };
        db.ensure_default_playlists();
        db
    }
}

impl Db {
    /// Reads an older version's JSON library file.
    pub fn load_json(path: &Path) -> Db {
        let mut db = match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Db>(&bytes) {
                Ok(db) => db,
                Err(e) => {
                    let backup = path.with_extension("broken.json");
                    let _ = std::fs::copy(path, &backup);
                    eprintln!(
                        "library file could not be read ({e}); a copy was kept at {}",
                        backup.display()
                    );
                    Db::default()
                }
            },
            Err(_) => Db::default(),
        };
        db.after_load();
        db
    }

    /// Updates that apply to a library however it was read.
    pub fn after_load(&mut self) {
        // Servers still called by the old default name get the new one; a name
        // chosen by the owner stays.
        if matches!(self.settings.server_name.trim(), "Home Media Server" | "Media Server") {
            self.settings.server_name = crate::server::APP_NAME.into();
        }
        self.ensure_default_playlists();
    }

    pub fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Adds a viewing to the play history.
    /// A watched movie or episode leaves the watchlist; a season or show once
    /// every episode of it is watched.
    pub fn record_play(&mut self, user_id: u64, item_id: u64, manual: bool) {
        self.history.push(PlayEvent { user_id, item_id, at: now(), manual });
        let mut up = vec![item_id];
        let mut at = self.items.get(&item_id).and_then(|i| i.parent_id);
        while let Some(p) = at {
            up.push(p);
            at = self.items.get(&p).and_then(|i| i.parent_id);
        }
        for (n, id) in up.into_iter().enumerate() {
            if self.views.get(&format!("{user_id}:{id}")).is_none_or(|v| v.watchlist_at == 0) {
                continue;
            }
            let done = n == 0 || self.leaves(id).iter().all(|l| *l == item_id || self.view(user_id, *l).view_count > 0);
            if done {
                self.view_mut(user_id, id).watchlist_at = 0;
            }
        }
        if self.history.len() > HISTORY_MAX {
            let extra = self.history.len() - HISTORY_MAX;
            self.history.drain(..extra);
        }
    }

    pub fn user(&self, id: u64) -> Option<&User> {
        self.users.iter().find(|u| u.id == id)
    }

    pub fn view(&self, user: u64, item: u64) -> ViewState {
        self.views
            .get(&format!("{user}:{item}"))
            .cloned()
            .unwrap_or_default()
    }

    pub fn view_mut(&mut self, user: u64, item: u64) -> &mut ViewState {
        self.views.entry(format!("{user}:{item}")).or_default()
    }

    pub fn can_see_section(&self, user: &User, section: u64) -> bool {
        user.admin
            || match &user.allowed_sections {
                None => true,
                Some(list) => list.contains(&section),
            }
    }

    pub fn children(&self, parent: u64) -> Vec<&Item> {
        let mut v: Vec<&Item> = self
            .items
            .values()
            .filter(|i| i.parent_id == Some(parent))
            .collect();
        v.sort_by(|a, b| {
            a.index
                .unwrap_or(0)
                .cmp(&b.index.unwrap_or(0))
                .then(a.sort_title.cmp(&b.sort_title))
        });
        v
    }

    /// All playable items below an item (a show → its episodes).
    pub fn leaves(&self, id: u64) -> Vec<u64> {
        let Some(item) = self.items.get(&id) else {
            return vec![];
        };
        if item.kind.playable() {
            return vec![id];
        }
        let mut out = vec![];
        for c in self.children(id) {
            out.extend(self.leaves(c.id));
        }
        out
    }

    /// The default playlists every user gets. They are smart playlists whose
    /// rules are worked out per user (their own watch state).
    pub fn ensure_default_playlists(&mut self) {
        let defaults: Vec<(&str, &str, &str, SmartQuery)> = vec![
            (
                "continue",
                "Continue Watching",
                "Everything you started but did not finish.",
                SmartQuery {
                    match_all: true,
                    rules: vec![rule("in_progress", "is", Value::Bool(true))],
                    sort: "last_viewed_at:desc".into(),
                    limit: Some(50),
                    sections: vec![],
                },
            ),
            (
                "recent_movies",
                "Recently Added Movies",
                "Movies added in the last 30 days.",
                SmartQuery {
                    match_all: true,
                    rules: vec![
                        rule("kind", "is", Value::from("movie")),
                        rule("added_at", "in_last_days", Value::from(30)),
                    ],
                    sort: "added_at:desc".into(),
                    limit: Some(50),
                    sections: vec![],
                },
            ),
            (
                "recent_episodes",
                "Recently Added Episodes",
                "TV episodes added in the last 30 days.",
                SmartQuery {
                    match_all: true,
                    rules: vec![
                        rule("kind", "is", Value::from("episode")),
                        rule("added_at", "in_last_days", Value::from(30)),
                    ],
                    sort: "added_at:desc".into(),
                    limit: Some(50),
                    sections: vec![],
                },
            ),
            (
                "unwatched_movies",
                "Unwatched Movies",
                "Movies you have not seen yet.",
                SmartQuery {
                    match_all: true,
                    rules: vec![
                        rule("kind", "is", Value::from("movie")),
                        rule("watched", "is", Value::Bool(false)),
                    ],
                    sort: "title".into(),
                    limit: None,
                    sections: vec![],
                },
            ),
            (
                "favorites",
                "Favorites",
                "Everything you marked with a heart.",
                SmartQuery {
                    match_all: true,
                    rules: vec![rule("favorite", "is", Value::Bool(true))],
                    sort: "title".into(),
                    limit: None,
                    sections: vec![],
                },
            ),
            (
                "top_rated",
                "Top Rated",
                "Rated 8 or higher.",
                SmartQuery {
                    match_all: true,
                    rules: vec![rule("rating", "gte", Value::from(8))],
                    sort: "rating:desc".into(),
                    limit: Some(100),
                    sections: vec![],
                },
            ),
        ];
        for (key, title, summary, query) in defaults {
            if self
                .playlists
                .iter()
                .any(|p| p.default_key.as_deref() == Some(key))
            {
                continue;
            }
            let id = self.new_id();
            let t = now();
            self.playlists.push(Playlist {
                id,
                owner_id: 0,
                title: title.into(),
                summary: summary.into(),
                kind: PlaylistKind::Smart,
                items: vec![],
                query: Some(query),
                default_key: Some(key.into()),
                shared: true,
                pinned: true,
                created_at: t,
                updated_at: t,
            });
        }
    }
}

pub fn rule(field: &str, op: &str, value: Value) -> Rule {
    Rule {
        field: field.into(),
        op: op.into(),
        value,
    }
}

/// Lower-case title with leading articles moved off, for sorting.
pub fn sort_title(title: &str) -> String {
    let t = title.trim();
    let lower = t.to_lowercase();
    // The ArticleStrings default: "the,das,der,a,an,el,la".
    for article in ["the ", "das ", "der ", "a ", "an ", "el ", "la "] {
        if lower.starts_with(article) && lower.len() > article.len() {
            return lower[article.len()..].to_string();
        }
    }
    lower
}
