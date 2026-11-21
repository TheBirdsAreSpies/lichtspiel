//! People: users with their age limit, invites and sign-in tokens.

use super::*;

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

impl User {
    /// The name other people see: the full name, or the user name when none is set.
    pub fn display_name(&self) -> String {
        let n = self.full_name.trim();
        if n.is_empty() { self.username.clone() } else { n.to_string() }
    }
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
