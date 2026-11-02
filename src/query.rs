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
    ("decade", "number", "Decade of release (1990, 2000 …)"),
    ("collection", "text", "Collection (film series)"),
    ("director", "text", "Director"),
    ("actor", "text", "Actor"),
    ("writer", "text", "Writer"),
    ("producer", "text", "Producer"),
    ("country", "text", "Country of production"),
    ("network", "text", "TV network (the show's studio)"),
    ("label", "text", "Your own labels"),
    ("edition", "text", "Edition, from {edition-…} in the file name"),
    ("video_resolution", "text", "4k, 1080, 720, 480 or sd"),
    ("hdr", "bool", "HDR picture (HDR10, HLG or HDR10+)"),
    ("hdr10plus", "bool", "HDR10+ picture"),
    ("dovi", "bool", "Dolby Vision picture"),
    ("atmos", "bool", "Dolby Atmos sound"),
    ("subtitle_codec", "text", "srt, ass, pgs …"),
    ("audio_layout", "text", "mono, stereo, 5.1, 7.1 …"),
    ("folder", "text", "Folder the file is in"),
    ("unwatched", "bool", "Not seen yet"),
    ("unwatched_leaves", "bool", "Show or season with episodes not seen yet"),
    ("unmatched", "bool", "No online details found yet"),
    ("duplicate", "bool", "The same movie or episode is in the library more than once"),
    ("random", "sort", "Random order (sort only)"),
];

/// The filters offered on a library page, per kind of item, in order.
pub const MOVIE_FILTERS: &[&str] = &[
    "genre", "year", "decade", "content_rating", "collection", "director", "actor", "writer", "producer", "country", "studio",
    "video_resolution", "hdr", "dovi", "hdr10plus", "atmos", "unwatched", "in_progress", "unmatched", "video_codec", "audio_codec",
    "subtitle_codec", "audio_layout", "audio_language", "subtitle_language", "edition", "label", "folder", "duplicate",
];
pub const SHOW_FILTERS: &[&str] = &[
    "genre", "year", "content_rating", "network", "country", "collection", "director", "actor", "writer", "producer",
    "unwatched_leaves", "unmatched", "edition", "label",
];
pub const EPISODE_FILTERS: &[&str] = &[
    "year", "collection", "video_resolution", "hdr", "dovi", "hdr10plus", "atmos", "unwatched", "in_progress", "video_codec", "audio_codec",
    "subtitle_codec", "audio_layout", "audio_language", "subtitle_language", "unmatched", "folder", "duplicate",
];
/// Shown only to the server's owner ( adds them when the account is 1).
pub const OWNER_FILTERS: &[&str] = &["label", "folder"];
/// Shown only when the client asks for ?includeAdvanced=1 (the only advanced
/// filter).
pub const ADVANCED_FILTERS: &[&str] = &["duplicate"];

pub fn filters_for(kind: ItemKind) -> &'static [&'static str] {
    match kind {
        ItemKind::Show | ItemKind::Season => SHOW_FILTERS,
        ItemKind::Episode => EPISODE_FILTERS,
        _ => MOVIE_FILTERS,
    }
}

/// The names a URL may use for a field (lower case), mapped to this build's fields.
pub fn canonical_field(name: &str) -> &str {
    match name {
        "type" => "kind",
        "videocodec" => "video_codec",
        "audiocodec" => "audio_codec",
        "subtitlecodec" => "subtitle_codec",
        "audiolanguage" => "audio_language",
        "subtitlelanguage" => "subtitle_language",
        "audiolayout" | "audiochannellayout" => "audio_layout",
        "videoresolution" => "video_resolution",
        "contentrating" => "content_rating",
        "inprogress" => "in_progress",
        "unwatchedleaves" => "unwatched_leaves",
        "userrating" => "user_rating",
        "addedat" => "added_at",
        "lastviewedat" => "last_viewed_at",
        "viewcount" => "view_count",
        "location" => "folder",
        "editiontitle" => "edition",
        other => other,
    }
}

/// The name for a picture size: the longer side
/// counts as the width; a size reaches a step when its width or its height
/// does (table, width × height).
pub const RESOLUTION_STEPS: &[(u32, u32, &str)] = &[
    (15360, 8640, "16k"),
    (7680, 4320, "8k"),
    (3776, 2112, "4k"),
    (2688, 1472, "2.7k"),
    (2032, 1072, "2k"),
    (1904, 1072, "1080"),
    (1264, 704, "720"),
    (1008, 560, "576"),
    (848, 464, "480"),
];

pub fn resolution_name(width: u32, height: u32) -> &'static str {
    if width == 0 || height == 0 {
        return "";
    }
    let (w, h) = if width < height { (height, width) } else { (width, height) };
    for (sw, sh, name) in RESOLUTION_STEPS {
        // "2k" is reached by the width alone.
        let reached = if *name == "2k" { w >= *sw } else { w >= *sw || h >= *sh };
        if reached {
            return name;
        }
    }
    "sd"
}

fn layout_name(s: &crate::db::Stream) -> String {
    let l = s.channel_layout.split('(').next().unwrap_or("").trim();
    if !l.is_empty() {
        return l.to_string();
    }
    match s.channels {
        0 => String::new(),
        1 => "mono".into(),
        2 => "stereo".into(),
        6 => "5.1".into(),
        8 => "7.1".into(),
        n => format!("{n} channels"),
    }
}

/// The text inside {edition-…} in a file or folder name.
pub fn edition_of(path: &str) -> String {
    let lower = path.to_lowercase();
    let Some(at) = lower.rfind("{edition-") else { return String::new() };
    let rest = &path[at + "{edition-".len()..];
    rest.split('}').next().unwrap_or("").trim().to_string()
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from).or_else(|| x["name"].as_str().map(String::from))).collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        _ => vec![],
    }
}

/// Movies and episodes that are in the same library more than once (same
/// title and year, or the same episode of the same season). Worked out once
/// for many items in a row.
fn duplicates(db: &Db) -> std::rc::Rc<std::collections::HashSet<u64>> {
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;
    type Cached = Option<((usize, usize, u64), Rc<HashSet<u64>>)>;
    thread_local! {
        static CACHE: RefCell<Cached> = const { RefCell::new(None) };
    }
    let key = (db as *const Db as usize, db.items.len(), now() / 10);
    if let Some(hit) = CACHE.with(|c| c.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return hit;
    }
    let mut groups: HashMap<(u64, String), Vec<u64>> = HashMap::new();
    for i in db.items.values().filter(|i| i.file.is_some()) {
        let k = match i.kind {
            ItemKind::Episode => match (i.parent_id, i.index) {
                (Some(p), Some(n)) => (p, format!("e{n}")),
                _ => continue,
            },
            _ => (i.section_id, format!("{}|{}", i.sort_title.to_lowercase(), i.year.unwrap_or(0))),
        };
        groups.entry(k).or_default().push(i.id);
    }
    let set: HashSet<u64> = groups.into_values().filter(|g| g.len() > 1).flatten().collect();
    let set = Rc::new(set);
    CACHE.with(|c| *c.borrow_mut() = Some((key, set.clone())));
    set
}

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
    // Seasons and episodes share the show's people, collections and country.
    let top = || -> &Item {
        match item.kind {
            ItemKind::Season => item.parent_id.and_then(|p| db.items.get(&p)).unwrap_or(item),
            ItemKind::Episode => show().unwrap_or(item),
            _ => item,
        }
    };
    let inherited = |key: &str| -> Vec<String> {
        let own = str_list(item.extra.get(key));
        if own.is_empty() { str_list(top().extra.get(key)) } else { own }
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
            watched_container(db).1
        }),
        "unwatched" => Bool(if item.kind.playable() { view.view_count == 0 } else { !watched_container(db).0 }),
        "unwatched_leaves" => Bool(if item.kind.playable() {
            view.view_count == 0
        } else {
            db.leaves(item.id).iter().any(|l| db.view(user_id, *l).view_count == 0)
        }),
        "decade" => Num(item.year.or(top().year).map(|y| (y / 10 * 10) as f64)),
        "collection" => Text(inherited("collections")),
        "director" => Text(inherited("directors")),
        "writer" => Text(inherited("writers")),
        "producer" => Text(inherited("producers")),
        "country" => Text(inherited("countries")),
        "actor" => Text(inherited("cast")),
        "label" => Text(str_list(item.extra.get("labels"))),
        "network" => Text(vec![top().studio.clone()]),
        "edition" => Text({
            let e = item.extra.get("edition").and_then(|v| v.as_str()).map(String::from)
                .unwrap_or_else(|| file.map(|f| edition_of(&f.path)).unwrap_or_default());
            if e.is_empty() { vec![] } else { vec![e] }
        }),
        "unmatched" => Bool({
            let t = top();
            matches!(t.kind, ItemKind::Movie | ItemKind::Show) && !t.extra.contains_key("metadata_source")
        }),
        "duplicate" => Bool(duplicates(db).contains(&item.id)),
        "video_resolution" => Text(file.and_then(|f| f.video()).map(|v| resolution_name(v.width, v.height)).filter(|r| !r.is_empty()).map(|r| vec![r.to_string()]).unwrap_or_default()),
        "hdr" => Bool(file.and_then(|f| f.video()).is_some_and(|v| v.is_hdr())),
        "hdr10plus" => Bool(file.and_then(|f| f.video()).is_some_and(|v| v.hdr10plus)),
        "dovi" => Bool(file.and_then(|f| f.video()).is_some_and(|v| v.dovi)),
        "atmos" => Bool(file.is_some_and(|f| f.streams.iter().any(|s| s.kind == "audio" && s.is_atmos()))),
        "subtitle_codec" => Text(file.map(|f| {
            let mut v: Vec<String> = f.streams.iter().filter(|s| s.kind == "subtitle").map(|s| s.codec.clone()).collect();
            v.extend(f.external_subtitles.iter().filter_map(|p| p.rsplit('.').next().map(|e| e.to_lowercase())));
            v
        }).unwrap_or_default()),
        "audio_layout" => Text(file.map(|f| f.streams.iter().filter(|s| s.kind == "audio").map(layout_name).filter(|s| !s.is_empty()).collect()).unwrap_or_default()),
        "folder" => Text(file.and_then(|f| std::path::Path::new(&f.path).parent().map(|p| p.to_string_lossy().to_string())).into_iter().collect()),
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
        "filter", "includeadvanced", "includemeta", "includeguids",
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
        let mut fieldname = canonical_field(&fieldname).to_string();
        // The resolution filter uses names (4k, 1080, sd); a number stays a picture height.
        if fieldname == "resolution" && v.split(',').any(|x| x.trim().parse::<f64>().is_err()) {
            fieldname = "video_resolution".into();
        }
        let value = if v.contains(',') && (op == "is" || op == "in") {
            return_rules_in(&mut rules, &fieldname, v);
            continue;
        } else {
            Value::String(v.clone())
        };
        rules.push(Rule { field: fieldname, op: op.into(), value });
    }
    rules
}

fn return_rules_in(rules: &mut Vec<Rule>, field: &str, v: &str) {
    rules.push(Rule { field: field.into(), op: "in".into(), value: Value::String(v.into()) });
}

/// For a library page: every filter offered for this kind of
/// item, with the values found and how many items have each.
pub fn section_filters(db: &Db, user_id: u64, items: &[&Item], kind: ItemKind, owner: bool, advanced: bool) -> Vec<Value> {
    use std::collections::BTreeMap;
    let label = |f: &'static str| -> &'static str { ITEM_FIELDS.iter().find(|x| x.0 == f).map(|x| x.2).unwrap_or(f) };
    let mut out = vec![];
    for f in filters_for(kind) {
        if (!owner && OWNER_FILTERS.contains(f)) || (!advanced && ADVANCED_FILTERS.contains(f)) {
            continue;
        }
        let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
        let mut is_bool = false;
        let mut yes = 0;
        for it in items {
            match field(db, user_id, it, f) {
                FieldValue::Bool(b) => {
                    is_bool = true;
                    yes += b as usize;
                }
                FieldValue::Num(Some(n)) => {
                    let e = counts.entry(format!("{n:012.0}")).or_insert((format!("{n}"), 0));
                    e.1 += 1;
                }
                FieldValue::Num(None) => {}
                FieldValue::Text(vals) => {
                    let mut seen = std::collections::HashSet::new();
                    for v in vals.into_iter().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
                        if seen.insert(v.to_lowercase()) {
                            let e = counts.entry(v.to_lowercase()).or_insert((v, 0));
                            e.1 += 1;
                        }
                    }
                }
            }
        }
        let mut values: Vec<(String, usize)> = counts.into_values().collect();
        if *f == "year" || *f == "decade" {
            values.reverse();
        }
        if *f == "video_resolution" {
            let rank = |v: &str| RESOLUTION_STEPS.iter().position(|x| x.2 == v).unwrap_or(RESOLUTION_STEPS.len());
            values.sort_by_key(|(v, _)| rank(v));
        }
        out.push(serde_json::json!({
            "filter": f,
            "title": label(f),
            "type": if is_bool { "boolean" } else if *f == "decade" || *f == "year" { "integer" } else { "string" },
            "advanced": ADVANCED_FILTERS.contains(f),
            "count": if is_bool { Some(yes) } else { None },
            "values": values.iter().map(|(v, c)| serde_json::json!({"value": v, "count": c})).collect::<Vec<_>>(),
        }));
    }
    out
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
            "section" => true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{MediaFile, Stream};
    use serde_json::json;

    fn item(id: u64, title: &str, year: u32, extra: Value) -> Item {
        serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": title, "sort_title": title.to_lowercase(),
            "year": year, "added_at": 0, "updated_at": 0, "extra": extra})).unwrap()
    }

    #[test]
    fn original_library_filters() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::db::Settings::default(),
            "sections": [], "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let mut a = item(1, "Alpha", 1994, json!({"directors": ["Dee Rector"], "countries": ["France"], "collections": ["Alpha Saga"],
            "cast": [{"name": "Ann Actor"}], "labels": ["Kids"], "metadata_source": "tmdb"}));
        a.file = Some(MediaFile {
            path: "/m/Alpha (1994) {edition-Director's Cut}/a.mkv".into(), probed: true,
            streams: vec![
                Stream { kind: "video".into(), codec: "hevc".into(), width: 3840, height: 2160, color_transfer: "smpte2084".into(), hdr10plus: true, ..Default::default() },
                Stream { kind: "audio".into(), codec: "truehd".into(), channels: 8, profile: "Dolby TrueHD + Dolby Atmos".into(), channel_layout: "7.1".into(), ..Default::default() },
                Stream { kind: "subtitle".into(), codec: "subrip".into(), ..Default::default() },
            ],
            ..Default::default()
        });
        let mut b = item(2, "Beta", 2003, json!({}));
        b.file = Some(MediaFile { path: "/m/b.mp4".into(), streams: vec![Stream { kind: "video".into(), codec: "h264".into(), width: 1280, height: 720, ..Default::default() },
            Stream { kind: "audio".into(), codec: "aac".into(), channels: 2, ..Default::default() }], ..Default::default() });
        let mut c = b.clone();
        c.id = 3;
        for i in [a, b, c] {
            db.items.insert(i.id, i);
        }
        let hits = |params: &[(&str, &str)]| -> Vec<u64> {
            let p: Vec<(String, String)> = params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            let rules = rules_from_params(&p);
            let mut ids: Vec<u64> = db.items.values().filter(|i| matches_all(&db, 1, i, &rules, true)).map(|i| i.id).collect();
            ids.sort();
            ids
        };
        assert_eq!(hits(&[("decade", "1990")]), vec![1]);
        assert_eq!(hits(&[("director", "Dee Rector")]), vec![1]);
        assert_eq!(hits(&[("actor", "ann actor")]), vec![1]);
        assert_eq!(hits(&[("country", "France")]), vec![1]);
        assert_eq!(hits(&[("collection", "Alpha Saga")]), vec![1]);
        assert_eq!(hits(&[("label", "kids")]), vec![1]);
        assert_eq!(hits(&[("edition", "Director's Cut")]), vec![1]);
        assert_eq!(hits(&[("resolution", "4k")]), vec![1]);
        assert_eq!(hits(&[("resolution", "720")]), vec![2, 3]);
        assert_eq!(hits(&[("videoResolution", "720")]), vec![2, 3]);
        assert_eq!(hits(&[("hdr", "1")]), vec![1]);
        assert_eq!(hits(&[("hdr10plus", "1")]), vec![1]);
        assert_eq!(hits(&[("atmos", "1")]), vec![1]);
        assert_eq!(hits(&[("audioLayout", "7.1")]), vec![1]);
        assert_eq!(hits(&[("audio_layout", "stereo")]), vec![2, 3]);
        assert_eq!(hits(&[("subtitleCodec", "subrip")]), vec![1]);
        assert_eq!(hits(&[("videoCodec", "hevc")]), vec![1]);
        assert_eq!(hits(&[("unmatched", "1")]), vec![2, 3]);
        assert_eq!(hits(&[("duplicate", "1")]), vec![2, 3]);
        assert_eq!(hits(&[("folder", "/m")]), vec![2, 3]);
        assert_eq!(hits(&[("unwatched", "1")]), vec![1, 2, 3]);
        let all: Vec<&Item> = db.items.values().collect();
        let f = section_filters(&db, 1, &all, ItemKind::Movie, true, true);
        assert_eq!(f.len(), MOVIE_FILTERS.len());
        assert_eq!(section_filters(&db, 1, &all, ItemKind::Movie, false, false).len(), MOVIE_FILTERS.len() - 3);
        let res = f.iter().find(|x| x["filter"] == "video_resolution").unwrap();
        assert_eq!(res["values"][0]["value"], "4k");
        assert_eq!(res["values"][1]["count"], 2);
        assert_eq!(f.iter().find(|x| x["filter"] == "hdr").unwrap()["count"], 1);
        assert_eq!(edition_of("Film {Edition-Extended}.mkv"), "Extended");
        assert_eq!(resolution_name(1920, 800), "1080");
        assert_eq!(resolution_name(2048, 858), "2k");
        assert_eq!(resolution_name(3840, 1600), "4k");
        assert_eq!(resolution_name(2704, 1520), "2.7k");
        assert_eq!(resolution_name(1080, 1920), "1080");
        assert_eq!(resolution_name(720, 576), "576");
        assert_eq!(resolution_name(720, 480), "480");
        assert_eq!(resolution_name(640, 360), "sd");
        assert_eq!(resolution_name(7680, 4320), "8k");
        assert_eq!(filters_for(ItemKind::Movie).len(), 29);
        assert_eq!(filters_for(ItemKind::Show).len(), 14);
        assert_eq!(filters_for(ItemKind::Episode).len(), 18);
    }
}
