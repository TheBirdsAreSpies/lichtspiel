//! A library's Collections and Categories pages, and the rows under a movie
//! or show ("Related Movies", "More by …", "More with …").

use crate::data::db::{Db, Item, ItemKind, SectionKind, User};
use crate::library::query::{field, FieldValue};
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
        SectionKind::Music => ItemKind::Artist,
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
        "key": format!("/library/sections/{sid}/all?{filter}={}", crate::library::metadata::enc(title)),
        "filter": filter,
        "childCount": items.len(),
        "thumb": pic.map(|i| format!("/library/metadata/{}/thumb?v={}", i.id, i.updated_at)),
        "art": pic.map(|i| format!("/library/metadata/{}/art?v={}", i.id, i.updated_at)),
        "ratingKeys": items.iter().take(4).map(|i| i.id).collect::<Vec<_>>(),
    })
}

/// The library's collections (film series and the ones set by hand), each with how many of its
/// titles this user has seen (`viewedLeafCount`), for "Hide watched" on the Collections page.
pub fn collections(db: &Db, user: &User, sid: u64) -> Vec<Value> {
    let min = db.settings.collection_min_items.max(1) as usize;
    let seen = |it: &Item| !matches!(field(db, user.id, it, "unwatched"), FieldValue::Bool(true));
    groups(db, user, sid, "collection").iter().filter(|(_, its)| its.len() >= min).map(|(t, its)| {
        let mut d = directory(sid, t, "collection", its);
        d["viewedLeafCount"] = json!(its.iter().filter(|it| seen(it)).count());
        d
    }).collect()
}

/// The library's categories: one per genre, with a picture.
pub fn categories(db: &Db, user: &User, sid: u64) -> Vec<Value> {
    groups(db, user, sid, "genre").iter().map(|(t, its)| directory(sid, t, "genre", its)).collect()
}

/// The names of one title that the rows compare, lower case. Made once per title, not once per comparison:
/// in a big library "Related" would otherwise take seconds.
struct Tags {
    genre: Vec<String>,
    collection: Vec<String>,
    director: Vec<String>,
    actor: Vec<String>,
}

fn tags(db: &Db, user_id: u64, it: &Item) -> Tags {
    let low = |name: &str| texts(db, user_id, it, name).into_iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
    Tags { genre: low("genre"), collection: low("collection"), director: low("director"), actor: low("actor") }
}

/// How much `b` is like `a` (estimated weights).
fn similarity(a: &Tags, ay: Option<u32>, b: &Tags, by: Option<u32>) -> f64 {
    let shared = |av: &[String], bv: &[String], limit: usize| -> usize { bv.iter().filter(|x| av.iter().take(limit).any(|y| y == *x)).count() };
    let genres = shared(&a.genre, &b.genre, 20);
    let collections = shared(&a.collection, &b.collection, 20);
    if genres == 0 && collections == 0 {
        return 0.0;
    }
    let mut s = genres as f64 + 3.0 * collections as f64 + 2.0 * shared(&a.director, &b.director, 20) as f64 + shared(&a.actor, &b.actor, 10) as f64;
    if let (Some(x), Some(y)) = (ay, by) {
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
    let mine = tags(db, user.id, it);
    let pool_tags: Vec<Tags> = pool.iter().map(|o| tags(db, user.id, o)).collect();
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
    // The films of the same series, in the order they came out (movies only); the player offers the next one.
    if it.kind == ItemKind::Movie {
        if let Some(c) = texts(db, user.id, it, "collection").into_iter().next() {
            let c_low = c.to_lowercase();
            let mut list: Vec<&Item> = pool.iter().zip(&pool_tags).filter(|(_, t)| t.collection.contains(&c_low)).map(|(o, _)| *o).collect();
            list.sort_by(|a, b| a.year.unwrap_or(u32::MAX).cmp(&b.year.unwrap_or(u32::MAX)).then(a.sort_title.cmp(&b.sort_title)));
            push(c, "movie.collection".into(), "hub.movie.collection", list);
        }
    }
    // Other editions: the same film more than once (movies only).
    if it.kind == ItemKind::Movie {
        let mut eds: Vec<&Item> = pool.iter().copied().filter(|o| o.sort_title.eq_ignore_ascii_case(&it.sort_title) && o.year == it.year).collect();
        newest_first(&mut eds);
        push(format!("Other Editions of {}", it.title), "movie.editions".into(), "hub.movie.editions", eds);
    }
    // Related: the most alike first.
    let mut scored: Vec<(f64, &Item)> = pool.iter().zip(&pool_tags).map(|(o, t)| (similarity(&mine, it.year, t, o.year), *o)).filter(|(s, _)| *s >= SIMILAR_MIN_SCORE).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(b.1.rating.partial_cmp(&a.1.rating).unwrap_or(std::cmp::Ordering::Equal)));
    push(format!("Related {plural}"), format!("{prefix}.similar"), &format!("hub.{prefix}.similar"), scored.into_iter().map(|x| x.1).collect());
    // More by the first director (movies only).
    if it.kind == ItemKind::Movie {
        if let Some(d) = texts(db, user.id, it, "director").into_iter().next() {
            let d_low = d.to_lowercase();
            let mut list: Vec<&Item> = pool.iter().zip(&pool_tags).filter(|(_, t)| t.director.contains(&d_low)).map(|(o, _)| *o).collect();
            newest_first(&mut list);
            push(format!("More by {d}"), "movie.same.director".into(), "hub.movie.same.director", list);
        }
    }
    // More with each of the first two actors.
    for (n, a) in texts(db, user.id, it, "actor").into_iter().take(ACTOR_ROWS).enumerate() {
        let a_low = a.to_lowercase();
        let mut list: Vec<&Item> = pool.iter().zip(&pool_tags).filter(|(_, t)| t.actor.contains(&a_low)).map(|(o, _)| *o).collect();
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

// ---------------------------------------------------------------- a library's Recommended page

/// Items per row on a library's Recommended page (like the home rows).
pub const SECTION_ROW_COUNT: usize = 20;
/// A genre, person or network gets a row of its own only with at least this many titles (estimated).
pub const PICK_MIN_TITLES: usize = 3;
/// The row's genre, person or network is one of the this many biggest, a different one each day (estimated).
pub const PICK_FROM: usize = 5;
/// "Rediscover": shows last watched more than this many days ago (estimated).
pub const REDISCOVER_DAYS: u64 = 365;

/// The release date as "YYYY-MM-DD" (or the year alone), for sorting by it.
fn released(it: &Item) -> String {
    match it.extra.get("originally_available").and_then(|v| v.as_str()).filter(|s| s.len() >= 4) {
        Some(d) => d.to_string(),
        None => it.year.map(|y| format!("{y:04}")).unwrap_or_default(),
    }
}

fn by_rating(a: &&Item, b: &&Item) -> std::cmp::Ordering {
    b.rating.partial_cmp(&a.rating).unwrap_or(std::cmp::Ordering::Equal).then(a.sort_title.cmp(&b.sort_title))
}

/// The name for today's row: one of the biggest names (at least [`PICK_MIN_TITLES`] titles), a different
/// one each day. Returns the name as written and its items.
fn pick<'a>(names: &[(String, Vec<&'a Item>)], seed: u64) -> Option<(String, Vec<&'a Item>)> {
    let mut big: Vec<&(String, Vec<&Item>)> = names.iter().filter(|(_, v)| v.len() >= PICK_MIN_TITLES).collect();
    big.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    big.truncate(PICK_FROM);
    if big.is_empty() {
        return None;
    }
    let (n, v) = big[(seed % big.len() as u64) as usize];
    Some((n.clone(), v.clone()))
}

/// Groups `items` by the names `of` gives (case does not matter; the first spelling is kept).
fn group_by<'a>(items: &[&'a Item], of: impl Fn(&Item) -> Vec<String>) -> Vec<(String, Vec<&'a Item>)> {
    let mut map: BTreeMap<String, (String, Vec<&Item>)> = BTreeMap::new();
    for it in items {
        for n in of(it) {
            let e = map.entry(n.to_lowercase()).or_insert_with(|| (n.clone(), vec![]));
            if !e.1.iter().any(|x| x.id == it.id) {
                e.1.push(it);
            }
        }
    }
    map.into_values().collect()
}

/// The show an episode or season belongs to.
fn show_of<'a>(db: &'a Db, it: &'a Item) -> Option<&'a Item> {
    let mut cur = it;
    for _ in 0..3 {
        if cur.kind == ItemKind::Show {
            return Some(cur);
        }
        cur = db.items.get(&cur.parent_id?)?;
    }
    None
}

/// Adds a row unless it is empty; at most `count` titles, `more` when there are further ones.
fn add(hubs: &mut Vec<Hub>, count: usize, title: String, identifier: &str, list: Vec<&Item>) {
    if list.is_empty() {
        return;
    }
    let more = list.len() > count;
    hubs.push(Hub { title, identifier: identifier.into(), context: format!("hub.{identifier}"), ids: list.iter().take(count).map(|i| i.id).collect(), more });
}

/// The rows of a library's Recommended page. `day` picks the time-of-year rows, `seed` (the day number)
/// which genre, person or network gets a row today.
pub fn section_rows(db: &Db, user: &User, sid: u64, day: (u32, u32), seed: u64, count: usize) -> Vec<Hub> {
    let Some(sec) = db.sections.iter().find(|s| s.id == sid) else { return vec![] };
    let top = top_kind(sec.kind);
    let mine = |i: &&Item| i.section_id == sid && !i.hidden() && db.can_see_item(user, i);
    let mut pool: Vec<&Item> = db.items.values().filter(|i| i.kind == top).filter(mine).collect();
    pool.sort_by(|a, b| a.sort_title.cmp(&b.sort_title));
    let today = crate::data::db::today();
    let mut hubs: Vec<Hub> = vec![];
    let prefix = match sec.kind {
        SectionKind::Movie => "movie",
        SectionKind::Show => "tv",
        SectionKind::Music => "music",
        SectionKind::Other => "video",
    };

    // Continue Watching, only from this library.
    let cw: Vec<&Item> = crate::server::continue_watching(db, user).iter().filter_map(|i| db.items.get(i)).filter(|i| i.section_id == sid).collect();
    add(&mut hubs, count, "Continue Watching".into(), &format!("{prefix}.inprogress"), cw);

    // What this person watched here last (episodes for TV), newest first.
    let recently_viewed = || -> Vec<&Item> {
        let mut seen: Vec<(u64, &Item)> = db
            .views
            .range(format!("{}:", user.id)..format!("{};", user.id))
            .filter(|(_, v)| v.view_count > 0 && v.last_viewed_at > 0)
            .filter_map(|(k, v)| Some((v.last_viewed_at, db.items.get(&k.split_once(':')?.1.parse::<u64>().ok()?)?)))
            .filter(|(_, i)| i.kind.playable() && mine(i))
            .collect();
        seen.sort_by(|a, b| b.0.cmp(&a.0));
        seen.into_iter().map(|x| x.1).collect()
    };
    let recently_added = |kind: ItemKind| -> Vec<&Item> {
        let mut v: Vec<&Item> = db.items.values().filter(|i| i.kind == kind).filter(mine).collect();
        v.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(b.id.cmp(&a.id)));
        v
    };
    // Out already (not announced for later), newest first.
    let recently_released = |kind: ItemKind| -> Vec<&Item> {
        let mut v: Vec<(String, &Item)> = db.items.values().filter(|i| i.kind == kind).filter(mine).map(|i| (released(i), i)).filter(|(d, _)| !d.is_empty() && d.as_str() <= today.as_str()).collect();
        v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.sort_title.cmp(&b.1.sort_title)));
        v.into_iter().map(|x| x.1).collect()
    };
    let genres = group_by(&pool, |i| i.genres.iter().map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect());
    let seasonal = || -> Vec<Hub> {
        occasion_rows(db, user, day, usize::MAX)
            .into_iter()
            .filter_map(|mut o| {
                o.ids.retain(|i| db.items.get(i).is_some_and(|i| i.section_id == sid && (i.kind == top || i.kind == ItemKind::Episode)));
                let more = o.ids.len() > count;
                o.ids.truncate(count);
                (!o.ids.is_empty()).then_some(Hub { more, ..o })
            })
            .collect()
    };

    match sec.kind {
        SectionKind::Movie => {
            add(&mut hubs, count, "Recently Released Movies".into(), "movie.recentlyreleased", recently_released(ItemKind::Movie));
            add(&mut hubs, count, "Recently Added Movies".into(), "movie.recentlyadded", recently_added(ItemKind::Movie));
            if let Some((g, mut list)) = pick(&genres, seed) {
                list.sort_by(by_rating);
                add(&mut hubs, count, format!("Top Movies in {g}"), "movie.genre", list);
            }
            // Actors (the first 10 of each film) and directors together; a different one each day.
            let mut people = group_by(&pool, |i| texts(db, user.id, i, "actor").into_iter().take(10).collect());
            let directors = group_by(&pool, |i| texts(db, user.id, i, "director"));
            let director_names: Vec<String> = directors.iter().map(|d| d.0.to_lowercase()).collect();
            people.extend(directors);
            if let Some((p, mut list)) = pick(&people, seed / 2 + 1) {
                list.sort_by(by_rating);
                let title = if director_names.contains(&p.to_lowercase()) { format!("Top Movies by {p}") } else { format!("Top Movies with {p}") };
                add(&mut hubs, count, title, "movie.person", list);
            }
            let mut unwatched: Vec<&Item> = pool.iter().copied().filter(|i| db.view(user.id, i.id).view_count == 0 && i.rating.is_some()).collect();
            unwatched.sort_by(by_rating);
            add(&mut hubs, count, "Top Unwatched Picks".into(), "movie.unwatched", unwatched);
            for o in seasonal() {
                hubs.push(o);
            }
            add(&mut hubs, count, "Recently Watched Movies".into(), "movie.recentlyviewed", recently_viewed());
        }
        SectionKind::Show => {
            add(&mut hubs, count, "Recently Released Episodes".into(), "tv.recentlyreleased", recently_released(ItemKind::Episode));
            if db.settings.group_recent_episodes {
                let eps: Vec<u64> = recently_added(ItemKind::Episode).iter().map(|i| i.id).collect();
                add(&mut hubs, count, "Recently Added TV".into(), "tv.recentlyadded", crate::server::shows_of(db, &eps, count + 1).iter().filter_map(|i| db.items.get(i)).collect());
            } else {
                add(&mut hubs, count, "Recently Added Episodes".into(), "tv.recentlyadded", recently_added(ItemKind::Episode));
            }
            // When this person last watched an episode of each show.
            let mut last: std::collections::HashMap<u64, u64> = Default::default();
            for (k, v) in db.views.range(format!("{}:", user.id)..format!("{};", user.id)) {
                if v.view_count == 0 && v.view_offset_ms == 0 {
                    continue;
                }
                let Some(ep) = k.split_once(':').and_then(|x| x.1.parse::<u64>().ok()).and_then(|i| db.items.get(&i)) else { continue };
                if ep.section_id != sid {
                    continue;
                }
                if let Some(show) = show_of(db, ep) {
                    let e = last.entry(show.id).or_default();
                    *e = (*e).max(v.last_viewed_at);
                }
            }
            // Start Watching: shows not begun yet, the newest first.
            let mut fresh: Vec<&Item> = pool.iter().copied().filter(|s| !last.contains_key(&s.id)).collect();
            fresh.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(a.sort_title.cmp(&b.sort_title)));
            add(&mut hubs, count, "Start Watching".into(), "tv.startwatching", fresh);
            // Rediscover: shows watched long ago, the longest ago first.
            let cutoff = crate::data::db::now().saturating_sub(REDISCOVER_DAYS * 86400);
            let mut old: Vec<(u64, &Item)> = pool.iter().filter_map(|s| last.get(&s.id).filter(|t| **t > 0 && **t < cutoff).map(|t| (*t, *s))).collect();
            old.sort_by(|a, b| a.0.cmp(&b.0));
            add(&mut hubs, count, "Rediscover".into(), "tv.rediscover", old.into_iter().map(|x| x.1).collect());
            let networks = group_by(&pool, |i| vec![i.studio.trim().to_string()].into_iter().filter(|s| !s.is_empty()).collect());
            if let Some((n, mut list)) = pick(&networks, seed / 2 + 1) {
                list.sort_by(by_rating);
                add(&mut hubs, count, format!("More from {n}"), "tv.network", list);
            }
            if let Some((g, mut list)) = pick(&genres, seed) {
                list.sort_by(by_rating);
                add(&mut hubs, count, format!("More in {g}"), "tv.genre", list);
            }
            let mut rated: Vec<&Item> = pool.iter().copied().filter(|i| i.rating.is_some_and(|r| r > 0.0)).collect();
            rated.sort_by(by_rating);
            add(&mut hubs, count, "Top Rated Shows".into(), "tv.toprated", rated);
            for o in seasonal() {
                hubs.push(o);
            }
            add(&mut hubs, count, "Recently Watched Episodes".into(), "tv.recentlyviewed", recently_viewed());
        }
        SectionKind::Music => {
            add(&mut hubs, count, "Recently Added Music".into(), "music.recentlyadded", recently_added(ItemKind::Album));
            add(&mut hubs, count, "Recently Played".into(), "music.recentlyviewed", recently_viewed());
        }
        SectionKind::Other => {
            add(&mut hubs, count, "Recently Added".into(), "video.recentlyadded", recently_added(ItemKind::Video));
            add(&mut hubs, count, "Recently Watched".into(), "video.recentlyviewed", recently_viewed());
        }
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
    use crate::data::db::{Section, User};

    #[test]
    fn related_rows_cover_every_kind() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::data::db::Settings::default(),
            "sections": [], "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        db.sections.push(Section { id: 1, name: "M".into(), kind: SectionKind::Movie, paths: vec![], created_at: 0, scanned_at: 0, credits_detection: true, intro_detection: true, language: String::new(), scan_interval_minutes: None });
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
        assert_eq!(cols[0]["viewedLeafCount"], 0);
        db.views.insert("1:2".into(), crate::data::db::ViewState { view_count: 1, ..Default::default() });
        assert_eq!(collections(&db, &user, 1)[0]["viewedLeafCount"], 1, "seen by this user");
        let cats = categories(&db, &user, 1);
        assert!(occasion_rows(&db, &user, (10, 31), 20).is_empty(), "no horror, no Halloween row");
        assert_eq!(cats.iter().map(|c| c["title"].as_str().unwrap().to_string()).collect::<Vec<_>>(), vec!["Comedy", "Drama", "Sci-Fi"]);
    }

    #[test]
    fn recommended_rows_of_a_library() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 99, "settings": crate::data::db::Settings::default(),
            "sections": [], "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        for (id, kind) in [(1, SectionKind::Movie), (2, SectionKind::Show)] {
            db.sections.push(Section { id, name: "L".into(), kind, paths: vec![], created_at: 0, scanned_at: 0, credits_detection: true, intro_detection: true, language: String::new(), scan_interval_minutes: None });
        }
        let mk = |id: u64, sid: u64, kind: &str, parent: Option<u64>, genres: &[&str], rating: f32, extra: Value| -> Item {
            serde_json::from_value(json!({"id": id, "section_id": sid, "kind": kind, "parent_id": parent, "title": format!("T{id}"), "sort_title": format!("t{id:03}"),
                "genres": genres, "rating": rating, "added_at": id, "updated_at": 0, "extra": extra})).unwrap()
        };
        let cast = |a: &[&str]| a.iter().map(|n| json!({"name": n})).collect::<Vec<_>>();
        for it in [
            mk(1, 1, "movie", None, &["Drama"], 6.0, json!({"originally_available": "2020-05-01", "cast": cast(&["Ann"])})),
            mk(2, 1, "movie", None, &["Drama"], 8.0, json!({"originally_available": "2024-01-01", "cast": cast(&["Ann"])})),
            mk(3, 1, "movie", None, &["Drama", "Horror"], 7.0, json!({"originally_available": "1999-01-01", "cast": cast(&["Ann", "Bob"])})),
            mk(4, 1, "movie", None, &["Comedy"], 9.0, json!({"originally_available": "2999-01-01"})),
            // A show with one season and two episodes, and a second show nobody started.
            mk(10, 2, "show", None, &["Crime"], 8.5, json!({})),
            mk(11, 2, "season", Some(10), &[], 0.0, json!({})),
            mk(12, 2, "episode", Some(11), &[], 0.0, json!({"originally_available": "2023-03-01"})),
            mk(13, 2, "episode", Some(11), &[], 0.0, json!({"originally_available": "2023-03-08"})),
            mk(20, 2, "show", None, &["Crime"], 7.5, json!({})),
        ] {
            db.items.insert(it.id, it);
        }
        let user: User = serde_json::from_value(json!({"id": 1, "username": "o", "admin": true, "password_hash": "", "salt": "", "created_at": 0})).unwrap();
        db.views.insert("1:2".into(), crate::data::db::ViewState { view_count: 1, last_viewed_at: crate::data::db::now(), ..Default::default() });
        db.views.insert("1:12".into(), crate::data::db::ViewState { view_count: 1, last_viewed_at: 1000, ..Default::default() });

        let rows = section_rows(&db, &user, 1, (6, 1), 0, 20);
        let by = |rows: &[Hub], id: &str| rows.iter().find(|h| h.identifier == id).map(|h| h.ids.clone()).unwrap_or_default();
        // Not yet out (2999) is left out; newest release first.
        assert_eq!(by(&rows, "movie.recentlyreleased"), vec![2, 1, 3]);
        assert_eq!(by(&rows, "movie.recentlyadded"), vec![4, 3, 2, 1]);
        // Drama has 3 films (Comedy and Horror too few): best rated first.
        let g = rows.iter().find(|h| h.identifier == "movie.genre").unwrap();
        assert_eq!((g.title.as_str(), g.ids.clone()), ("Top Movies in Drama", vec![2, 3, 1]));
        assert_eq!(rows.iter().find(|h| h.identifier == "movie.person").unwrap().title, "Top Movies with Ann");
        assert_eq!(by(&rows, "movie.unwatched"), vec![4, 3, 1]);
        assert_eq!(by(&rows, "movie.recentlyviewed"), vec![2]);
        assert!(rows.iter().all(|h| h.ids.iter().all(|i| db.items[i].section_id == 1)), "only this library");

        let rows = section_rows(&db, &user, 2, (6, 1), 0, 20);
        assert_eq!(by(&rows, "tv.recentlyreleased"), vec![13, 12]);
        assert_eq!(by(&rows, "tv.startwatching"), vec![20]);
        assert_eq!(by(&rows, "tv.rediscover"), vec![10], "watched long ago");
        assert_eq!(by(&rows, "tv.toprated"), vec![10, 20]);
        assert_eq!(by(&rows, "tv.recentlyviewed"), vec![12]);
        assert!(by(&rows, "tv.genre").is_empty(), "two shows are too few for a genre row");
        // Only one row per kind, at most `count` titles, `more` when cut.
        let short = section_rows(&db, &user, 1, (6, 1), 0, 2);
        let added = short.iter().find(|h| h.identifier == "movie.recentlyadded").unwrap();
        assert!(added.more && added.ids == vec![4, 3]);
    }

    #[test]
    fn occasion_rows_by_date() {
        assert_eq!(month_day("2026-10-31"), Some((10, 31)));
        assert_eq!(month_day("12-24"), Some((12, 24)));
        assert_eq!(month_day("13-01"), None);
        let ny = OCCASIONS.iter().find(|o| o.key == "newyear").unwrap();
        assert!(ny.on((12, 31)) && ny.on((1, 1)) && !ny.on((1, 2)) && !ny.on((12, 26)));

        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::data::db::Settings::default(),
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
