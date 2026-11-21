//! The server's stored data: libraries, items, users, invites, playlists,
//! watch state and plugin settings. All of it is held in memory; it is kept
//! in an SQLite file (see crate::data::store), which a save updates row by row.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use crate::data::store::Tracked;
use std::path::Path;

/// Today's date (UTC) as YYYY-MM-DD.
pub fn today() -> String {
    let (y, m, d) = crate::connect::rss::civil((now() / 86400) as i64);
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

mod library;
mod users;
mod playlists;
mod history;
mod settings;
mod state;
mod text;

pub use library::*;
pub use users::*;
pub use playlists::*;
pub use history::*;
pub use settings::*;
pub use state::*;
pub use text::*;

#[cfg(test)]
mod tests;
