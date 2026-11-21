//! "Coming soon": the next episodes of the shows in the library (TMDB with a key, else TVmaze)
//! and what plugins add through the `calendar` event (the wish list: Radarr's and Sonarr's
//! calendars). Looked up in the background — a running show once a day, an ended one once a
//! month, the plugins every few hours — and kept in `<data>/upcoming.json`.

use crate::data::db::{now, Db, ItemKind, User};
use crate::server::App;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How far ahead is looked (days).
pub const DAYS_AHEAD: i64 = 90;
const RUNNING_EVERY: u64 = 20 * 3600;
const ENDED_EVERY: u64 = 30 * 86400;
const PLUGINS_EVERY: u64 = 3 * 3600;

/// One thing still to come.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// "episode" or "movie".
    pub kind: String,
    /// The show in the library it belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_id: Option<u64>,
    /// The show's or the movie's title.
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<u32>,
    /// The episode's title.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// YYYY-MM-DD.
    pub date: String,
    /// A picture address (from a plugin; the library's own poster for a show in the library).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    /// What a plugin says about it ("in cinemas", "digital release", "wished" …).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// "tmdb", "tvmaze" or the plugin's id.
    #[serde(default)]
    pub source: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct ShowState {
    checked: u64,
    #[serde(default)]
    ended: bool,
    #[serde(default)]
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Store {
    #[serde(default)]
    shows: BTreeMap<u64, ShowState>,
    #[serde(default)]
    plugins: Vec<Entry>,
    #[serde(default)]
    plugins_checked: u64,
    #[serde(skip)]
    file: PathBuf,
}

impl Store {
    pub fn load(data_dir: &Path) -> Store {
        let file = data_dir.join("upcoming.json");
        let mut s: Store = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        s.file = file;
        s
    }

    fn save(&self) {
        if self.file.as_os_str().is_empty() { return; }
        let tmp = self.file.with_extension("json.tmp");
        if let Ok(b) = serde_json::to_vec(self) {
            if std::fs::write(&tmp, b).is_ok() { let _ = std::fs::rename(&tmp, &self.file); }
        }
    }
}

/// The date `days` from today (UTC), YYYY-MM-DD.
pub fn day(days: i64) -> String {
    let (y, m, d) = crate::connect::rss::civil((now() / 86400) as i64 + days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Shows in the library that are due to be looked up again: (id, ids, title, year).
fn due_shows(db: &Db, store: &Store, at: u64) -> Vec<(u64, Value, String, Option<u32>)> {
    db.items.values()
        .filter(|i| i.kind == ItemKind::Show && !i.extra.contains_key(crate::library::scanner::MISSING))
        .filter(|i| i.extra.get("ids").is_some_and(|v| v.as_object().is_some_and(|o| !o.is_empty())))
        .filter(|i| match store.shows.get(&i.id) {
            None => true,
            Some(s) => at.saturating_sub(s.checked) >= if s.ended { ENDED_EVERY } else { RUNNING_EVERY },
        })
        .map(|i| (i.id, i.extra["ids"].clone(), i.title.clone(), i.year))
        .collect()
}

/// Looks up what is due. Slow (one or two requests a show), so it runs in the background.
pub fn refresh(app: &Arc<App>) {
    let at = now();
    let (settings, due, shows) = {
        let db = app.db.lock().unwrap();
        let store = app.upcoming.lock().unwrap();
        let shows: Vec<u64> = db.items.values().filter(|i| i.kind == ItemKind::Show).map(|i| i.id).collect();
        (db.settings.clone(), due_shows(&db, &store, at), shows)
    };
    let today = day(0);
    let mut changed = false;
    for (id, ids, title, year) in due {
        let looked = crate::library::metadata::coming_episodes(&settings, &ids, &today);
        let source = if !settings.tmdb_api_key.trim().is_empty() && ids.get("tmdb").is_some() { "tmdb" } else { "tvmaze" };
        let mut store = app.upcoming.lock().unwrap();
        match looked {
            Ok((ended, list)) => {
                let entries = list.into_iter().map(|(s, e, name, date)| Entry {
                    kind: "episode".into(), show_id: Some(id), title: title.clone(), year, season: Some(s), episode: Some(e),
                    name, date, source: source.into(), ..Default::default()
                }).collect();
                store.shows.insert(id, ShowState { checked: at, ended, entries });
            }
            // Not found or not reachable: tried again in a day, the old entries stay meanwhile.
            Err(_) => { store.shows.entry(id).or_default().checked = at; }
        }
        changed = true;
        drop(store);
        std::thread::sleep(Duration::from_millis(300));
    }
    {
        let mut store = app.upcoming.lock().unwrap();
        let before = store.shows.len();
        store.shows.retain(|id, _| shows.contains(id));
        changed |= store.shows.len() != before;
    }
    let plugins_due = at.saturating_sub(app.upcoming.lock().unwrap().plugins_checked) >= PLUGINS_EVERY;
    if plugins_due {
        let mut found = vec![];
        for (pl, st) in app.listeners("calendar") {
            let data = json!({"start": today, "end": day(DAYS_AHEAD)});
            match crate::admin::plugins::invoke(&pl, &st, "calendar", &data, &app.server_info_for(&pl)) {
                Ok(v) => {
                    let n = from_plugin(&v, &pl.manifest.id, &mut found);
                    app.plugins.lock().unwrap().note(&pl.manifest.id, "calendar", true, format!("{n} coming"));
                }
                Err(e) => app.plugins.lock().unwrap().note(&pl.manifest.id, "calendar", false, e),
            }
        }
        let mut store = app.upcoming.lock().unwrap();
        store.plugins = found;
        store.plugins_checked = at;
        changed = true;
    }
    if changed { app.upcoming.lock().unwrap().save(); }
}

/// A plugin's answer `{"entries": [{type, title, year, season, episode, name, date, poster, note}]}`.
fn from_plugin(v: &Value, pid: &str, out: &mut Vec<Entry>) -> usize {
    let mut n = 0;
    for e in v["entries"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().take(1000) {
        let date: String = e["date"].as_str().unwrap_or("").chars().take(10).collect();
        let title = e["title"].as_str().unwrap_or("").trim();
        let kind = e["type"].as_str().unwrap_or("movie");
        if title.is_empty() || date.len() != 10 || !matches!(kind, "movie" | "episode") { continue; }
        let num = |k: &str| e[k].as_u64().map(|x| x as u32);
        let poster = e["poster"].as_str().filter(|p| p.starts_with("https://")).map(String::from);
        out.push(Entry {
            kind: kind.into(), show_id: None, title: title.into(), year: num("year"), season: num("season"), episode: num("episode"),
            name: e["name"].as_str().unwrap_or("").into(), date, poster, note: e["note"].as_str().unwrap_or("").chars().take(60).collect(),
            source: pid.into(),
        });
        n += 1;
    }
    n
}

/// What `user` may see from `from` to `to` (YYYY-MM-DD), in date order. A plugin's episode of a
/// show in the library is linked to it, and left out when the library's lookup has it already.
/// Titles from plugins are not in the library, so people with an age limit don't get them.
pub fn list(db: &Db, store: &Store, user: &User, from: &str, to: &str) -> Vec<Entry> {
    let visible = |id: u64| db.items.get(&id).is_some_and(|i| db.can_see_item(user, i));
    let mut out: Vec<Entry> = store.shows.iter().filter(|(id, _)| visible(**id)).flat_map(|(_, s)| s.entries.iter().cloned()).collect();
    let limited = !user.admin && (user.max_age.is_some() || user.block_unrated);
    if !limited {
        let by_title: BTreeMap<String, u64> = db.items.values().filter(|i| i.kind == ItemKind::Show && !i.extra.contains_key(crate::library::scanner::MISSING))
            .map(|i| (crate::library::metadata::normalize(&i.title), i.id)).collect();
        for e in &store.plugins {
            let mut e = e.clone();
            if e.kind == "episode" {
                if let Some(&sid) = by_title.get(&crate::library::metadata::normalize(&e.title)) {
                    if !visible(sid) { continue; }
                    if out.iter().any(|o| o.show_id == Some(sid) && o.season == e.season && o.episode == e.episode) { continue; }
                    e.show_id = Some(sid);
                }
            }
            out.push(e);
        }
    }
    out.retain(|e| e.date.as_str() >= from && e.date.as_str() <= to);
    out.sort_by(|a, b| (&a.date, &a.title, a.season, a.episode).cmp(&(&b.date, &b.title, b.season, b.episode)));
    out
}

/// The answer of GET /api/upcoming: the entries with the library's poster for shows in it.
pub fn answer(db: &Db, store: &Store, user: &User, days: i64) -> Value {
    let (from, to) = (day(0), day(days.clamp(1, DAYS_AHEAD)));
    let entries: Vec<Value> = list(db, store, user, &from, &to).into_iter().map(|e| {
        let mut v = serde_json::to_value(&e).unwrap_or(Value::Null);
        if let Some(show) = e.show_id.and_then(|id| db.items.get(&id)) {
            v["thumb"] = json!(format!("/library/metadata/{}/thumb?v={}", show.id, show.updated_at));
            v["title"] = json!(show.title);
        }
        v
    }).collect();
    json!({"today": from, "entries": entries})
}

/// Started once: looks up what is due every 10 minutes (the first time a minute after the start).
pub fn start(app: &Arc<App>) {
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            refresh(&a);
            std::thread::sleep(Duration::from_secs(600));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(show: u64, title: &str, s: u32, e: u32, date: &str) -> Entry {
        Entry { kind: "episode".into(), show_id: Some(show), title: title.into(), season: Some(s), episode: Some(e), date: date.into(), source: "tmdb".into(), ..Default::default() }
    }

    fn db_with_shows() -> Db {
        let mut db = Db::default();
        for (id, title, section) in [(10u64, "Dark", 1u64), (11, "Severance", 2)] {
            db.items.insert(id, serde_json::from_value(json!({"id": id, "section_id": section, "kind": "show", "title": title, "sort_title": title, "added_at": 0, "updated_at": 5,
                "extra": {"ids": {"tvdb": id}}})).unwrap());
        }
        db.users.push(serde_json::from_value(json!({"id": 1, "username": "admin", "password_hash": "x", "salt": "x", "admin": true, "created_at": 0})).unwrap());
        db.users.push(serde_json::from_value(json!({"id": 2, "username": "kid", "password_hash": "x", "salt": "x", "admin": false, "created_at": 0, "allowed_sections": [1], "max_age": 12})).unwrap());
        db
    }

    #[test]
    fn each_person_gets_what_they_may_see_in_date_order_with_plugin_titles_merged() {
        let db = db_with_shows();
        let mut store = Store::default();
        store.shows.insert(10, ShowState { checked: 1, ended: false, entries: vec![ep(10, "Dark", 4, 2, "2026-12-03"), ep(10, "Dark", 4, 1, "2026-11-26")] });
        store.shows.insert(11, ShowState { checked: 1, ended: false, entries: vec![ep(11, "Severance", 3, 1, "2026-12-01")] });
        let mut found = vec![];
        from_plugin(&json!({"entries": [
            {"type": "movie", "title": "Dune: Part Three", "year": 2026, "date": "2026-12-18T00:00:00Z", "poster": "https://img/x.jpg", "note": "in cinemas"},
            {"type": "episode", "title": "Dark", "season": 4, "episode": 1, "date": "2026-11-26"},
            {"type": "episode", "title": "Severance", "season": 3, "episode": 2, "date": "2026-12-08"},
            {"type": "episode", "title": "Andor", "season": 3, "episode": 1, "date": "2026-11-30", "poster": "http://insecure/x.jpg"},
            {"type": "movie", "title": "", "date": "2026-12-01"},
            {"type": "movie", "title": "Too late", "date": "2027-12-01"}]}), "wishlist", &mut found);
        store.plugins = found;
        let admin = db.users[0].clone();
        let got = list(&db, &store, &admin, "2026-11-21", "2027-02-19");
        let names: Vec<String> = got.iter().map(|e| format!("{} {}", e.date, e.title)).collect();
        assert_eq!(names, ["2026-11-26 Dark", "2026-11-30 Andor", "2026-12-01 Severance", "2026-12-03 Dark", "2026-12-08 Severance", "2026-12-18 Dune: Part Three"]);
        // Sonarr's Severance episode belongs to the show in the library; Andor is not in it; no http picture.
        assert_eq!(got[4].show_id, Some(11));
        assert_eq!((got[1].show_id, &got[1].poster), (None, &None));
        assert_eq!(got[5].note, "in cinemas");
        // The kid sees only their library, and nothing from plugins (an age limit).
        let kid = db.users[1].clone();
        let got = list(&db, &store, &kid, "2026-11-21", "2027-02-19");
        assert_eq!(got.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Dark", "Dark"]);
    }

    #[test]
    fn a_running_show_is_due_daily_and_an_ended_one_monthly() {
        let db = db_with_shows();
        let mut store = Store::default();
        let at = 100 * 86400;
        assert_eq!(due_shows(&db, &store, at).len(), 2);
        store.shows.insert(10, ShowState { checked: at - 2 * 86400, ended: true, entries: vec![] });
        store.shows.insert(11, ShowState { checked: at - 2 * 86400, ended: false, entries: vec![] });
        assert_eq!(due_shows(&db, &store, at).iter().map(|d| d.0).collect::<Vec<_>>(), vec![11]);
        store.shows.get_mut(&10).unwrap().checked = at - 31 * 86400;
        assert_eq!(due_shows(&db, &store, at).len(), 2);
    }
}
