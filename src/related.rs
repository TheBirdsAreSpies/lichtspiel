//! A library's Collections and Categories pages, and the rows under a movie
//! or show ("Related Movies", "More by …", "More with …").

use crate::db::{Db, Item, ItemKind, SectionKind, User};
use crate::query::{field, FieldValue};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// How many items a row shows by default (`count`).
pub const HUB_COUNT: usize = 6;
/// "More with {1}" rows for the first this many actors.
pub const ACTOR_ROWS: usize = 2;
/// Similarity weights for "Related Movies"/"Related Shows" (chosen by hand).
pub const SIMILAR_MIN_SCORE: f64 = 2.0;

fn texts(db: &Db, user_id: u64, it: &Item, name: &str) -> Vec<String> {
    match field(db, user_id, it, name) {
        FieldValue::Text(v) => v.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        _ => vec![],
    }
}

fn top_kind(kind: SectionKind) -> ItemKind {
    match kind {
        SectionKind::Movie => ItemKind::Movie,
        SectionKind::Show => ItemKind::Show,
        SectionKind::Other => ItemKind::Video,
    }
}

fn top_items<'a>(db: &'a Db, sid: u64) -> Vec<&'a Item> {
    let Some(sec) = db.sections.iter().find(|s| s.id == sid) else { return vec![] };
    let kind = top_kind(sec.kind);
    let mut v: Vec<&Item> = db.items.values().filter(|i| i.section_id == sid && i.kind == kind).collect();
    v.sort_by(|a, b| a.sort_title.cmp(&b.sort_title));
    v
}

/// One entry per name of `field` (collection or genre) with its items.
fn groups<'a>(db: &'a Db, user: &User, sid: u64, name: &str) -> Vec<(String, Vec<&'a Item>)> {
    let mut map: BTreeMap<String, (String, Vec<&Item>)> = BTreeMap::new();
    for it in top_items(db, sid) {
        for n in texts(db, user.id, it, name) {
            map.entry(n.to_lowercase()).or_insert_with(|| (n.clone(), vec![])).1.push(it);
        }
    }
    map.into_values().collect()
}

fn directory(sid: u64, title: &str, filter: &str, items: &[&Item]) -> Value {
    // The picture of the newest item stands for the group.
    let pic = items.iter().max_by_key(|i| (i.year.unwrap_or(0), i.added_at)).copied();
    json!({
        "title": title,
        "type": filter,
        "key": format!("/library/sections/{sid}/all?{filter}={}", crate::metadata::enc(title)),
        "filter": filter,
        "childCount": items.len(),
        "thumb": pic.map(|i| format!("/library/metadata/{}/thumb?v={}", i.id, i.updated_at)),
        "art": pic.map(|i| format!("/library/metadata/{}/art?v={}", i.id, i.updated_at)),
        "ratingKeys": items.iter().take(4).map(|i| i.id).collect::<Vec<_>>(),
    })
}

/// The library's collections (film series and the ones set by hand).
pub fn collections(db: &Db, user: &User, sid: u64) -> Vec<Value> {
    groups(db, user, sid, "collection").iter().map(|(t, its)| directory(sid, t, "collection", its)).collect()
}

/// The library's categories: one per genre, with a picture.
pub fn categories(db: &Db, user: &User, sid: u64) -> Vec<Value> {
    groups(db, user, sid, "genre").iter().map(|(t, its)| directory(sid, t, "genre", its)).collect()
}

/// How much `b` is like `a` (estimated weights).
pub fn similarity(db: &Db, user_id: u64, a: &Item, b: &Item) -> f64 {
    let shared = |name: &str, limit: usize| -> usize {
        let av: Vec<String> = texts(db, user_id, a, name).into_iter().take(limit).map(|s| s.to_lowercase()).collect();
        texts(db, user_id, b, name).iter().filter(|x| av.contains(&x.to_lowercase())).count()
    };
    let genres = shared("genre", 20);
    let collections = shared("collection", 20);
    if genres == 0 && collections == 0 {
        return 0.0;
    }
    let mut s = genres as f64 + 3.0 * collections as f64 + 2.0 * shared("director", 20) as f64 + shared("actor", 10) as f64;
    if let (Some(x), Some(y)) = (a.year, b.year) {
        if x.abs_diff(y) <= 5 {
            s += 0.5;
        }
    }
    s
}

/// A row under a movie or show: (title, hub identifier, item ids).
pub struct Hub {
    pub title: String,
    pub identifier: String,
    pub context: String,
    pub ids: Vec<u64>,
    pub more: bool,
}

pub fn related(db: &Db, user: &User, it: &Item, count: usize) -> Vec<Hub> {
    let (prefix, plural) = match it.kind {
        ItemKind::Movie => ("movie", "Movies"),
        ItemKind::Show => ("tv", "Shows"),
        _ => return vec![],
    };
    // Every row looks only in the same library and never shows the item itself ("&id!=").
    let pool: Vec<&Item> = db.items.values().filter(|o| o.section_id == it.section_id && o.kind == it.kind && o.id != it.id).collect();
    let newest_first = |v: &mut Vec<&Item>| v.sort_by(|a, b| b.year.cmp(&a.year).then(a.sort_title.cmp(&b.sort_title)));
    let mut hubs = vec![];
    let mut push = |title: String, identifier: String, context: &str, mut list: Vec<&Item>| {
        if list.is_empty() {
            return;
        }
        let more = list.len() > count;
        list.truncate(count);
        hubs.push(Hub { title, identifier, context: context.into(), ids: list.iter().map(|i| i.id).collect(), more });
    };
    // Other editions: the same film more than once (movies only).
    if it.kind == ItemKind::Movie {
        let mut eds: Vec<&Item> = pool.iter().copied().filter(|o| o.sort_title.eq_ignore_ascii_case(&it.sort_title) && o.year == it.year).collect();
        newest_first(&mut eds);
        push(format!("Other Editions of {}", it.title), "movie.editions".into(), "hub.movie.editions", eds);
    }
    // Related: the most alike first.
    let mut scored: Vec<(f64, &Item)> = pool.iter().map(|o| (similarity(db, user.id, it, o), *o)).filter(|(s, _)| *s >= SIMILAR_MIN_SCORE).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.rating.partial_cmp(&a.1.rating).unwrap_or(std::cmp::Ordering::Equal)));
    push(format!("Related {plural}"), format!("{prefix}.similar"), &format!("hub.{prefix}.similar"), scored.into_iter().map(|x| x.1).collect());
    // More by the first director (movies only).
    if it.kind == ItemKind::Movie {
        if let Some(d) = texts(db, user.id, it, "director").into_iter().next() {
            let mut list: Vec<&Item> = pool.iter().copied().filter(|o| texts(db, user.id, o, "director").iter().any(|x| x.eq_ignore_ascii_case(&d))).collect();
            newest_first(&mut list);
            push(format!("More by {d}"), "movie.same.director".into(), "hub.movie.same.director", list);
        }
    }
    // More with each of the first two actors.
    for (n, a) in texts(db, user.id, it, "actor").into_iter().take(ACTOR_ROWS).enumerate() {
        let mut list: Vec<&Item> = pool.iter().copied().filter(|o| texts(db, user.id, o, "actor").iter().any(|x| x.eq_ignore_ascii_case(&a))).collect();
        newest_first(&mut list);
        push(format!("More with {a}"), format!("{prefix}.same.actor.{n}"), &format!("hub.{prefix}.same.actor"), list);
    }
    hubs
}

// ---------------------------------------------------------------- occasions

/// A time of year with films of its own: a home row while it lasts.
pub struct Occasion {
    pub key: &'static str,
    /// Row title (English; the web app translates it).
    pub title: &'static str,
    /// First and last day, (month, day); a span may run over New Year.
    pub from: (u32, u32),
    pub to: (u32, u32),
    /// Parts of genre names that fit (lower case; German and English names).
    pub genres: &'static [&'static str],
    /// Words in a title, tagline or plot that fit.
    pub words: &'static [&'static str],
}

pub const OCCASIONS: &[Occasion] = &[
    Occasion {
        key: "halloween",
        title: "Halloween",
        from: (10, 1),
        to: (11, 2),
        genres: &["horror", "halloween", "grusel"],
        words: &["halloween"],
    },
    Occasion {
        key: "christmas",
        title: "Christmas",
        from: (12, 1),
        to: (12, 26),
        genres: &["christmas", "weihnacht"],
        words: &["christmas", "xmas", "weihnacht", "heiligabend", "santa claus", "weihnachtsmann", "nikolaus", "grinch"],
    },
    Occasion {
        key: "newyear",
        title: "New Year's Eve",
        from: (12, 27),
        to: (1, 1),
        genres: &[],
        words: &["new year's eve", "new year\u{2019}s eve", "silvester"],
    },
    Occasion {
        key: "valentine",
        title: "Valentine's Day",
        from: (2, 7),
        to: (2, 14),
        genres: &["romance", "romant", "liebes"],
        words: &["valentine", "valentin"],
    },
];

impl Occasion {
    pub fn on(&self, day: (u32, u32)) -> bool {
        if self.from <= self.to { self.from <= day && day <= self.to } else { day >= self.from || day <= self.to }
    }

    /// How well an item fits: 3 a word in its title, 2 a genre, 1 a word in
    /// its tagline or plot, 0 not at all.
    fn fit(&self, it: &Item) -> u8 {
        let has = |text: &str| {
            let t = text.to_lowercase();
            self.words.iter().any(|w| t.contains(w))
        };
        if has(&it.title) {
            3
        } else if it.genres.iter().any(|g| {
            let g = g.to_lowercase();
            self.genres.iter().any(|x| g.contains(x))
        }) {
            2
        } else if has(&it.tagline) || has(&it.summary) {
            1
        } else {
            0
        }
    }
}

/// The occasion rows for a day: movies and shows that fit, best fit first and
/// then the best rated; episodes only when their own title fits (a show's
/// Halloween episode).
pub fn occasion_rows(db: &Db, user: &User, day: (u32, u32), count: usize) -> Vec<Hub> {
    let mut hubs = vec![];
    for o in OCCASIONS.iter().filter(|o| o.on(day)) {
        let mut list: Vec<(u8, &Item)> = db
            .items
            .values()
            .filter(|i| db.can_see_item(user, i))
            .filter_map(|i| {
                let fit = o.fit(i);
                let wanted = match i.kind {
                    ItemKind::Movie | ItemKind::Show => fit > 0,
                    ItemKind::Episode => fit == 3,
                    _ => false,
                };
                wanted.then_some((fit, i))
            })
            .collect();
        list.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(b.1.rating.partial_cmp(&a.1.rating).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.1.sort_title.cmp(&b.1.sort_title))
        });
        if list.is_empty() {
            continue;
        }
        let more = list.len() > count;
        list.truncate(count);
        hubs.push(Hub {
            title: o.title.into(),
            identifier: format!("occasion.{}", o.key),
            context: "hub.home.occasion".into(),
            ids: list.iter().map(|x| x.1.id).collect(),
            more,
        });
    }
    hubs
}

/// "MM-DD" or "YYYY-MM-DD" to (month, day).
pub fn month_day(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.trim().rsplitn(3, '-');
    let d: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((m, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Section, User};

    #[test]
    fn related_rows_cover_every_kind() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::db::Settings::default(),
            "sections": [], "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        db.sections.push(Section { id: 1, name: "M".into(), kind: SectionKind::Movie, paths: vec![], created_at: 0, scanned_at: 0, credits_detection: true, intro_detection: true, language: String::new() });
        let mk = |id: u64, title: &str, year: u32, genres: &[&str], extra: Value| -> Item {
            serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": title, "sort_title": title.to_lowercase(), "year": year,
                "genres": genres, "added_at": id, "updated_at": 0, "extra": extra})).unwrap()
        };
        let cast = |a: &[&str]| a.iter().map(|n| json!({"name": n})).collect::<Vec<_>>();
        for it in [
            mk(1, "Space One", 2001, &["Sci-Fi", "Drama"], json!({"directors": ["Dee"], "cast": cast(&["Ann", "Bob", "Cy"]), "collections": ["Space Saga"]})),
            mk(2, "Space Two", 2004, &["Sci-Fi"], json!({"directors": ["Dee"], "cast": cast(&["Ann"]), "collections": ["Space Saga"]})),
            mk(3, "Garden", 1990, &["Comedy"], json!({"cast": cast(&["Bob"])})),
            mk(4, "Space One", 2001, &["Sci-Fi"], json!({})),
            mk(5, "Other Film", 2010, &["Drama"], json!({"cast": cast(&["Cy"])})),
        ] {
            db.items.insert(it.id, it);
        }
        let user: User = serde_json::from_value(json!({"id": 1, "username": "o", "admin": true, "password_hash": "", "salt": "", "created_at": 0})).unwrap();
        let it = db.items[&1].clone();
        let hubs = related(&db, &user, &it, HUB_COUNT);
        let by = |id: &str| hubs.iter().find(|h| h.identifier == id).map(|h| h.ids.clone()).unwrap_or_default();
        assert_eq!(by("movie.editions"), vec![4]);
        assert_eq!(by("movie.similar").first(), Some(&2));
        assert!(!by("movie.similar").contains(&1));
        assert!(!by("movie.similar").contains(&3));
        assert_eq!(by("movie.same.director"), vec![2]);
        assert_eq!(by("movie.same.actor.0"), vec![2]);
        assert_eq!(by("movie.same.actor.1"), vec![3]);
        assert!(by("movie.same.actor.2").is_empty());
        assert_eq!(hubs.iter().find(|h| h.identifier == "movie.same.actor.0").unwrap().title, "More with Ann");
        assert_eq!(hubs.iter().find(|h| h.identifier == "movie.same.director").unwrap().title, "More by Dee");
        let cols = collections(&db, &user, 1);
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0]["childCount"], 2);
        let cats = categories(&db, &user, 1);
        assert!(occasion_rows(&db, &user, (10, 31), 20).is_empty(), "no horror, no Halloween row");
        assert_eq!(cats.iter().map(|c| c["title"].as_str().unwrap().to_string()).collect::<Vec<_>>(), vec!["Comedy", "Drama", "Sci-Fi"]);
    }

    #[test]
    fn occasion_rows_by_date() {
        assert_eq!(month_day("2026-10-31"), Some((10, 31)));
        assert_eq!(month_day("12-24"), Some((12, 24)));
        assert_eq!(month_day("13-01"), None);
        let ny = OCCASIONS.iter().find(|o| o.key == "newyear").unwrap();
        assert!(ny.on((12, 31)) && ny.on((1, 1)) && !ny.on((1, 2)) && !ny.on((12, 26)));

        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::db::Settings::default(),
            "sections": [], "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let mk = |id: u64, kind: &str, title: &str, genres: &[&str], rating: f32, summary: &str| -> Item {
            serde_json::from_value(json!({"id": id, "section_id": 1, "kind": kind, "title": title, "sort_title": title.to_lowercase(),
                "genres": genres, "rating": rating, "summary": summary, "added_at": 0, "updated_at": 0})).unwrap()
        };
        for it in [
            mk(1, "movie", "Scream", &["Horrorfilm"], 7.0, ""),
            mk(2, "movie", "Halloween", &["Horror"], 6.0, ""),
            mk(3, "movie", "The Shining", &["Horror"], 8.4, ""),
            mk(4, "movie", "Garden", &["Comedy"], 9.0, ""),
            mk(5, "episode", "Halloween Special", &[], 0.0, ""),
            mk(6, "episode", "Pilot", &[], 0.0, "On Halloween night"),
            mk(7, "movie", "Die Hard", &["Actionfilm"], 8.2, "On Christmas Eve, a cop"),
            mk(8, "movie", "Weihnachtsfilm", &["Weihnachtsfilm"], 5.0, ""),
        ] {
            db.items.insert(it.id, it);
        }
        let user: User = serde_json::from_value(json!({"id": 1, "username": "o", "admin": true, "password_hash": "", "salt": "", "created_at": 0})).unwrap();
        let rows = occasion_rows(&db, &user, (10, 31), 20);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Halloween");
        // Title first (film, then the episode), then horror by rating; the plain episode not.
        assert_eq!(rows[0].ids, vec![2, 5, 3, 1]);
        let rows = occasion_rows(&db, &user, (12, 24), 20);
        assert_eq!(rows[0].ids, vec![8, 7]);
        assert!(occasion_rows(&db, &user, (6, 1), 20).is_empty());
    }
}
