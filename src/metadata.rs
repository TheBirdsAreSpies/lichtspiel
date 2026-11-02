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

use crate::db::{ItemKind, Settings};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const UA: &str = "Lichtspiel/0.1 (self-hosted personal streaming server; metadata lookup)";
const TMDB: &str = "https://api.themoviedb.org/3";
const TMDB_IMG: &str = "https://image.tmdb.org/t/p";
const TVMAZE: &str = "https://api.tvmaze.com";

/// A possible match shown in "Fix match" and used by the automatic matcher.
#[derive(Serialize, Clone, Debug)]
pub struct Candidate {
    pub source: String,
    pub id: String,
    pub title: String,
    pub year: Option<u32>,
    pub summary: String,
    pub thumb: Option<String>,
    /// 0 … 1.2: how well title and year fit.
    pub score: f64,
}

#[derive(Default, Debug, Clone)]
pub struct Season {
    pub index: u32,
    pub summary: String,
    pub poster_url: Option<String>,
}

#[derive(Default, Debug, Clone)]
pub struct Episode {
    pub season: u32,
    pub episode: u32,
    pub title: String,
    pub summary: String,
    pub still_url: Option<String>,
    pub aired: String,
    pub rating: Option<f64>,
}

/// Everything found for one movie or show.
#[derive(Default, Debug, Clone)]
pub struct Details {
    /// In the shape `apply_metadata` takes (title, summary, year, genres, …, extra{…}).
    pub meta: Value,
    pub poster_url: Option<String>,
    pub art_url: Option<String>,
    pub seasons: Vec<Season>,
    pub episodes: Vec<Episode>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent(UA).build()
}

fn get_json(url: &str, bearer: Option<&str>) -> Result<Value, String> {
    let mut r = agent().get(url).set("Accept", "application/json");
    if let Some(b) = bearer {
        r = r.set("Authorization", &format!("Bearer {b}"));
    }
    // Busy services (Wikidata, TVmaze) answer 429 / 503 when asked too often:
    // wait as long as they say (Retry-After, at most 30 s, else 2/4/8 s) and try again.
    let mut tries = 0;
    let resp = loop {
        match r.clone().call() {
            Ok(resp) => break resp,
            Err(ureq::Error::Status(429 | 503, resp)) if tries < 3 => {
                tries += 1;
                let wait = resp.header("Retry-After").and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(1 << tries).min(30);
                std::thread::sleep(Duration::from_secs(wait));
            }
            Err(ureq::Error::Status(code, _)) => return Err(format!("{} answered HTTP {code}", host_of(url))),
            Err(other) => return Err(format!("could not reach {}: {other}", host_of(url))),
        }
    };
    let text = resp.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("{} sent something unreadable: {e}", host_of(url)))
}

fn host_of(url: &str) -> &str {
    url.split("//").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

/// The kind of picture in `b`, only for pictures a browser shows as a photo
/// (an SVG drawing or a web page saved as "poster.jpg" shows as broken).
pub fn image_type(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if b.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else {
        None
    }
}

/// `image_type` of a saved file.
pub fn file_image_type(p: &Path) -> Option<&'static str> {
    let mut head = [0u8; 16];
    let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut head)).ok()?;
    image_type(&head[..n])
}

/// For an SVG drawing on Wikimedia, the address of a PNG rendering of it.
fn raster_version(url: &str) -> Option<String> {
    let lower = url.to_lowercase();
    if lower.contains("special:filepath/") {
        return Some(if url.contains("width=") { url.to_string() } else { format!("{url}{}width=600", if url.contains('?') { '&' } else { '?' }) });
    }
    // https://upload.wikimedia.org/wikipedia/<site>/<a>/<ab>/<Name>.svg (?utm_… dropped);
    // Wikimedia only makes thumbnails in set sizes (500 works, 600 does not).
    let url = url.split('?').next().unwrap_or(url);
    let rest = url.strip_prefix("https://upload.wikimedia.org/wikipedia/")?;
    if !url.to_lowercase().ends_with(".svg") || rest.contains("/thumb/") {
        return None;
    }
    let parts: Vec<&str> = rest.splitn(4, '/').collect();
    let [site, a, ab, name] = parts.as_slice() else { return None };
    Some(format!("https://upload.wikimedia.org/wikipedia/{site}/thumb/{a}/{ab}/{name}/500px-{name}.png"))
}

/// Logos and other drawings are not posters.
fn is_logo(file: &str) -> bool {
    let f = file.to_lowercase();
    f.ends_with(".svg") || f.contains("logo") || f.contains("wordmark") || f.contains("title card") || f.contains("titlecard")
}

/// Downloads a picture (at most 15 MB) to `dest`. True when it worked.
/// Only real photos are kept; an SVG drawing is fetched as a PNG instead.
pub fn download(url: &str, dest: &Path) -> bool {
    download_try(url, dest, true)
}

fn download_try(url: &str, dest: &Path, may_convert: bool) -> bool {
    // Picture servers (Wikimedia, TMDB) answer 429 when asked too quickly: wait and retry.
    let mut tries = 0;
    let resp = loop {
        match agent().get(url).call() {
            Ok(r) => break r,
            Err(ureq::Error::Status(429 | 503, r)) if tries < 3 => {
                tries += 1;
                let wait = r.header("Retry-After").and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(1 << tries).min(30);
                std::thread::sleep(Duration::from_secs(wait));
            }
            Err(_) => return false,
        }
    };
    if resp.content_type().starts_with("text/") {
        return false;
    }
    let mut bytes = vec![];
    if resp.into_reader().take(15_000_000).read_to_end(&mut bytes).is_err() || bytes.len() < 200 {
        return false;
    }
    if image_type(&bytes).is_none() {
        return may_convert && raster_version(url).is_some_and(|u| u != url && download_try(&u, dest, false));
    }
    if let Some(dir) = dest.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = dest.with_extension("part");
    std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, dest).is_ok()
}

/// Is the internet reachable (one quick request)?
pub fn online() -> bool {
    agent().get(&format!("{TVMAZE}/shows/1")).timeout(Duration::from_secs(6)).call().is_ok()
}

pub fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b' ' => o.push_str("%20"),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ").trim().to_string()
}

fn year_of(date: &str) -> Option<u32> {
    let d = date.trim_start_matches('+');
    d.get(0..4).and_then(|y| y.parse().ok()).filter(|y| *y > 1800)
}

/// Lower-case letters and digits only, articles dropped, "&" → "and".
pub fn normalize(t: &str) -> String {
    let t = t.to_lowercase().replace('&', " and ");
    let cleaned: String = t.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let skip = matches!(words.first(), Some(&("the" | "a" | "an" | "der" | "die" | "das" | "le" | "la" | "les" | "el" | "il"))) && words.len() > 1;
    words[usize::from(skip)..].join(" ")
}

/// How well a found title/year fits what the file name said.
pub fn score(want_title: &str, want_year: Option<u32>, title: &str, year: Option<u32>) -> f64 {
    let a = normalize(want_title);
    let b = normalize(title);
    let sim = if a == b {
        1.0
    } else {
        let wa: std::collections::HashSet<&str> = a.split(' ').collect();
        let wb: std::collections::HashSet<&str> = b.split(' ').collect();
        let inter = wa.intersection(&wb).count() as f64;
        let union = wa.union(&wb).count().max(1) as f64;
        inter / union * 0.9
    };
    let ybonus = match (want_year, year) {
        (Some(a), Some(b)) if a == b => 0.2,
        (Some(a), Some(b)) if a.abs_diff(b) == 1 => 0.05,
        (Some(_), Some(_)) => -0.4,
        _ => 0.0,
    };
    sim + ybonus
}

/// Good enough to take without asking.
pub fn acceptable(c: &Candidate) -> bool {
    c.score >= 0.85
}

fn region(lang: &str) -> &'static str {
    match lang {
        "de" => "DE",
        "fr" => "FR",
        "es" => "ES",
        "it" => "IT",
        "nl" => "NL",
        "pl" => "PL",
        "pt" => "PT",
        "sv" => "SE",
        "ja" => "JP",
        _ => "US",
    }
}

fn lang_of(settings: &Settings) -> String {
    let l = settings.metadata_language.trim().to_lowercase();
    if l.len() == 2 && l.chars().all(|c| c.is_ascii_lowercase()) { l } else { "en".into() }
}

// ------------------------------------------------------------------ search

pub fn search(settings: &Settings, kind: ItemKind, title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
    let mut list = if !settings.tmdb_api_key.trim().is_empty() {
        tmdb_search(settings, kind, title, year)?
    } else if kind == ItemKind::Show {
        tvmaze_search(title, year)?
    } else {
        wiki_search(settings, title, year)?
    };
    list.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    Ok(list)
}

/// Which entry an id tag from a file or folder name ("{imdb-tt0068646}", see
/// scanner::id_tag) points to, as (source, id) for `fetch`. With a TMDB key
/// TMDB is used; without one shows go to TVmaze and movies to Wikidata.
pub fn resolve_id(settings: &Settings, kind: ItemKind, tag_source: &str, tag_id: &str) -> Result<(String, String), String> {
    let show = kind == ItemKind::Show;
    let not_found = || format!("Nothing found online for {{{tag_source}-{tag_id}}}");
    if !settings.tmdb_api_key.trim().is_empty() {
        if tag_source == "tmdb" {
            return Ok(("tmdb".into(), tag_id.into()));
        }
        let ext = if tag_source == "imdb" { "imdb_id" } else { "tvdb_id" };
        let v = tmdb_get(settings, &format!("/find/{}?external_source={ext}", enc(tag_id)))?;
        let list = if show { &v["tv_results"] } else { &v["movie_results"] };
        return list[0]["id"].as_u64().map(|i| ("tmdb".to_string(), i.to_string())).ok_or_else(not_found);
    }
    if show {
        // TVmaze looks shows up by IMDb and TheTVDB id; a TMDB id goes through Wikidata (P4983).
        let tvmaze = match tag_source {
            "imdb" => get_json(&format!("{TVMAZE}/lookup/shows?imdb={}", enc(tag_id)), None).ok().and_then(|v| v["id"].as_u64()).map(|i| i.to_string()),
            "tvdb" => get_json(&format!("{TVMAZE}/lookup/shows?thetvdb={}", enc(tag_id)), None).ok().and_then(|v| v["id"].as_u64()).map(|i| i.to_string()),
            _ => wd_value_by(&format!("wdt:P4983 \"{tag_id}\""), "P8600"),
        };
        return tvmaze.map(|i| ("tvmaze".to_string(), i)).ok_or_else(not_found);
    }
    // Movies: the Wikidata entry with that IMDb id (P345) or TMDB movie id (P4947).
    let prop = match tag_source { "imdb" => "P345", "tmdb" => "P4947", _ => return Err("A TheTVDB id needs a TMDB key for movies".into()) };
    wd_item_by(prop, tag_id).map(|q| ("wikidata".to_string(), q)).ok_or_else(not_found)
}

/// The Wikidata item (Q…) whose property `prop` has the value `value`.
fn wd_item_by(prop: &str, value: &str) -> Option<String> {
    let q = format!("SELECT ?item WHERE {{ ?item wdt:{prop} \"{value}\" }} LIMIT 1");
    let v = get_json(&format!("https://query.wikidata.org/sparql?format=json&query={}", enc(&q)), None).ok()?;
    Some(v["results"]["bindings"][0]["item"]["value"].as_str()?.rsplit('/').next()?.to_string())
}

/// The value of property `want` on the Wikidata item matching `cond` ("wdt:P… \"…\"").
fn wd_value_by(cond: &str, want: &str) -> Option<String> {
    let q = format!("SELECT ?v WHERE {{ ?item {cond} . ?item wdt:{want} ?v }} LIMIT 1");
    let v = get_json(&format!("https://query.wikidata.org/sparql?format=json&query={}", enc(&q)), None).ok()?;
    v["results"]["bindings"][0]["v"]["value"].as_str().map(String::from)
}

pub fn fetch(settings: &Settings, kind: ItemKind, source: &str, id: &str, seasons_wanted: &[u32]) -> Result<Details, String> {
    let mut d = match source {
        "tmdb" => tmdb_fetch(settings, kind, id, seasons_wanted),
        "tvmaze" => tvmaze_fetch(settings, id),
        "wikidata" => wiki_fetch(settings, id),
        _ => Err(format!("unknown source {source}")),
    }?;
    // With an OMDb key the plot is IMDb's (OMDb hands out IMDb's plot by IMDb id;
    // IMDb itself has no free service and doesn't allow reading its pages).
    if matches!(kind, ItemKind::Movie | ItemKind::Show) {
        if let Some(plot) = d.meta["extra"]["ids"]["imdb"].as_str().and_then(|i| imdb_plot(settings, i)) {
            d.meta["summary"] = json!(plot);
            d.meta["extra"]["plot_source"] = json!("imdb");
        }
    }
    Ok(d)
}

/// IMDb's full plot through OMDb (free key from omdbapi.com). English only.
pub fn imdb_plot(settings: &Settings, imdb_id: &str) -> Option<String> {
    let key = settings.omdb_api_key.trim();
    let ok_id = imdb_id.starts_with("tt") && imdb_id.len() <= 12 && imdb_id[2..].chars().all(|c| c.is_ascii_digit());
    if key.is_empty() || !ok_id || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let v = get_json(&format!("https://www.omdbapi.com/?i={imdb_id}&plot=full&apikey={key}"), None).ok()?;
    if v["Response"] != "True" {
        return None;
    }
    let p = v["Plot"].as_str()?.trim();
    (!p.is_empty() && p != "N/A").then(|| p.to_string())
}

// ------------------------------------------------------------------ TMDB

fn tmdb_get(settings: &Settings, path: &str) -> Result<Value, String> {
    let key = settings.tmdb_api_key.trim();
    let sep = if path.contains('?') { '&' } else { '?' };
    if key.starts_with("eyJ") {
        get_json(&format!("{TMDB}{path}"), Some(key))
    } else {
        get_json(&format!("{TMDB}{path}{sep}api_key={}", enc(key)), None)
    }
}

fn tmdb_img(path: &Value, size: &str) -> Option<String> {
    path.as_str().filter(|p| !p.is_empty()).map(|p| format!("{TMDB_IMG}/{size}{p}"))
}

fn tmdb_search(settings: &Settings, kind: ItemKind, title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
    let lang = lang_of(settings);
    let (kind_path, title_key, date_key, year_key) = if kind == ItemKind::Show {
        ("tv", "name", "first_air_date", "first_air_date_year")
    } else {
        ("movie", "title", "release_date", "year")
    };
    let mut path = format!("/search/{kind_path}?query={}&language={lang}&include_adult=false", enc(title));
    if let Some(y) = year {
        path.push_str(&format!("&{year_key}={y}"));
    }
    let mut v = tmdb_get(settings, &path)?;
    if year.is_some() && v["results"].as_array().is_none_or(|a| a.is_empty()) {
        v = tmdb_get(settings, &format!("/search/{kind_path}?query={}&language={lang}&include_adult=false", enc(title)))?;
    }
    Ok(v["results"].as_array().cloned().unwrap_or_default().iter().take(10).enumerate().map(|(n, r)| {
        let t = r[title_key].as_str().unwrap_or("").to_string();
        let original = r[if kind == ItemKind::Show { "original_name" } else { "original_title" }].as_str().unwrap_or("");
        let y = r[date_key].as_str().and_then(year_of);
        let s = score(title, year, &t, y).max(score(title, year, original, y)) - n as f64 * 0.01;
        Candidate { source: "tmdb".into(), id: r["id"].to_string(), title: t, year: y, summary: r["overview"].as_str().unwrap_or("").into(), thumb: tmdb_img(&r["poster_path"], "w185"), score: s }
    }).collect())
}

fn tmdb_fetch(settings: &Settings, kind: ItemKind, id: &str, seasons_wanted: &[u32]) -> Result<Details, String> {
    let lang = lang_of(settings);
    let reg = region(&lang);
    let show = kind == ItemKind::Show;
    let v = if show {
        tmdb_get(settings, &format!("/tv/{id}?language={lang}&append_to_response=aggregate_credits,content_ratings,external_ids"))?
    } else {
        tmdb_get(settings, &format!("/movie/{id}?language={lang}&append_to_response=credits,release_dates,external_ids"))?
    };
    let mut summary = v["overview"].as_str().unwrap_or("").to_string();
    if summary.is_empty() && lang != "en" {
        let en = tmdb_get(settings, &format!("/{}/{id}?language=en", if show { "tv" } else { "movie" })).unwrap_or(Value::Null);
        summary = en["overview"].as_str().unwrap_or("").to_string();
    }
    let names = |a: &Value, key: &str| -> Vec<String> { a.as_array().map(|x| x.iter().filter_map(|g| g[key].as_str().map(String::from)).collect()).unwrap_or_default() };
    let genres = names(&v["genres"], "name");
    let (title, date, studio, cert, cast, directors, writers) = if show {
        let cert = v["content_ratings"]["results"].as_array().and_then(|a| a.iter().find(|r| r["iso_3166_1"] == reg).or_else(|| a.iter().find(|r| r["iso_3166_1"] == "US"))).and_then(|r| r["rating"].as_str()).unwrap_or("").to_string();
        let cast: Vec<Value> = v["aggregate_credits"]["cast"].as_array().cloned().unwrap_or_default().iter().take(20).map(|c| json!({
            "name": c["name"], "role": c["roles"][0]["character"].as_str().unwrap_or(""), "photo": tmdb_img(&c["profile_path"], "w185"), "person_id": format!("tmdb-{}", c["id"]),
        })).collect();
        let creators = names(&v["created_by"], "name");
        (v["name"].as_str().unwrap_or("").to_string(), v["first_air_date"].as_str().unwrap_or("").to_string(),
            v["networks"][0]["name"].as_str().or(v["production_companies"][0]["name"].as_str()).unwrap_or("").to_string(), cert, cast, creators, vec![])
    } else {
        let rel = v["release_dates"]["results"].as_array().cloned().unwrap_or_default();
        let cert_for = |c: &str| rel.iter().find(|r| r["iso_3166_1"] == c).and_then(|r| r["release_dates"].as_array().and_then(|d| d.iter().filter_map(|x| x["certification"].as_str()).find(|s| !s.is_empty())).map(String::from));
        let cert = cert_for(reg).or_else(|| cert_for("US")).unwrap_or_default();
        let cast: Vec<Value> = v["credits"]["cast"].as_array().cloned().unwrap_or_default().iter().take(20).map(|c| json!({
            "name": c["name"], "role": c["character"].as_str().unwrap_or(""), "photo": tmdb_img(&c["profile_path"], "w185"), "person_id": format!("tmdb-{}", c["id"]),
        })).collect();
        let crew = v["credits"]["crew"].as_array().cloned().unwrap_or_default();
        let mut directors: Vec<String> = crew.iter().filter(|c| c["job"] == "Director").filter_map(|c| c["name"].as_str().map(String::from)).collect();
        directors.dedup();
        let mut writers: Vec<String> = vec![];
        for c in crew.iter().filter(|c| c["department"] == "Writing") {
            if let Some(n) = c["name"].as_str() {
                if !writers.iter().any(|w| w == n) && writers.len() < 6 {
                    writers.push(n.into());
                }
            }
        }
        (v["title"].as_str().unwrap_or("").to_string(), v["release_date"].as_str().unwrap_or("").to_string(),
            v["production_companies"][0]["name"].as_str().unwrap_or("").to_string(), cert, cast, directors, writers)
    };
    let mut ids = json!({"tmdb": id});
    if let Some(i) = v["external_ids"]["imdb_id"].as_str().or(v["imdb_id"].as_str()).filter(|s| !s.is_empty()) {
        ids["imdb"] = json!(i);
    }
    if let Some(i) = v["external_ids"]["tvdb_id"].as_u64() {
        ids["tvdb"] = json!(i);
    }
    let rating = v["vote_average"].as_f64().filter(|r| *r > 0.0).map(|r| (r * 10.0).round() / 10.0);
    let mut extra = json!({
        "cast": cast, "ids": ids, "metadata_source": "tmdb", "metadata_id": id, "metadata_language": lang_of(settings),
        "original_title": v[if show { "original_name" } else { "original_title" }],
    });
    if show { extra["creators"] = json!(directors) } else { extra["directors"] = json!(directors); extra["writers"] = json!(writers); }
    // Review sites (from Wikidata) plus TMDB's own user score.
    let mut ratings = scores_by_ids(&extra["ids"], show);
    if let Some(r) = rating {
        ratings["tmdb"] = json!({"score": format!("{r:.1}/10"), "count": v["vote_count"].as_u64(), "count_label": "votes", "source": "TMDB",
            "url": format!("https://www.themoviedb.org/{}/{id}", if show { "tv" } else { "movie" }), "as_of": crate::db::today()});
    }
    extra["ratings"] = ratings;
    // Country, producer and collection, for the library filters.
    extra["countries"] = json!(names(&v["production_countries"], "name"));
    if !show {
        let mut producers: Vec<String> = vec![];
        for c in v["credits"]["crew"].as_array().cloned().unwrap_or_default().iter().filter(|c| c["job"] == "Producer") {
            if let Some(n) = c["name"].as_str() {
                if !producers.iter().any(|w| w == n) && producers.len() < 6 {
                    producers.push(n.into());
                }
            }
        }
        extra["producers"] = json!(producers);
        if let Some(c) = v["belongs_to_collection"]["name"].as_str() {
            extra["collections"] = json!([c]);
        }
    }
    if let Some(r) = v["runtime"].as_u64() { extra["runtime_minutes"] = json!(r); }
    if !date.is_empty() { extra["originally_available"] = json!(date); }
    let mut meta = json!({"title": title, "summary": summary, "tagline": v["tagline"].as_str().unwrap_or(""), "genres": genres,
        "studio": studio, "content_rating": cert, "extra": extra});
    if let Some(y) = year_of(&date) { meta["year"] = json!(y); }
    if let Some(r) = rating { meta["rating"] = json!(r); }
    let mut d = Details { meta, poster_url: tmdb_img(&v["poster_path"], "w500"), art_url: tmdb_img(&v["backdrop_path"], "w1280"), ..Default::default() };
    if show {
        for s in v["seasons"].as_array().cloned().unwrap_or_default() {
            let n = s["season_number"].as_u64().unwrap_or(0) as u32;
            d.seasons.push(Season { index: n, summary: s["overview"].as_str().unwrap_or("").into(), poster_url: tmdb_img(&s["poster_path"], "w500") });
        }
        for n in seasons_wanted {
            let Ok(sv) = tmdb_get(settings, &format!("/tv/{id}/season/{n}?language={lang}")) else { continue };
            for e in sv["episodes"].as_array().cloned().unwrap_or_default() {
                d.episodes.push(Episode {
                    season: *n,
                    episode: e["episode_number"].as_u64().unwrap_or(0) as u32,
                    title: e["name"].as_str().unwrap_or("").into(),
                    summary: e["overview"].as_str().unwrap_or("").into(),
                    still_url: tmdb_img(&e["still_path"], "w500"),
                    aired: e["air_date"].as_str().unwrap_or("").into(),
                    rating: e["vote_average"].as_f64().filter(|r| *r > 0.0),
                });
            }
        }
    }
    Ok(d)
}

// ------------------------------------------------------------------ TVmaze

fn tvmaze_search(title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
    let v = get_json(&format!("{TVMAZE}/search/shows?q={}", enc(title)), None)?;
    Ok(v.as_array().cloned().unwrap_or_default().iter().take(10).enumerate().map(|(n, r)| {
        let s = &r["show"];
        let t = s["name"].as_str().unwrap_or("").to_string();
        let y = s["premiered"].as_str().and_then(year_of);
        Candidate {
            source: "tvmaze".into(), id: s["id"].to_string(), title: t.clone(), year: y,
            summary: strip_html(s["summary"].as_str().unwrap_or("")), thumb: s["image"]["medium"].as_str().map(String::from),
            score: score(title, year, &t, y) - n as f64 * 0.01,
        }
    }).collect())
}

/// TVmaze is English only. For other languages the show's own title and the
/// opening of its Wikipedia article in that language are used (found through
/// Wikidata by the TVmaze id, P8600, or the IMDb id, P345).
fn localize_show(lang: &str, tvmaze_id: &str, imdb: Option<&str>) -> Option<(String, String)> {
    let mut cond = format!("{{ ?item wdt:P8600 \"{tvmaze_id}\" }}");
    if let Some(i) = imdb.filter(|i| i.starts_with("tt") && i[2..].chars().all(|c| c.is_ascii_digit())) {
        cond.push_str(&format!(" UNION {{ ?item wdt:P345 \"{i}\" }}"));
    }
    let q = format!("SELECT ?item WHERE {{ {cond} }} LIMIT 1");
    let v = get_json(&format!("https://query.wikidata.org/sparql?format=json&query={}", enc(&q)), None).ok()?;
    let qid = v["results"]["bindings"][0]["item"]["value"].as_str()?.rsplit('/').next()?.to_string();
    let ents = wd_entities(&[qid.clone()], lang, "labels|sitelinks").ok()?;
    let e = &ents[qid.as_str()];
    let title = e["labels"][lang]["value"].as_str().unwrap_or("").to_string();
    let summary = e["sitelinks"][format!("{lang}wiki")]["title"].as_str().and_then(|a| wiki_intro(lang, a)).unwrap_or_default();
    (!title.is_empty() || !summary.is_empty()).then_some((title, summary))
}

/// The opening paragraphs of a Wikipedia article.
fn wiki_intro(lang: &str, article: &str) -> Option<String> {
    let v = wiki_api(lang, &format!("action=query&prop=extracts&exintro=1&explaintext=1&redirects=1&titles={}", enc(article))).ok()?;
    let t = v["query"]["pages"][0]["extract"].as_str()?.trim().to_string();
    (t.len() > 40).then(|| shorten(&t, 1200))
}

fn tvmaze_fetch(settings: &Settings, id: &str) -> Result<Details, String> {
    let lang = lang_of(settings);
    let v = get_json(&format!("{TVMAZE}/shows/{id}?embed[]=cast&embed[]=crew&embed[]=episodes&embed[]=seasons"), None)?;
    let e = &v["_embedded"];
    let cast: Vec<Value> = e["cast"].as_array().cloned().unwrap_or_default().iter().take(20).map(|c| json!({
        "name": c["person"]["name"], "role": c["character"]["name"].as_str().unwrap_or(""), "photo": c["person"]["image"]["medium"], "person_id": format!("tvmaze-{}", c["person"]["id"]),
    })).collect();
    let creators: Vec<String> = e["crew"].as_array().cloned().unwrap_or_default().iter()
        .filter(|c| c["type"] == "Creator").filter_map(|c| c["person"]["name"].as_str().map(String::from)).collect();
    let genres: Vec<String> = v["genres"].as_array().map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut ids = json!({"tvmaze": id});
    if let Some(i) = v["externals"]["imdb"].as_str() { ids["imdb"] = json!(i); }
    if let Some(i) = v["externals"]["thetvdb"].as_u64() { ids["tvdb"] = json!(i); }
    let date = v["premiered"].as_str().unwrap_or("").to_string();
    let mut meta = json!({
        "title": v["name"], "summary": strip_html(v["summary"].as_str().unwrap_or("")), "genres": genres,
        "studio": v["network"]["name"].as_str().or(v["webChannel"]["name"].as_str()).unwrap_or(""),
        "extra": {"cast": cast, "creators": creators, "ids": ids, "metadata_source": "tvmaze", "metadata_id": id, "originally_available": date},
    });
    if let Some(y) = year_of(&date) { meta["year"] = json!(y); }
    if let Some(r) = v["rating"]["average"].as_f64() { meta["rating"] = json!(r); }
    meta["extra"]["metadata_language"] = json!("en");
    meta["extra"]["ratings"] = scores_by_ids(&meta["extra"]["ids"], true);
    if lang != "en" {
        if let Some((t, s)) = localize_show(&lang, id, v["externals"]["imdb"].as_str()) {
            if !t.is_empty() {
                meta["extra"]["original_title"] = v["name"].clone();
                meta["title"] = json!(t);
            }
            if !s.is_empty() {
                meta["summary"] = json!(s);
            }
            meta["extra"]["metadata_language"] = json!(lang);
        }
    }
    // A wide background picture, if TVmaze has one.
    let art_url = get_json(&format!("{TVMAZE}/shows/{id}/images"), None).ok().and_then(|imgs| {
        imgs.as_array()?.iter().find(|i| i["type"] == "background").and_then(|i| i["resolutions"]["original"]["url"].as_str().map(String::from))
    });
    let mut d = Details { meta, poster_url: v["image"]["original"].as_str().map(String::from), art_url, ..Default::default() };
    for s in e["seasons"].as_array().cloned().unwrap_or_default() {
        d.seasons.push(Season { index: s["number"].as_u64().unwrap_or(0) as u32, summary: strip_html(s["summary"].as_str().unwrap_or("")), poster_url: s["image"]["original"].as_str().map(String::from) });
    }
    for ep in e["episodes"].as_array().cloned().unwrap_or_default() {
        d.episodes.push(Episode {
            season: ep["season"].as_u64().unwrap_or(0) as u32,
            episode: ep["number"].as_u64().unwrap_or(0) as u32,
            title: ep["name"].as_str().unwrap_or("").into(),
            summary: strip_html(ep["summary"].as_str().unwrap_or("")),
            still_url: ep["image"]["original"].as_str().map(String::from),
            aired: ep["airdate"].as_str().unwrap_or("").into(),
            rating: ep["rating"]["average"].as_f64(),
        });
    }
    Ok(d)
}

// ------------------------------------------------------------------ Wikidata + Wikipedia (movies)

fn film_word(lang: &str) -> &'static str {
    match lang {
        "es" => "película",
        "pt" => "filme",
        "ja" => "映画",
        "de" => "Film",
        _ => "film",
    }
}

/// Section titles that hold the plot in each language's Wikipedia.
fn plot_headings(lang: &str) -> &'static [&'static str] {
    match lang {
        "de" => &["Handlung", "Inhalt"],
        "fr" => &["Synopsis", "Résumé", "Intrigue"],
        "es" => &["Sinopsis", "Argumento", "Trama"],
        "it" => &["Trama"],
        "nl" => &["Verhaal", "Plot", "Synopsis"],
        "pl" => &["Fabuła", "Opis fabuły"],
        "pt" => &["Enredo", "Sinopse", "Trama"],
        "sv" => &["Handling", "Handlingen"],
        "ja" => &["あらすじ", "ストーリー"],
        _ => &["Plot", "Synopsis", "Premise", "Plot summary"],
    }
}

fn wiki_api(lang: &str, query: &str) -> Result<Value, String> {
    get_json(&format!("https://{lang}.wikipedia.org/w/api.php?format=json&formatversion=2&{query}"), None)
}

fn wd_entities(ids: &[String], lang: &str, props: &str) -> Result<Value, String> {
    let mut all = serde_json::Map::new();
    for chunk in ids.chunks(50) {
        let v = get_json(&format!(
            "https://www.wikidata.org/w/api.php?action=wbgetentities&format=json&ids={}&props={props}&languages={lang}|en",
            chunk.join("|")
        ), None)?;
        if let Some(Value::Object(m)) = v.get("entities") {
            for (k, e) in m {
                all.insert(k.clone(), e.clone());
            }
        }
    }
    Ok(Value::Object(all))
}

fn label(e: &Value, lang: &str) -> String {
    e["labels"][lang]["value"].as_str().or(e["labels"]["en"]["value"].as_str()).unwrap_or("").to_string()
}

fn claim_ids(e: &Value, prop: &str) -> Vec<String> {
    e["claims"][prop].as_array().map(|a| a.iter().filter_map(|c| c["mainsnak"]["datavalue"]["value"]["id"].as_str().map(String::from)).collect()).unwrap_or_default()
}

fn claim_str(e: &Value, prop: &str) -> Option<String> {
    e["claims"][prop].as_array()?.iter().find_map(|c| c["mainsnak"]["datavalue"]["value"].as_str().map(String::from))
}

fn claim_year(e: &Value) -> Option<u32> {
    e["claims"]["P577"].as_array()?.iter().filter_map(|c| c["mainsnak"]["datavalue"]["value"]["time"].as_str().and_then(year_of)).min()
}

/// A film-like Wikidata item: has a publication date and a director or cast.
fn is_film(e: &Value) -> bool {
    let films = ["Q11424", "Q202866", "Q24869", "Q506240", "Q229390", "Q93204", "Q17123180", "Q20650540", "Q226730", "Q24862", "Q1257444", "Q130232"];
    claim_ids(e, "P31").iter().any(|q| films.contains(&q.as_str()))
        || (e["claims"]["P577"].is_array() && (e["claims"]["P57"].is_array() || e["claims"]["P161"].is_array()))
}

fn wiki_search(settings: &Settings, title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
    let lang = lang_of(settings);
    let mut q = format!("{title} {}", film_word(&lang));
    if let Some(y) = year {
        q = format!("{title} {y} {}", film_word(&lang));
    }
    let s = wiki_api(&lang, &format!("action=query&list=search&srlimit=8&srprop=&srsearch={}", enc(&q)))?;
    let titles: Vec<String> = s["query"]["search"].as_array().map(|a| a.iter().filter_map(|r| r["title"].as_str().map(String::from)).collect()).unwrap_or_default();
    if titles.is_empty() {
        return Ok(vec![]);
    }
    let pp = wiki_api(&lang, &format!("action=query&prop=pageprops&ppprop=wikibase_item&redirects=1&titles={}", enc(&titles.join("|"))))?;
    let mut order: Vec<(String, String)> = vec![]; // (wikipedia title, Q-id) in search order
    if let Some(pages) = pp["query"]["pages"].as_array() {
        for t in &titles {
            if let Some(p) = pages.iter().find(|p| p["title"].as_str() == Some(t)) {
                if let Some(q) = p["pageprops"]["wikibase_item"].as_str() {
                    order.push((t.clone(), q.into()));
                }
            }
        }
    }
    let ids: Vec<String> = order.iter().map(|o| o.1.clone()).collect();
    let ents = wd_entities(&ids, &lang, "claims|labels|descriptions")?;
    let mut out = vec![];
    for (n, (wtitle, q)) in order.iter().enumerate() {
        let e = &ents[q];
        if !is_film(e) {
            continue;
        }
        let name = label(e, &lang);
        let y = claim_year(e);
        // The article title without " (2019 film)" also counts as a name.
        let plain = wtitle.split(" (").next().unwrap_or(wtitle);
        let sc = score(title, year, &name, y).max(score(title, year, plain, y)) - n as f64 * 0.01;
        let desc = e["descriptions"][&lang]["value"].as_str().or(e["descriptions"]["en"]["value"].as_str()).unwrap_or("").to_string();
        out.push(Candidate { source: "wikidata".into(), id: q.clone(), title: name, year: y, summary: desc, thumb: None, score: sc });
    }
    Ok(out)
}

/// The plot section of a Wikipedia article (plain text), else its lead.
fn wiki_plot(lang: &str, article: &str) -> Option<String> {
    let v = wiki_api(lang, &format!("action=query&prop=extracts&explaintext=1&redirects=1&titles={}", enc(article))).ok()?;
    let text = v["query"]["pages"][0]["extract"].as_str()?.to_string();
    let mut plot = None;
    for h in plot_headings(lang) {
        for marker in [format!("\n== {h} ==\n"), format!("\n==  {h}  ==\n")] {
            if let Some(i) = text.find(&marker) {
                let rest = &text[i + marker.len()..];
                let end = rest.find("\n== ").unwrap_or(rest.len());
                let body: String = rest[..end].lines().filter(|l| !l.starts_with("===")).collect::<Vec<_>>().join("\n");
                if body.trim().len() > 80 {
                    plot = Some(body.trim().to_string());
                    break;
                }
            }
        }
        if plot.is_some() {
            break;
        }
    }
    let plot = plot.or_else(|| text.split("\n== ").next().map(|s| s.trim().to_string()))?;
    Some(shorten(&plot, 1800))
}

/// Cut at a sentence end before `max` characters.
fn shorten(s: &str, max: usize) -> String {
    let s = s.replace("\n\n", "\n");
    if s.chars().count() <= max {
        return s;
    }
    let cut: String = s.chars().take(max).collect();
    match cut.rfind(". ") {
        Some(i) if i > max / 2 => cut[..=i].to_string(),
        _ => format!("{}…", cut.trim_end()),
    }
}

/// The article's main picture, when it looks like a poster: not a logo or
/// drawing, and taller than wide (the German article on a film often shows
/// the film's logo instead).
fn wiki_poster(lang: &str, article: &str) -> Option<String> {
    let v = wiki_api(lang, &format!("action=query&prop=pageimages&piprop=original|name&pilicense=any&redirects=1&titles={}", enc(article))).ok()?;
    let p = &v["query"]["pages"][0];
    let src = p["original"]["source"].as_str()?;
    let name = p["pageimage"].as_str().unwrap_or(src);
    let (w, h) = (p["original"]["width"].as_f64().unwrap_or(0.0), p["original"]["height"].as_f64().unwrap_or(0.0));
    (!is_logo(name) && !is_logo(src) && h >= w * 1.15).then(|| src.to_string())
}

/// A Wikimedia Commons picture `file` scaled to `width` pixels.
fn commons(file: &str, width: u32) -> String {
    format!("https://commons.wikimedia.org/wiki/Special:FilePath/{}?width={width}", enc(file))
}

/// Portrait pictures (Wikidata "image", P18, on Wikimedia Commons) of people,
/// by Wikidata id. One query for everyone; if that service is down, each
/// person's entry is read instead.
pub fn wd_photos(ids: &[String]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    if ids.is_empty() {
        return out;
    }
    let values = ids.iter().map(|i| format!("wd:{i}")).collect::<Vec<_>>().join(" ");
    let q = format!("SELECT ?p ?img WHERE {{ VALUES ?p {{ {values} }} ?p wdt:P18 ?img }}");
    if let Ok(v) = get_json(&format!("https://query.wikidata.org/sparql?format=json&query={}", enc(&q)), None) {
        for b in v["results"]["bindings"].as_array().into_iter().flatten() {
            let (Some(p), Some(img)) = (b["p"]["value"].as_str(), b["img"]["value"].as_str()) else { continue };
            let id = p.rsplit('/').next().unwrap_or("").to_string();
            out.entry(id).or_insert_with(|| format!("{}?width=300", img.replacen("http://", "https://", 1)));
        }
        return out;
    }
    if let Ok(ents) = wd_entities(ids, "en", "claims") {
        for id in ids {
            if let Some(f) = claim_str(&ents[id.as_str()], "P18") {
                out.insert(id.clone(), commons(&f, 300));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ review scores
//
// Wikidata keeps scores from review sites as "review score" (P444) statements
// with "review score by" (P447), "determination method" (P459), "point in
// time" (P585) and "number of reviews/ratings" (P7887) qualifiers.

const BY_IMDB: &str = "Q37312";
const BY_ROTTEN_TOMATOES: &str = "Q105584";
const BY_METACRITIC: &str = "Q150248";
/// Preferred methods: IMDb user rating, Tomatometer, Metascore.
const PREFERRED_METHODS: &[&str] = &["Q107218751", "Q108403393", "Q106515043"];

#[derive(Debug, Clone, Default)]
pub struct ReviewScore {
    pub by: String,
    pub method: String,
    pub time: String,
    pub score: String,
    pub count: Option<u64>,
}

/// One score per site: the preferred method first, then the newest.
pub fn pick_scores(all: &[ReviewScore], rt_id: Option<&str>, mc_id: Option<&str>, imdb_id: Option<&str>) -> Value {
    let mut out = json!({});
    for (by, key, label, url) in [
        (BY_IMDB, "imdb", "votes", imdb_id.map(|i| format!("https://www.imdb.com/title/{i}/"))),
        (BY_ROTTEN_TOMATOES, "rotten_tomatoes", "reviews", rt_id.map(|i| format!("https://www.rottentomatoes.com/{i}"))),
        (BY_METACRITIC, "metacritic", "reviews", mc_id.map(|i| format!("https://www.metacritic.com/{i}"))),
    ] {
        let best = all
            .iter()
            .filter(|s| s.by == by && !s.score.is_empty())
            // Rotten Tomatoes also has an audience average ("8.8/10"); the
            // Tomatometer is the percentage.
            .filter(|s| by != BY_ROTTEN_TOMATOES || s.score.ends_with('%'))
            .max_by_key(|s| (PREFERRED_METHODS.contains(&s.method.as_str()), s.time.clone()));
        if let Some(s) = best {
            let mut v = json!({"score": s.score, "source": "Wikidata", "as_of": s.time.get(..10).unwrap_or("")});
            if let Some(n) = s.count { v["count"] = json!(n); v["count_label"] = json!(label); }
            if let Some(u) = url { v["url"] = json!(u); }
            out[key] = v;
        }
    }
    out
}

/// Review scores from a Wikidata entity read with its claims.
pub fn scores_from_entity(e: &Value) -> Value {
    let q = |c: &Value, p: &str| c["qualifiers"][p][0]["datavalue"]["value"].clone();
    let all: Vec<ReviewScore> = e["claims"]["P444"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| ReviewScore {
            by: q(c, "P447")["id"].as_str().unwrap_or("").into(),
            method: q(c, "P459")["id"].as_str().unwrap_or("").into(),
            time: q(c, "P585")["time"].as_str().unwrap_or("").trim_start_matches('+').into(),
            score: c["mainsnak"]["datavalue"]["value"].as_str().unwrap_or("").into(),
            count: q(c, "P7887")["amount"].as_str().and_then(|a| a.trim_start_matches('+').parse().ok()),
        })
        .collect();
    pick_scores(&all, claim_str(e, "P1258").as_deref(), claim_str(e, "P1712").as_deref(), claim_str(e, "P345").as_deref())
}

/// Review scores for a movie or show known by its IMDb, TMDB or TVmaze id
/// (one Wikidata query).
pub fn scores_by_ids(ids: &Value, show: bool) -> Value {
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric());
    let mut conds = vec![];
    if let Some(i) = ids["imdb"].as_str().filter(|i| i.starts_with("tt") && ok(i)) {
        conds.push(format!("{{ ?item wdt:P345 \"{i}\" }}"));
    }
    let tmdb = ids["tmdb"].as_str().map(String::from).or(ids["tmdb"].as_u64().map(|n| n.to_string()));
    if let Some(t) = tmdb.filter(|t| ok(t)) {
        conds.push(format!("{{ ?item wdt:{} \"{t}\" }}", if show { "P4983" } else { "P4947" }));
    }
    let tvmaze = ids["tvmaze"].as_str().map(String::from).or(ids["tvmaze"].as_u64().map(|n| n.to_string()));
    if let Some(t) = tvmaze.filter(|t| ok(t)) {
        conds.push(format!("{{ ?item wdt:P8600 \"{t}\" }}"));
    }
    if conds.is_empty() {
        return json!({});
    }
    let q = format!(
        "SELECT ?item ?score ?by ?method ?time ?n ?rt ?mc ?imdb WHERE {{ {{ SELECT ?item WHERE {{ {} }} LIMIT 1 }} \
         OPTIONAL {{ ?item p:P444 ?st . ?st ps:P444 ?score ; pq:P447 ?by . OPTIONAL {{ ?st pq:P459 ?method }} OPTIONAL {{ ?st pq:P585 ?time }} OPTIONAL {{ ?st pq:P7887 ?n }} }} \
         OPTIONAL {{ ?item wdt:P1258 ?rt }} OPTIONAL {{ ?item wdt:P1712 ?mc }} OPTIONAL {{ ?item wdt:P345 ?imdb }} }}",
        conds.join(" UNION ")
    );
    let Ok(v) = get_json(&format!("https://query.wikidata.org/sparql?format=json&query={}", enc(&q)), None) else { return json!({}) };
    let rows = v["results"]["bindings"].as_array().cloned().unwrap_or_default();
    let s = |b: &Value, k: &str| b[k]["value"].as_str().unwrap_or("").to_string();
    let tail = |x: String| x.rsplit('/').next().unwrap_or("").to_string();
    let all: Vec<ReviewScore> = rows
        .iter()
        .filter(|b| b.get("score").is_some())
        .map(|b| ReviewScore { by: tail(s(b, "by")), method: tail(s(b, "method")), time: s(b, "time"), score: s(b, "score"), count: s(b, "n").parse::<f64>().ok().map(|n| n as u64) })
        .collect();
    let first = |k: &str| rows.iter().map(|b| s(b, k)).find(|x| !x.is_empty());
    let imdb = ids["imdb"].as_str().map(String::from).or(first("imdb"));
    pick_scores(&all, first("rt").as_deref(), first("mc").as_deref(), imdb.as_deref())
}

/// The IMDb rating as stored from IMDb's own ratings file.
pub fn imdb_score(rating: f32, votes: u64, imdb_id: &str) -> Value {
    json!({"score": format!("{rating:.1}/10"), "count": votes, "count_label": "votes", "source": "IMDb ratings file",
        "url": format!("https://www.imdb.com/title/{imdb_id}/"), "as_of": crate::db::today()})
}

/// IMDb publishes its ratings as a free file for personal use, refreshed daily:
/// tconst, averageRating, numVotes (tab separated, gzip).
pub const IMDB_RATINGS_URL: &str = "https://datasets.imdbws.com/title.ratings.tsv.gz";

/// Downloads the IMDb ratings file (about 9 MB) to `dest`.
pub fn download_imdb_ratings(dest: &Path) -> Result<(), String> {
    let resp = ureq::AgentBuilder::new().timeout(Duration::from_secs(300)).user_agent(UA).build()
        .get(IMDB_RATINGS_URL).call().map_err(|e| format!("could not download the IMDb ratings file: {e}"))?;
    let mut bytes = vec![];
    resp.into_reader().take(200_000_000).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() < 1000 || bytes[0] != 0x1f || bytes[1] != 0x8b {
        return Err("the IMDb ratings file looks wrong (not gzip)".into());
    }
    if let Some(d) = dest.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = dest.with_extension("part");
    std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, dest)).map_err(|e| e.to_string())
}

/// Reads the IMDb ratings file and returns the ratings of the wanted ids.
pub fn read_imdb_ratings(file: &Path, wanted: &std::collections::HashSet<String>) -> Result<HashMap<String, (f32, u64)>, String> {
    use std::io::BufRead;
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let r = std::io::BufReader::new(flate2::read::GzDecoder::new(f));
    let mut out = HashMap::new();
    for line in r.lines() {
        let line = line.map_err(|e| format!("the IMDb ratings file is damaged ({e})"))?;
        let mut p = line.split('\t');
        let (Some(id), Some(avg), Some(n)) = (p.next(), p.next(), p.next()) else { continue };
        if wanted.contains(id) {
            if let (Ok(a), Ok(n)) = (avg.parse(), n.parse()) {
                out.insert(id.to_string(), (a, n));
                if out.len() == wanted.len() {
                    break;
                }
            }
        }
    }
    Ok(out)
}

fn wiki_fetch(settings: &Settings, q: &str) -> Result<Details, String> {
    let lang = lang_of(settings);
    let ents = wd_entities(&[q.to_string()], &lang, "claims|labels|descriptions|sitelinks")?;
    let e = ents.get(q).cloned().ok_or("Wikidata has no such entry")?;
    let local_article = e["sitelinks"][format!("{lang}wiki")]["title"].as_str().map(String::from);
    let en_article = e["sitelinks"]["enwiki"]["title"].as_str().map(String::from);
    // Names of everyone and everything the item points to, in one go.
    let rating_prop = if lang == "de" { "P1981" } else { "P1657" };
    let mut refs: Vec<String> = vec![];
    for p in ["P57", "P58", "P161", "P136", "P272", "P162", "P495", rating_prop] {
        refs.extend(claim_ids(&e, p));
    }
    // Character names are qualifiers (P453) on cast members.
    let cast_claims = e["claims"]["P161"].as_array().cloned().unwrap_or_default();
    for c in &cast_claims {
        if let Some(qs) = c["qualifiers"]["P453"].as_array() {
            refs.extend(qs.iter().filter_map(|q| q["datavalue"]["value"]["id"].as_str().map(String::from)));
        }
    }
    refs.sort();
    refs.dedup();
    let names = if refs.is_empty() { json!({}) } else { wd_entities(&refs, &lang, "labels")? };
    let lbl = |id: &str| label(&names[id], &lang);
    let list = |p: &str, max: usize| -> Vec<String> { claim_ids(&e, p).iter().map(|i| lbl(i)).filter(|s| !s.is_empty()).take(max).collect() };
    let cast_ids: Vec<String> = cast_claims.iter().take(20).filter_map(|c| c["mainsnak"]["datavalue"]["value"]["id"].as_str().map(String::from)).collect();
    let photos = wd_photos(&cast_ids);
    let cast: Vec<Value> = cast_claims.iter().take(20).filter_map(|c| {
        let id = c["mainsnak"]["datavalue"]["value"]["id"].as_str()?;
        let role = c["qualifiers"]["P453"][0]["datavalue"]["value"]["id"].as_str().map(|r| lbl(r)).unwrap_or_default();
        Some(json!({"name": lbl(id), "role": role, "photo": photos.get(id), "person_id": format!("wd-{id}")}))
    }).filter(|c| c["name"] != "").collect();
    let mut genres = list("P136", 6);
    for g in &mut genres {
        // Wikidata genre names are often "drama film" / "Filmdrama": keep them short.
        for suffix in [" film", " movie"] {
            if g.ends_with(suffix) && g.len() > suffix.len() + 2 {
                g.truncate(g.len() - suffix.len());
            }
        }
        if let Some(c) = g.get(0..1) {
            *g = c.to_uppercase() + &g[1..];
        }
    }
    let (plot, poster) = {
        let mut plot = None;
        // Wikidata's "film poster" (P3383) is a real poster; the articles' pictures next.
        let mut poster = claim_str(&e, "P3383").filter(|f| !f.to_lowercase().contains("logo")).map(|f| commons(&f, 600));
        for (l, art) in [(lang.as_str(), local_article.as_deref()), ("en", en_article.as_deref())] {
            let Some(art) = art else { continue };
            if plot.is_none() {
                plot = wiki_plot(l, art);
            }
            if poster.is_none() {
                poster = wiki_poster(l, art);
            }
        }
        if poster.is_none() {
            poster = claim_str(&e, "P18").filter(|f| !is_logo(f)).map(|f| commons(&f, 600));
        }
        (plot, poster)
    };
    let mut ids = json!({"wikidata": q});
    if let Some(i) = claim_str(&e, "P345") { ids["imdb"] = json!(i); }
    if let Some(i) = claim_str(&e, "P4947") { ids["tmdb"] = json!(i); }
    if let Some(a) = local_article.as_ref().or(en_article.as_ref()) {
        let l = if local_article.is_some() { lang.as_str() } else { "en" };
        ids["wikipedia"] = json!(format!("https://{l}.wikipedia.org/wiki/{}", enc(&a.replace(' ', "_"))));
    }
    let mut meta = json!({
        "title": label(&e, &lang),
        "summary": plot.unwrap_or_default(),
        "genres": genres,
        "studio": list("P272", 1).first().cloned().unwrap_or_default(),
        "content_rating": claim_ids(&e, rating_prop).first().map(|r| lbl(r)).unwrap_or_default(),
        "extra": {"cast": cast, "directors": list("P57", 4), "writers": list("P58", 6), "producers": list("P162", 6), "countries": list("P495", 3), "ids": ids, "metadata_source": "wikidata", "metadata_id": q,
            "metadata_language": if local_article.is_some() { lang.as_str() } else { "en" }},
    });
    if let Some(y) = claim_year(&e) { meta["year"] = json!(y); }
    meta["extra"]["ratings"] = scores_from_entity(&e);
    Ok(Details { meta, poster_url: poster, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pictures() {
        assert_eq!(image_type(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(image_type(b"\x89PNG\r\n"), Some("image/png"));
        assert_eq!(image_type(b"<?xml version=\"1.0\"?><svg"), None);
        assert_eq!(image_type(b"<!DOCTYPE html>"), None);
        assert!(is_logo("Pulp_Fiction_Logo.svg") && is_logo("x.SVG") && !is_logo("Pulp_Fiction_(1994)_poster.jpg"));
        assert_eq!(
            raster_version("https://upload.wikimedia.org/wikipedia/commons/d/d0/Pulp_Fiction_Logo.svg?utm_source=de.wikipedia.org").as_deref(),
            Some("https://upload.wikimedia.org/wikipedia/commons/thumb/d/d0/Pulp_Fiction_Logo.svg/500px-Pulp_Fiction_Logo.svg.png")
        );
        assert_eq!(raster_version("https://commons.wikimedia.org/wiki/Special:FilePath/A.svg").as_deref(), Some("https://commons.wikimedia.org/wiki/Special:FilePath/A.svg?width=600"));
        assert_eq!(raster_version("https://image.tmdb.org/t/p/w500/x.jpg"), None);
    }
    #[test]
    fn matching() {
        assert_eq!(normalize("The Matrix"), "matrix");
        assert_eq!(normalize("Fast & Furious"), "fast and furious");
        assert!(score("The Matrix", Some(1999), "Matrix", Some(1999)) > 1.1);
        assert!(score("The Matrix", Some(1999), "The Matrix Reloaded", Some(2003)) < 0.85);
        assert!(score("Dune", Some(2021), "Dune", Some(1984)) < 0.85);
        assert_eq!(shorten("One. Two. Three.", 100), "One. Two. Three.");
        assert_eq!(strip_html("<p>Hi &amp; bye</p>"), "Hi & bye");
    }
}
