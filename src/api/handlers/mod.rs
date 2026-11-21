//! The larger request handlers: accounts and invites, playlists, playback
//! progress, direct streaming with seeking, transcoding (progressive MP4
//! and HLS), subtitles, posters, folder browsing and plugins.

use crate::accounts::auth;
use crate::data::db::{self, now, Db, Item, ItemKind, Playlist, PlaylistKind, SmartQuery, User};
use crate::admin::plugins;
use crate::library::scanner;
use crate::server::*;
use crate::playback::transcode::{self, Method};
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_http::{Response, StatusCode};

mod accounts;
mod playlists;
mod streaming;
mod pictures;
mod folders;
mod plugin_api;
mod extras;

pub use accounts::*;
pub use playlists::*;
pub use streaming::*;
pub use pictures::*;
pub use folders::*;
pub use plugin_api::*;
pub use extras::*;
