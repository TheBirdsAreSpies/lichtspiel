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
    let (y, m, d) = crate::rss::civil((now() / 86400) as i64);
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
    /// A home profile: shown on the sign-in screen of browsers in the local network,
    /// picked with a tap (and the PIN, if one is set).
    #[serde(default)]
    pub home: bool,
    /// PBKDF2 of the PIN; empty = no PIN.
    #[serde(default)]
    pub pin_hash: String,
    #[serde(default)]
    pub pin_salt: String,
    /// Parental control: the highest age rating this person may see (None = no limit).
    #[serde(default)]
    pub max_age: Option<u32>,
    /// With a limit: also hide titles that have no age rating.
    #[serde(default)]
    pub block_unrated: bool,
    /// Private key in the address of this person's watchlist feed; empty = no feed.
    #[serde(default)]
    pub rss_key: String,
    #[serde(default)]
    pub full_name: String,
    /// Base32 secret of the authenticator app; empty = no two-step sign-in.
    #[serde(default)]
    pub totp_secret: String,
    /// A secret being set up (not active until a code from the app was typed in).
    #[serde(default)]
    pub totp_pending: String,
    /// Settings that follow this person from browser to browser (audio, subtitles, watched mark): key → text.
    #[serde(default)]
    pub prefs: std::collections::BTreeMap<String, String>,
}

/// The minimum age an age rating stands for ("FSK 12", "PG-13", "TV-MA", "de/16" …).
pub fn rating_age(rating: &str) -> Option<u32> {
    let r = rating.trim().to_lowercase();
    let r = r.rsplit('/').next().unwrap_or("").trim().to_string();
    let r = r.trim_start_matches("fsk").trim_start_matches("ab").trim().trim_end_matches("jahren").trim_end_matches('+').trim().to_string();
    if let Ok(n) = r.parse::<u32>() { return Some(n); }
    Some(match r.as_str() {
        "o.a." | "oa" | "g" | "tv-g" | "tv-y" | "u" | "all" | "tv-y7" => 0,
        "pg" | "tv-pg" => 8,
        "pg-13" => 13,
        "tv-14" => 14,
        "r" | "tv-ma" | "ma" | "15+" => 17,
        "nc-17" | "x" | "xxx" | "r18" => 18,
        _ => return None,
    })
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
    /// When and from where it was used last (unix seconds, a device name, an address).
    #[serde(default)]
    pub last_used: u64,
    #[serde(default)]
    pub last_device: String,
    #[serde(default)]
    pub last_ip: String,
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
    /// Taken off Continue Watching at this time (unix seconds); it comes back once watched again.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cw_hidden_at: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One viewing in the play history: counted when the played threshold is
/// passed, or marked watched by hand (`manual`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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

/// One time someone played a movie or episode (watched to the end or not), for
/// the dashboard's play statistics: when it started and how long it really ran.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlaySession {
    pub user_id: u64,
    pub item_id: u64,
    pub at: u64,
    /// Time spent playing (pauses don't count).
    pub watched_ms: u64,
}

/// Shorter plays are not counted (a look in, a wrong click).
pub const SESSION_MIN_MS: u64 = 60_000;
/// The statistics keep at most this many plays (the oldest go first).
pub const SESSIONS_MAX: usize = 50000;

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
    /// Which graphics device to use: for VAAPI the render node (empty = /dev/dri/renderD128), for NVENC the card's number (empty = 0).
    #[serde(default)]
    pub hw_device: String,
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
    /// HDR video that is converted for a browser is tone mapped to normal (SDR) colours, else it looks
    /// grey and washed out. Needs an ffmpeg with the zscale and tonemap filters.
    #[serde(default = "yes")]
    pub hdr_tone_mapping: bool,
    /// "Generate credits video markers": "never", "scheduled" or "asap" (default:
    /// as a scheduled task and when media is added).
    #[serde(default = "asap")]
    pub credits_markers_behaviour: String,
    /// "Generate intro video markers": "never", "scheduled" or "asap" (default "asap").
    #[serde(default = "asap")]
    pub intro_markers_behaviour: String,
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
    /// Home profiles can be picked on the sign-in screen (in the local network).
    #[serde(default = "yes")]
    pub home_profiles: bool,
    /// Reverse proxies whose X-Forwarded-For / X-Real-IP / Forwarded header is believed:
    /// addresses or ranges (192.168.1.5, 172.16.0.0/12, ::1), separated by commas or spaces.
    #[serde(default)]
    pub trusted_proxies: String,
    /// Further addresses or ranges that count as the home network (a VPN, a routed subnet), besides the private ranges.
    #[serde(default)]
    pub lan_networks: String,
    /// The network card's address to listen on (empty = all). Needs a restart.
    #[serde(default)]
    pub bind_address: String,
    /// HTTPS: the certificate (PEM file, with the chain) and its private key (PEM file). Both set =
    /// the server speaks HTTPS on its port; both empty = plain HTTP. Needs a restart.
    #[serde(default)]
    pub tls_cert: String,
    #[serde(default)]
    pub tls_key: String,
    /// With HTTPS: a second port that answers plain http:// with a redirect to https:// (0 = none, e.g. 80).
    /// Needs a restart.
    #[serde(default)]
    pub http_redirect_port: u16,
    /// Relay: address ("host:port") of a computer on the internet running `lichtspiel --relay`, and the
    /// secret shared with it. Callers from outside reach this server through it, without a port forward.
    #[serde(default)]
    pub relay_server: String,
    #[serde(default)]
    pub relay_secret: String,
    /// Answer apps that search the home network for servers (GDM).
    #[serde(default = "yes")]
    pub gdm_enabled: bool,
    /// Maintenance window (local hours): scheduled tasks only start between these.
    #[serde(default = "maint_start")]
    pub maintenance_start_hour: u32,
    #[serde(default = "maint_end")]
    pub maintenance_end_hour: u32,
    /// The browser's offset from UTC in minutes (sent when settings are saved), so hours are local.
    #[serde(default)]
    pub utc_offset_minutes: i64,
    /// A copy of the library file every 3 days, the newest few kept.
    #[serde(default = "yes")]
    pub backup_enabled: bool,
    /// Empty = the "backups" folder in the data folder.
    #[serde(default)]
    pub backup_dir: String,
    #[serde(default = "backups_keep")]
    pub backups_keep: u32,
    /// Memory for the library file's page cache, in MB.
    #[serde(default = "db_cache_mb")]
    pub db_cache_mb: u32,
    #[serde(default = "yes")]
    pub optimize_weekly: bool,
    #[serde(default = "yes")]
    pub cleanup_weekly: bool,
    /// Every three days, inside the maintenance window, all libraries are read again.
    #[serde(default = "yes")]
    pub refresh_metadata: bool,
    /// Show online trailers (from TMDB / Wikidata) on movie and show pages.
    #[serde(default = "yes")]
    pub trailers_online: bool,
    /// Download a show's theme music from the online list when its folder has
    /// none (the theme music helper, by TheTVDB number).
    #[serde(default = "yes")]
    pub theme_music_online: bool,
    /// What of the online details is loaded (all on by default): people, pictures, seasons and episodes, and the
    /// biography and filmography on a person's page.
    #[serde(default = "yes")]
    pub meta_cast: bool,
    #[serde(default = "yes")]
    pub meta_artwork: bool,
    #[serde(default = "yes")]
    pub meta_episodes: bool,
    #[serde(default = "yes")]
    pub meta_ratings: bool,
    #[serde(default = "yes")]
    pub meta_person_details: bool,
    /// Also look for English trailers when the library's language has none.
    #[serde(default = "yes")]
    pub trailers_english: bool,
    /// Allow red band (restricted) trailers.
    #[serde(default)]
    pub trailers_red_band: bool,
    /// On the home page, new episodes are shown as their show (once per show).
    #[serde(default)]
    pub group_recent_episodes: bool,
    /// Measure how loud each video is (during maintenance) so the player can level the volume.
    #[serde(default)]
    pub analyze_loudness: bool,
    /// "Deep analysis" as a scheduled task (off by default).
    #[serde(default)]
    pub deep_analysis: bool,
    #[serde(default)]
    pub generate_thumbnails: bool,
    /// Surround sound converted to stereo: lift the dialogue (the centre channel) so voices stay clear.
    #[serde(default = "yes")]
    pub boost_dialogue: bool,
    /// Streams to people outside the home network are held to this many Mbit/s (0 = no limit).
    #[serde(default)]
    pub remote_max_mbps: u32,
    /// Off: the server never converts video (only files the player can play as they are).
    #[serde(default = "yes")]
    pub transcoding_enabled: bool,
    /// Where live conversions are written (empty = the "transcode" folder in the data folder).
    #[serde(default)]
    pub transcode_dir: String,
    /// Where converted downloads are kept (empty = the "downloads" folder in the data folder).
    #[serde(default)]
    pub downloads_dir: String,
    /// People outside the home network may play this many different videos at once (0 = no limit).
    #[serde(default)]
    pub max_remote_plays: u32,
    /// A play that stays paused this many minutes is ended (0 = never).
    #[serde(default)]
    pub paused_stop_minutes: u32,
    /// Addresses and networks that need no sign-in: they act as the first admin. Only believed
    /// addresses count (see trusted_proxies). Leave empty unless you know why you need it.
    #[serde(default)]
    pub no_auth_networks: String,
    /// Most conversions at once while the graphics chip converts (0 = no limit).
    #[serde(default)]
    pub max_gpu_transcodes: usize,
    /// Most conversions at once while the processor converts (0 = no limit).
    #[serde(default)]
    pub max_cpu_transcodes: usize,
    /// Video files played before a movie (one is picked each time): paths separated by new lines or
    /// semicolons. Files the browser can play (MP4, WebM) work best.
    #[serde(default)]
    pub preroll_paths: String,
    /// Leave articles off when sorting titles ("The Matrix" under M).
    #[serde(default = "yes")]
    pub sort_ignore_articles: bool,
    /// The articles, separated by commas.
    #[serde(default = "articles_default")]
    pub sort_articles: String,
    /// From this many movies of one film series a collection is made (2 = every series).
    #[serde(default = "collection_min_default")]
    pub collection_min_items: u32,
    /// "random" (one at random), "all" (all of them, in the order listed), "first" (always the first).
    #[serde(default = "preroll_mode_default")]
    pub preroll_mode: String,
    /// When each scheduled task last ran (unix seconds).
    #[serde(default)]
    pub maintenance_last: std::collections::BTreeMap<String, u64>,
}

fn maint_start() -> u32 { 2 }
fn maint_end() -> u32 { 5 }
fn backups_keep() -> u32 { 5 }
fn db_cache_mb() -> u32 { 64 }

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
    /// Older versions kept sign-in tokens as they were given out; keep only their hash (they still work).
    pub fn hash_old_tokens(&mut self) {
        for t in self.tokens.iter_mut().filter(|t| t.token.len() != 64) { t.token = crate::auth::token_hash(&t.token); }
    }

    /// Brings a library saved by an older version up to date (at start and after a restore).
    pub fn upgrade(&mut self) {
        if self.ticket_key.is_empty() { self.ticket_key = random_hex(32); }
        self.hash_old_tokens();
    }

    /// Credits markers of an item: set by hand (extra.credits_ms), else from
    /// chapters, but only when credits markers are generated at all (server
    /// setting not "never" and the library's credits detection on).
    pub fn credits_markers(&self, it: &Item) -> Vec<u64> {
        if let Some(a) = it.extra.get("credits_ms").and_then(|v| v.as_array()) {
            return a.iter().filter_map(|v| v.as_u64()).collect();
        }
        let library_on = self.credits_on(it);
        if self.settings.credits_markers_behaviour == "never" || !library_on {
            return vec![];
        }
        // Found in the last file; counted from the start of the whole movie.
        let start = it.last_file_start_ms();
        it.last_file().map(|f| f.credits_ms.iter().map(|c| c + start).collect()).unwrap_or_default()
    }

    /// Intro detection for an item: a TV library with "Enable intro
    /// detection" on, and the server setting not "never" (only
    /// schedules it for show libraries).
    pub fn intro_on(&self, it: &Item) -> bool {
        self.settings.intro_markers_behaviour != "never"
            && self.sections.iter().find(|s| s.id == it.section_id).is_some_and(|s| s.kind == SectionKind::Show && s.intro_detection)
    }

    /// An episode's intro (start, end ms): set by hand (extra.intro_ms), else found.
    pub fn intro_marker(&self, it: &Item) -> Option<(u64, u64)> {
        let pair = |v: &[u64]| (v.len() == 2 && v[1] > v[0]).then(|| (v[0], v[1]));
        if let Some(a) = it.extra.get("intro_ms").and_then(|v| v.as_array()) {
            return pair(&a.iter().filter_map(|v| v.as_u64()).collect::<Vec<_>>());
        }
        if it.kind != ItemKind::Episode || !self.intro_on(it) {
            return None;
        }
        it.file.as_ref().and_then(|f| pair(&f.intro_ms))
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

fn articles_default() -> String {
    DEFAULT_ARTICLES.into()
}

fn collection_min_default() -> u32 {
    2
}

fn preroll_mode_default() -> String {
    "random".into()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            server_name: crate::server::APP_NAME.into(),
            // Use the hardware when it works.
            hw_accel: "auto".into(),
            hw_device: String::new(),
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
            hdr_tone_mapping: true,
            credits_markers_behaviour: asap(),
            intro_markers_behaviour: asap(),
            cast_photos_limit: cast_photos(),
            port: port(),
            watch_folders: true,
            credits_from_picture: true,
            imdb_ratings_file: true,
            empty_trash_automatically: true,
            allow_media_deletion: true,
            download_keep_days: keep_days(),
            home_profiles: true,
            trusted_proxies: String::new(),
            lan_networks: String::new(),
            bind_address: String::new(),
            tls_cert: String::new(),
            tls_key: String::new(),
            http_redirect_port: 0,
            relay_server: String::new(),
            relay_secret: String::new(),
            gdm_enabled: true,
            maintenance_start_hour: maint_start(),
            maintenance_end_hour: maint_end(),
            utc_offset_minutes: 0,
            backup_enabled: true,
            backup_dir: String::new(),
            backups_keep: backups_keep(),
            db_cache_mb: db_cache_mb(),
            optimize_weekly: true,
            cleanup_weekly: true,
            refresh_metadata: true,
            trailers_online: true,
            theme_music_online: true,
            meta_cast: true,
            meta_artwork: true,
            meta_episodes: true,
            meta_ratings: true,
            meta_person_details: true,
            trailers_english: true,
            trailers_red_band: false,
            group_recent_episodes: false,
            analyze_loudness: false,
            deep_analysis: false,
            generate_thumbnails: false,
            boost_dialogue: true,
            remote_max_mbps: 0,
            preroll_paths: String::new(),
            sort_ignore_articles: true,
            sort_articles: DEFAULT_ARTICLES.into(),
            collection_min_items: 2,
            preroll_mode: preroll_mode_default(),
            max_gpu_transcodes: 0,
            max_cpu_transcodes: 0,
            max_remote_plays: 0,
            paused_stop_minutes: 0,
            no_auth_networks: String::new(),
            transcoding_enabled: true,
            transcode_dir: String::new(),
            downloads_dir: String::new(),
            maintenance_last: Default::default(),
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
    /// Plays that have ended, oldest first (see PlaySession).
    #[serde(default)]
    pub sessions: Vec<PlaySession>,
    #[serde(default)]
    pub reports: Vec<ProblemReport>,
    #[serde(default)]
    pub downloads: Vec<Download>,
    /// Signs the short-lived media tickets (auth::ticket); made on first start.
    #[serde(default)]
    pub ticket_key: String,
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
            sessions: vec![],
            reports: vec![],
            downloads: vec![],
            ticket_key: random_hex(32),
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
                    elog!(
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
        self.record_play_at(user_id, item_id, manual, now());
    }

    /// The same for a viewing at another time (imported from elsewhere); the
    /// history stays in time order.
    pub fn record_play_at(&mut self, user_id: u64, item_id: u64, manual: bool, at: u64) {
        let pos = self.history.iter().rposition(|e| e.at <= at).map_or(0, |i| i + 1);
        self.history.insert(pos, PlayEvent { user_id, item_id, at, manual });
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

    /// Keeps an ended play for the statistics (when it ran long enough).
    pub fn record_session(&mut self, s: PlaySession) -> bool {
        if s.watched_ms < SESSION_MIN_MS { return false; }
        let pos = self.sessions.iter().rposition(|e| e.at <= s.at).map_or(0, |i| i + 1);
        self.sessions.insert(pos, s);
        if self.sessions.len() > SESSIONS_MAX {
            let extra = self.sessions.len() - SESSIONS_MAX;
            self.sessions.drain(..extra);
        }
        true
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

    /// Library access plus the person's age limit. Seasons and episodes take the
    /// rating of the nearest parent that has one.
    pub fn can_see_item(&self, user: &User, item: &Item) -> bool {
        if !self.can_see_section(user, item.section_id) { return false; }
        let Some(max) = user.max_age else { return true };
        if user.admin { return true; }
        let mut cur = item;
        for _ in 0..4 {
            if !cur.content_rating.trim().is_empty() {
                return match rating_age(&cur.content_rating) {
                    Some(a) => a <= max,
                    None => !user.block_unrated,
                };
            }
            match cur.parent_id.and_then(|p| self.items.get(&p)) { Some(p) => cur = p, None => break }
        }
        !user.block_unrated
    }

    /// A further file of another title that is still there (several versions of one episode or movie):
    /// shown under that title, not as a title or an episode of its own.
    pub fn is_version(&self, i: &Item) -> bool {
        i.version_of().is_some_and(|m| self.items.contains_key(&m))
    }

    pub fn children(&self, parent: u64) -> Vec<&Item> {
        let mut v: Vec<&Item> = self
            .items
            .values()
            .filter(|i| i.parent_id == Some(parent) && !self.is_version(i))
            .collect();
        // Songs of a double album: disc by disc.
        let disc = |i: &Item| i.extra.get("disc").and_then(|d| d.as_u64()).unwrap_or(0);
        v.sort_by(|a, b| {
            disc(a)
                .cmp(&disc(b))
                .then(a.index.unwrap_or(0).cmp(&b.index.unwrap_or(0)))
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

/// One spelling for age ratings: "12", "FSK12", "DE-12", "ab 12" all become "FSK 12".
pub fn normalize_age_rating(r: &str) -> String {
    let t = r.trim();
    let lower = t.to_lowercase();
    let rest = ["fsk", "de-", "de:", "germany:", "ab", "rated"].iter().find_map(|p| lower.strip_prefix(p)).unwrap_or(&lower);
    let rest = rest.trim_matches(|c: char| c == ' ' || c == '-' || c == '_' || c == ':');
    let rest = rest.strip_suffix("jahren").or_else(|| rest.strip_suffix(" j.")).or_else(|| rest.strip_suffix('j')).unwrap_or(rest).trim();
    match rest {
        "0" | "6" | "12" | "16" | "18" => format!("FSK {rest}"),
        _ => t.to_string(),
    }
}

/// One spelling for genres: "Horrorfilm" → "Horror", "Familienfilm" → "Familie", "Sci-Fi" → "Science Fiction".
pub fn normalize_genre(g: &str) -> String {
    let t = g.trim();
    let lower = t.to_lowercase();
    let mapped = match lower.as_str() {
        "sci-fi" | "scifi" | "science-fiction" | "science fiction" | "sci-fi & fantasy" => Some("Science Fiction"),
        "familienfilm" | "familien" | "family" => Some("Familie"),
        "kriegsfilm" => Some("Krieg"),
        "liebesfilm" | "liebesfilme" | "romanze" => Some("Liebesfilm"),
        _ => None,
    };
    if let Some(m) = mapped { return if lower == "family" || lower == "sci-fi & fantasy" { t.to_string() } else { m.to_string() }; }
    for suffix in ["film", "filme"] {
        if let Some(stem) = lower.strip_suffix(suffix) {
            // "Buddy-Film" → "Buddy" (not "Buddy-"), "Buddy-Cop-Film" → "Buddy-Cop".
            let stem = stem.trim_end_matches('-');
            if stem.chars().count() >= 4 && !stem.ends_with(' ') {
                return stem.split('-').map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join("-");
            }
        }
    }
    t.to_string()
}

impl Item {
    /// Age rating and genres in one spelling (see above).
    pub fn normalize(&mut self) {
        if !self.content_rating.is_empty() { self.content_rating = normalize_age_rating(&self.content_rating); }
        let mut seen = std::collections::HashSet::new();
        self.genres = self.genres.iter().map(|g| normalize_genre(g)).filter(|g| !g.is_empty() && seen.insert(g.to_lowercase())).collect();
    }
}

/// The articles left off when sorting titles (lower case, each with a trailing space); empty = none.
static ARTICLES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
pub const DEFAULT_ARTICLES: &str = "the, a, an, der, die, das, ein, eine, le, la, les, el";

/// Applies the settings "ignore articles when sorting" and their list.
pub fn set_articles(enabled: bool, list: &str) {
    let v: Vec<String> = if enabled {
        list.split([',', ';', '\n']).map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty()).map(|a| format!("{a} ")).collect()
    } else { vec![] };
    if let Ok(mut a) = ARTICLES.lock() { *a = v; }
}

/// Lower-case title with leading articles moved off, for sorting.
pub fn sort_title(title: &str) -> String {
    let t = title.trim();
    let lower = t.to_lowercase();
    if let Ok(articles) = ARTICLES.lock() {
        for article in articles.iter() {
            if lower.starts_with(article.as_str()) && lower.len() > article.len() {
                return lower[article.len()..].trim_start().to_string();
            }
        }
    }
    lower
}

#[cfg(test)]
mod normalize_tests {
    use super::*;
    #[test]
    fn age_ratings_and_genres_get_one_spelling() {
        for r in ["12", "FSK 12", "FSK12", "DE-12", "ab 12", "Germany:12", "FSK-12"] { assert_eq!(normalize_age_rating(r), "FSK 12", "{r}"); }
        assert_eq!(normalize_age_rating("0"), "FSK 0");
        assert_eq!(normalize_age_rating("PG-13"), "PG-13");
        assert_eq!(normalize_age_rating("TV-MA"), "TV-MA");
        assert_eq!(normalize_genre("Horrorfilm"), "Horror");
        assert_eq!(normalize_genre("Familienfilm"), "Familie");
        assert_eq!(normalize_genre("Sci-Fi"), "Science Fiction");
        assert_eq!(normalize_genre("Film Noir"), "Film Noir");
        assert_eq!(normalize_genre("Drama"), "Drama");
        assert_eq!(normalize_genre("Buddy-Film"), "Buddy");
        assert_eq!(normalize_genre("Buddy-Cop-Film"), "Buddy-Cop");
    }
    #[test]
    fn articles_are_left_off_when_asked() {
        set_articles(true, "the, die");
        assert_eq!(sort_title("The Matrix"), "matrix");
        assert_eq!(sort_title("Die Abbotts"), "abbotts");
        set_articles(false, "the");
        assert_eq!(sort_title("The Matrix"), "the matrix");
        set_articles(true, DEFAULT_ARTICLES);
    }
}
