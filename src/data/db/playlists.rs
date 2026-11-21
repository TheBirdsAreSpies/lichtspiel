//! Playlists and smart playlist rules.

use super::*;

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

pub(super) fn yes() -> bool {
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


pub fn rule(field: &str, op: &str, value: Value) -> Rule {
    Rule {
        field: field.into(),
        op: op.into(),
        value,
    }
}
