//! The episodes of a show that are still to come.

use super::*;

/// One episode still to come: (season, episode, name, air date YYYY-MM-DD).
pub type ComingEpisode = (u32, u32, String, String);

/// The episodes of a show that air from `from` (YYYY-MM-DD) on, and whether the show has ended.
/// TMDB with a key and a TMDB id; else TVmaze (its own id, or found by TheTVDB / IMDb id).
pub fn coming_episodes(settings: &Settings, ids: &Value, from: &str) -> Result<(bool, Vec<ComingEpisode>), String> {
    let id = |k: &str| ids[k].as_str().map(String::from).or(ids[k].as_u64().map(|n| n.to_string())).filter(|s| !s.is_empty() && s != "0");
    if let (false, Some(tmdb)) = (settings.tmdb_api_key.trim().is_empty(), id("tmdb")) {
        let v = tmdb_get(settings, &format!("/tv/{}", enc(&tmdb)))?;
        let ended = matches!(v["status"].as_str(), Some("Ended" | "Canceled"));
        let Some(season) = v["next_episode_to_air"]["season_number"].as_u64() else { return Ok((ended, vec![])) };
        let s = tmdb_get(settings, &format!("/tv/{}/season/{season}?language={}", enc(&tmdb), enc(&lang_of(settings))))?;
        let list = s["episodes"].as_array().cloned().unwrap_or_default().iter().filter_map(|e| {
            let date = e["air_date"].as_str().filter(|d| *d >= from)?.to_string();
            Some((season as u32, e["episode_number"].as_u64()? as u32, e["name"].as_str().unwrap_or("").to_string(), date))
        }).collect();
        return Ok((ended, list));
    }
    let tvmaze = match id("tvmaze") {
        Some(t) => t,
        None => {
            let (k, q) = if let Some(t) = id("tvdb") { ("thetvdb", t) } else if let Some(i) = id("imdb") { ("imdb", i) } else { return Err("no id TVmaze knows".into()) };
            get_json(&format!("{TVMAZE}/lookup/shows?{k}={}", enc(&q)), None)?["id"].as_u64().ok_or("not at TVmaze")?.to_string()
        }
    };
    let v = get_json(&format!("{TVMAZE}/shows/{}?embed=episodes", enc(&tvmaze)), None)?;
    Ok((v["status"].as_str() == Some("Ended"), tvmaze_coming(&v, from)))
}

/// The episodes in a TVmaze show (with `_embedded.episodes`) that air from `from` on.
pub(super) fn tvmaze_coming(v: &Value, from: &str) -> Vec<ComingEpisode> {
    v["_embedded"]["episodes"].as_array().cloned().unwrap_or_default().iter().filter_map(|e| {
        let date = e["airdate"].as_str().filter(|d| !d.is_empty() && *d >= from)?.to_string();
        Some((e["season"].as_u64()? as u32, e["number"].as_u64()? as u32, e["name"].as_str().unwrap_or("").to_string(), date))
    }).collect()
}
