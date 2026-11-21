//! What people do: watch state, the play history, play sessions, downloads and problem reports.

use super::*;

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
