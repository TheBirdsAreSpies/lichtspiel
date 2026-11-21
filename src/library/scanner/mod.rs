//! Finds video files in a library's folders, works out movie titles and
//! show / season / episode numbers from the names, and reads each file's
//! container and codecs with ffprobe.

use crate::data::db::{now, sort_title, Db, Item, ItemKind, MediaFile, Section, SectionKind, Stream};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

mod files;
mod names;
mod probe;
mod credits;
mod sidecars;
mod classify;
mod scan;
mod housekeeping;

pub use files::*;
pub use names::*;
pub use probe::*;
pub use credits::*;
pub use sidecars::*;
pub use classify::*;
pub use scan::*;
pub use housekeeping::*;

#[cfg(test)]
mod tests;
