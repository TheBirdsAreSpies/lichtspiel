//! Online details for movies and TV shows: plot, cast, directors, writers,
//! genres, studio, age rating, rating, posters, backgrounds, season posters
//! and episode titles / plots / pictures.
//!
//! Sources (no account needed unless noted):
//! - TMDB (themoviedb.org) for movies and shows, when a free API key is set
//!   in Settings: best matches, cast photos, plots in the chosen language.
//! - Without a key: movies from Wikidata + Wikipedia (plot section of the
//!   article, cast, crew, genres, poster), shows from TVmaze (plot, cast,
//!   posters, backgrounds, every episode).

use crate::data::db::{ItemKind, Settings};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

mod common;
mod search;
mod tmdb;
mod tvmaze;
mod wiki;
mod scores;
mod coming;

pub use common::*;
pub use search::*;
pub use tmdb::*;
use tvmaze::*;
pub use wiki::*;
pub use scores::*;
pub use coming::*;

#[cfg(test)]
mod tests;
