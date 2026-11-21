//! Import from the old server (the one Lichtspiel replaces): its
//! libraries and folders, every account's watched state, resume points and
//! ratings, manual playlists, credits markers, and titles/plots.
//!
//! It reads the old server's library database (an SQLite file). A copy is
//! made first, so the old server's own file is never changed and it may keep
//! running. Files are matched by their path; folder paths can be translated
//! (e.g. `D:\Filme` on the old PC → `/media/Filme` in Docker).

use crate::data::db::{self, now, Playlist, PlaylistKind, Section, SectionKind};
use crate::server::{apply_metadata, App};
use rusqlite::types::Value as Sql;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod read;
mod paths;
mod import;

pub use read::*;
pub use paths::*;
pub use import::*;

#[cfg(test)]
mod tests;
