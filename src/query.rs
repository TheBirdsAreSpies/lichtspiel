//! Filtering and sorting, shared by library browsing, smart playlists,
//! playlist contents and the playlist list itself.

use crate::db::{now, Db, Item, ItemKind, Playlist, Rule, SmartQuery, User};
use serde_json::Value;
use std::cmp::Ordering;

/// Every field a rule or a sort can use, with a short description (also
/// returned by GET /api/filters so clients can build rule editors).
pub const ITEM_FIELDS: &[(&str, &str, &str)] = &[
    ("title", "text", "Title"),
    ("kind", "text", "movie, show, season, episode or video"),
    ("year", "number", "Release year"),
    ("genre", "text", "Genre"),
    ("rating", "number", "Rating 0-10"),
    ("user_rating", "number", "Your own rating 0-10"),
    ("content_rating", "text", "Age rating such as PG-13 or FSK 12"),
    ("studio", "text", "Studio or network"),
    ("summary", "text", "Description"),
    ("show", "text", "Show title (episodes)"),
    ("season", "number", "Season number (episodes)"),
    ("episode", "number", "Episode number"),
    ("section", "number", "Library id"),
    ("added_at", "date", "Date added (use in_last_days)"),
    ("last_viewed_at", "date", "Date you last watched it"),
    ("duration", "number", "Length in minutes"),
    ("watched", "bool", "Seen at least once"),
    ("in_progress", "bool", "Started but not finished"),
    ("view_count", "number", "How often you watched it"),
    ("favorite", "bool", "Marked as favorite"),
    ("container", "text", "File type: mkv, mp4, avi …"),
    ("video_codec", "text", "h264, hevc, av1 …"),
    ("audio_codec", "text", "aac, ac3, dts …"),
    ("resolution", "number", "Picture height in pixels (2160, 1080, 720 …)"),
    ("audio_channels", "number", "2 = stereo, 6 = 5.1"),
    ("audio_language", "text", "Audio language code (eng, ger …)"),
    ("subtitle_language", "text", "Subtitle language code"),
    ("size", "number", "File size in MB"),
    ("random", "sort", "Random order (sort only)"),
];

pub const OPERATORS: &[(&str, &str)] = &[
    ("is", "equals"),
    ("is_not", "does not equal"),
    ("contains", "contains the text"),
    ("not_contains", "does not contain the text"),
    ("starts_with", "begins with"),
    ("ends_with", "ends with"),
    ("gt", "greater than"),
    ("gte", "at least"),
    ("lt", "less than"),
    ("lte", "at most"),
    ("between", "between two values [low, high]"),
    ("in_last_days", "within the last N days"),
    ("not_in_last_days", "not within the last N days"),
    ("in", "one of a list"),
];

/// A field's value for one item and one user.
#[derive(Debug, Clone)]
pub enum FieldValue {
    Text(Vec<String>),
    Num(Option<f64>),
    Bool(bool),
}

pub fn field(db: &Db, user_id: u64, item: &Item, name: &str) -> FieldValue {
    use FieldValue::*;
    let view = db.view(user_id, item.id);
    let file = item.file.as_ref();
    let show = || -> Option<&Item> {
        let season = db.items.get(&item.parent_id?)?;
        match season.kind {
            ItemKind::Season => db.items.get(&season.parent_id?),
            ItemKind::Show => Some(season),
            _ => None,
        }
    };
    let watched_container = |db: &Db| -> (bool, bool) {
        // A show/season counts as watched when every episode is.
        let leaves = db.leaves(item.id);
        if leaves.is_empty() {
            return (false, false);
        }
        let seen = leaves.iter().filter(|l| db.view(user_id, **l).view_count > 0).count();
        (seen == leaves.len(), seen > 0 && seen < leaves.len())
    };
    match name {
        "title" => Text(vec![item.title.clone()]),
        "kind" | "type" => Text(vec![item.kind.as_str().into()]),
        "year" => Num(item.year.map(|y| y as f64)),
        "genre" => Text(item.genres.clone()),
        "rating" => Num(item.rating.map(|r| r as f64)),
        "user_rating" => Num(view.rating.map(|r| r as f64)),
        "content_rating" => Text(vec![item.content_rating.clone()]),
        "studio" => Text(vec![item.studio.clone()]),
        "summary" => Text(vec![item.summary.clone()]),
        "show" => Text(vec![show().map(|s| s.title.clone()).unwrap_or_default()]),
        "season" => Num(match item.kind {
            ItemKind::Season => item.index.map(|v| v as f64),
            ItemKind::Episode => item.parent_id.and_then(|p| db.items.get(&p)).and_then(|s| s.index).map(|v| v as f64),
            _ => None,
        }),
        "episode" => Num(if item.kind == ItemKind::Episode { item.index.map(|v| v as f64) } else { None }),
        "section" => Num(Some(item.section_id as f64)),
        "added_at" => Num(Some(item.added_at as f64)),
        "last_viewed_at" => Num(if view.last_viewed_at > 0 { Some(view.last_viewed_at as f64) } else { None }),
        "duration" => Num(Some(item.duration_ms as f64 / 60000.0)),
        "watched" => Bool(if item.kind.playable() { view.view_count > 0 } else { watched_container(db).0 }),
        "in_progress" => Bool(if item.kind.playable() {
            view.view_offset_ms > 0
        } else {
            false
        }),
        "view_count" => Num(Some(view.view_count as f64)),
        "favorite" => Bool(view.favorite),
        "container" => Text(vec![file.map(|f| f.container.clone()).unwrap_or_default()]),
        "video_codec" => Text(vec![file.and_then(|f| f.video()).map(|v| v.codec.clone()).unwrap_or_default()]),
        "audio_codec" => Text(file.map(|f| f.streams.iter().filter(|s| s.kind == "audio").map(|s| s.codec.clone()).collect()).unwrap_or_default()),
        "resolution" => Num(file.and_then(|f| f.video()).map(|v| v.height as f64)),
        "audio_channels" => Num(file.and_then(|f| f.audio()).map(|a| a.channels as f64)),
        "audio_language" => Text(file.map(|f| f.streams.iter().filter(|s| s.kind == "audio").map(|s| s.language.clone()).collect()).unwrap_or_default()),
        "subtitle_language" => Text(file.map(|f| f.streams.iter().filter(|s| s.kind == "subtitle").map(|s| s.language.clone()).collect()).unwrap_or_default()),
        "size" => Num(file.map(|f| f.size as f64 / 1_048_576.0)),
        other => {
            // Plugin-provided fields live in `extra`.
            match item.extra.get(other) {
                Some(Value::Number(n)) => Num(n.as_f64()),
                Some(Value::Bool(b)) => Bool(*b),
                Some(Value::String(s)) => Text(vec![s.clone()]),
                Some(Value::Array(a)) => Text(a.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect()),
                _ => Text(vec![]),
            }
        }
    }
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(*b as u8 as f64),
        _ => None,
    }
}

fn as_bool(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => matches!(s.to_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        _ => false,
    }
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.to_lowercase(),
        Value::Null => String::new(),
        other => other.to_string().to_lowercase(),
    }
}

pub fn rule_matches(db: &Db, user_id: u64, item: &Item, rule: &Rule) -> bool {
    let fv = field(db, user_id, item, &rule.field);
    let op = rule.op.as_str();
    let days = |d: &Value| as_num(d).unwrap_or(0.0) * 86400.0;
    match fv {
        FieldValue::Bool(b) => match op {
            "is_not" => b != as_bool(&rule.value),
            _ => b == as_bool(&rule.value),
        },
        FieldValue::Num(n) => {
            if op == "in_last_days" {
                return n.is_some_and(|n| n >= now() as f64 - days(&rule.value));
            }
            if op == "not_in_last_days" {
                return n.is_none_or(|n| n < now() as f64 - days(&rule.value));
            }
            if op == "between" {
                let (lo, hi) = match &rule.value {
                    Value::Array(a) if a.len() == 2 => (as_num(&a[0]), as_num(&a[1])),
                    Value::String(s) if s.contains(',') || s.contains('-') => {
                        let mut p = s.split([',', '-']);
                        (p.next().and_then(|x| x.trim().parse().ok()), p.next().and_then(|x| x.trim().parse().ok()))
                    }
                    _ => (None, None),
                };
                return matches!((n, lo, hi), (Some(n), Some(lo), Some(hi)) if n >= lo && n <= hi);
            }
            if op == "in" {
                let list: Vec<f64> = match &rule.value {
                    Value::Array(a) => a.iter().filter_map(as_num).collect(),
                    Value::String(s) => s.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
                    v => as_num(v).into_iter().collect(),
                };
                return n.is_some_and(|n| list.iter().any(|x| (x - n).abs() < 1e-9));
            }
            let Some(target) = as_num(&rule.value) else {
                // "is empty" style rule: value null means "has no value".
                return match op {
                    "is_not" => n.is_some(),
                    _ => n.is_none(),
                };
            };
            let Some(n) = n else { return op == "is_not" };
            match op {
                "is" => (n - target).abs() < 1e-9,
                "is_not" => (n - target).abs() >= 1e-9,
                "gt" => n > target,
                "gte" => n >= target,
                "lt" => n < target,
                "lte" => n <= target,
                _ => false,
            }
        }
        FieldValue::Text(values) => {
            let values: Vec<String> = values.iter().map(|v| v.to_lowercase()).collect();
            let wanted: Vec<String> = match &rule.value {
                Value::Array(a) => a.iter().map(as_text).collect(),
                v if op == "in" => as_text(v).split(',').map(|s| s.trim().to_string()).collect(),
                v => vec![as_text(v)],
            };
            let any = |f: &dyn Fn(&str, &str) -> bool| values.iter().any(|v| wanted.iter().any(|w| f(v, w)));
            match op {
                "is" | "in" => any(&|v, w| v == w),
                "is_not" => !any(&|v, w| v == w),
                "contains" => any(&|v, w| v.contains(w)),
                "not_contains" => !any(&|v, w| v.contains(w)),
                "starts_with" => any(&|v, w| v.starts_with(w)),
                "ends_with" => any(&|v, w| v.ends_with(w)),
                "gt" => any(&|v, w| v > w),
                "lt" => any(&|v, w| v < w),
                _ => false,
            }
        }
    }
}

pub fn matches_all(db: &Db, user_id: u64, item: &Item, rules: &[Rule], match_all: bool) -> bool {
    if rules.is_empty() {
        return true;
    }
    if match_all {
        rules.iter().all(|r| rule_matches(db, user_id, item, r))
    } else {
        rules.iter().any(|r| rule_matches(db, user_id, item, r))
    }
}

fn cmp_field(a: &FieldValue, b: &FieldValue) -> Ordering {
    use FieldValue::*;
    match (a, b) {
        (Num(x), Num(y)) => match (x, y) {
            (Some(x), Some(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            _ => Ordering::Equal,
        },
        (Bool(x), Bool(y)) => x.cmp(y),
        (Text(x), Text(y)) => {
            let xs = x.first().map(|s| s.to_lowercase()).unwrap_or_default();
            let ys = y.first().map(|s| s.to_lowercase()).unwrap_or_default();
            xs.cmp(&ys)
        }
        _ => Ordering::Equal,
    }
}

/// Parses "added_at:desc,title" into [("added_at", desc=true), ("title", false)].
pub fn parse_sort(sort: &str) -> Vec<(String, bool)> {
    sort.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let (f, dir) = s.split_once(':').unwrap_or((s, "asc"));
            let f = f.trim();
            let (f, desc) = match f.strip_prefix('-') {
                Some(rest) => (rest, true),
                None => (f, dir.eq_ignore_ascii_case("desc")),
            };
            (f.to_string(), desc)
        })
        .collect()
}

fn hash_mix(x: u64, seed: u64) -> u64 {
    let mut z = x.wrapping_add(seed).wrapping_mul(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z ^ (z >> 31)
}

pub fn sort_items(db: &Db, user_id: u64, ids: &mut Vec<u64>, sort: &str) {
    let keys = parse_sort(sort);
    if keys.is_empty() {
        return;
    }
    let seed = now() / 3600; // random order changes once an hour
    ids.sort_by(|a, b| {
        let (Some(ia), Some(ib)) = (db.items.get(a), db.items.get(b)) else {
            return Ordering::Equal;
        };
        for (f, desc) in &keys {
            let o = match f.as_str() {
                "random" => hash_mix(*a, seed).cmp(&hash_mix(*b, seed)),
                "title" => ia.sort_title.cmp(&ib.sort_title),
                "episode_order" | "index" => {
                    let k = |i: &Item| {
                        let s = match field(db, user_id, i, "season") {
                            FieldValue::Num(n) => n.unwrap_or(0.0),
                            _ => 0.0,
                        };
                        (sort_show(db, i), (s * 10000.0) as i64 + i.index.unwrap_or(0) as i64)
                    };
                    k(ia).cmp(&k(ib))
                }
                _ => cmp_field(&field(db, user_id, ia, f), &field(db, user_id, ib, f)),
            };
            let o = if *desc { o.reverse() } else { o };
            if o != Ordering::Equal {
                return o;
            }
        }
        ia.sort_title.cmp(&ib.sort_title)
    });
}

fn sort_show(db: &Db, i: &Item) -> String {
    match field(db, 0, i, "show") {
        FieldValue::Text(t) => t.first().cloned().unwrap_or_default().to_lowercase(),
        _ => String::new(),
    }
}

/// Runs a smart query for one user: only libraries they may see, only
/// playable items unless the rules ask for shows or seasons.
pub fn run_smart(db: &Db, user: &User, q: &SmartQuery) -> Vec<u64> {
    let asks_container = q.rules.iter().any(|r| {
        (r.field == "kind" || r.field == "type")
            && matches!(as_text(&r.value).as_str(), "show" | "season")
    });
    let mut ids: Vec<u64> = db
        .items
        .values()
        .filter(|i| db.can_see_section(user, i.section_id))
        .filter(|i| q.sections.is_empty() || q.sections.contains(&i.section_id))
        .filter(|i| i.kind.playable() || asks_container)
        .filter(|i| matches_all(db, user.id, i, &q.rules, q.match_all))
        .map(|i| i.id)
        .collect();
    sort_items(db, user.id, &mut ids, if q.sort.is_empty() { "title" } else { &q.sort });
    if let Some(limit) = q.limit {
        ids.truncate(limit);
    }
    ids
}

/// The items a playlist holds right now for this user.
pub fn playlist_items(db: &Db, user: &User, p: &Playlist) -> Vec<u64> {
    match (&p.kind, &p.query) {
        (crate::db::PlaylistKind::Smart, Some(q)) => run_smart(db, user, q),
        _ => p
            .items
            .iter()
            .copied()
            .filter(|id| db.items.get(id).is_some_and(|i| db.can_see_section(user, i.section_id)))
            .collect(),
    }
}

/// Turns URL query parameters into rules. Supported forms:
///   genre=Drama            (is)
///   year>=2000  year<=2010 (written year%3E=2000 when encoded; gte / lte)
///   title~=matrix          (contains)
///   kind!=episode          (is_not)
///   rating.gt=7            (any operator after a dot)
pub fn rules_from_params(params: &[(String, String)]) -> Vec<Rule> {
    const RESERVED: &[&str] = &[
        "sort", "limit", "offset", "start", "size", "token", "match", "q", "query",
        "smart", "playlisttype", "owner", "includecollections", "container-start", "container-size",
        "unwatched", "filter",
    ];
    let mut rules = vec![];
    for (k, v) in params {
        let lk = k.to_lowercase();
        if RESERVED.contains(&lk.as_str()) {
            continue;
        }
        let (fieldname, op) = if let Some(f) = lk.strip_suffix('>') {
            (f.trim_end_matches('>').to_string(), "gte")
        } else if let Some(f) = lk.strip_suffix('<') {
            (f.trim_end_matches('<').to_string(), "lte")
        } else if let Some(f) = lk.strip_suffix('!') {
            (f.to_string(), "is_not")
        } else if let Some(f) = lk.strip_suffix('~') {
            (f.to_string(), "contains")
        } else if let Some((f, o)) = lk.split_once('.') {
            (f.to_string(), OPERATORS.iter().find(|x| x.0 == o).map(|x| x.0).unwrap_or("is"))
        } else {
            (lk.clone(), "is")
        };
        let fieldname = if fieldname == "type" { "kind".to_string() } else { fieldname };
        let value = if v.contains(',') && (op == "is" || op == "in") {
            return_rules_in(&mut rules, &fieldname, v);
            continue;
        } else {
            Value::String(v.clone())
        };
        rules.push(Rule { field: fieldname, op: op.into(), value });
    }
    // The ?unwatched=1 shortcut
    if params.iter().any(|(k, v)| k == "unwatched" && v == "1") {
        rules.push(crate::db::rule("watched", "is", Value::Bool(false)));
    }
    rules
}

fn return_rules_in(rules: &mut Vec<Rule>, field: &str, v: &str) {
    rules.push(Rule { field: field.into(), op: "in".into(), value: Value::String(v.into()) });
}

/// Filter and sort for the list of playlists.
pub fn playlist_matches(p: &Playlist, user: &User, params: &[(String, String)], item_count: usize) -> bool {
    for (k, v) in params {
        let v = v.to_lowercase();
        let ok = match k.to_lowercase().as_str() {
            "smart" => (p.kind == crate::db::PlaylistKind::Smart) == (v == "1" || v == "true"),
            "kind" => match v.as_str() {
                "default" => p.is_default(),
                "smart" => p.kind == crate::db::PlaylistKind::Smart && !p.is_default(),
                "manual" | "regular" => p.kind == crate::db::PlaylistKind::Manual,
                _ => true,
            },
            "default" => p.is_default() == (v == "1" || v == "true"),
            "title" | "q" | "query" => p.title.to_lowercase().contains(&v),
            "owner" => match v.as_str() {
                "me" => p.owner_id == user.id,
                "others" => p.owner_id != user.id && p.owner_id != 0,
                "server" => p.owner_id == 0,
                n => n.parse::<u64>().map(|n| n == p.owner_id).unwrap_or(true),
            },
            "shared" => p.shared == (v == "1" || v == "true"),
            "pinned" => p.pinned == (v == "1" || v == "true"),
            "min_items" | "item_count>" => v.parse::<usize>().map(|n| item_count >= n).unwrap_or(true),
            "max_items" | "item_count<" => v.parse::<usize>().map(|n| item_count <= n).unwrap_or(true),
            "playlisttype" => v == "video" || v.is_empty(),
            _ => true,
        };
        if !ok {
            return false;
        }
    }
    true
}

pub fn sort_playlists(list: &mut [(Playlist, usize, u64)], sort: &str) {
    let keys = parse_sort(if sort.is_empty() { "pinned:desc,title" } else { sort });
    list.sort_by(|(a, ca, da), (b, cb, db_)| {
        for (f, desc) in &keys {
            let o = match f.as_str() {
                "title" => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
                "created_at" | "added_at" => a.created_at.cmp(&b.created_at),
                "updated_at" => a.updated_at.cmp(&b.updated_at),
                "item_count" | "leaf_count" | "size" => ca.cmp(cb),
                "duration" => da.cmp(db_),
                "kind" | "smart" => (a.kind as u8).cmp(&(b.kind as u8)),
                "pinned" => a.pinned.cmp(&b.pinned),
                "owner" => a.owner_id.cmp(&b.owner_id),
                _ => Ordering::Equal,
            };
            let o = if *desc { o.reverse() } else { o };
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
}
