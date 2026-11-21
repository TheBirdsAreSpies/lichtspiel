//! TMDB (themoviedb.org), when an API key is set: search, details, trailers and people.

use super::*;

pub(super) fn tmdb_get(settings: &Settings, path: &str) -> Result<Value, String> {
    let key = settings.tmdb_api_key.trim();
    let sep = if path.contains('?') { '&' } else { '?' };
    if key.starts_with("eyJ") {
        get_json(&format!("{TMDB}{path}"), Some(key))
    } else {
        get_json(&format!("{TMDB}{path}{sep}api_key={}", enc(key)), None)
    }
}

/// What TMDB knows about a person: biography, birth, more photos and everything they appeared in.
/// `key` is our person key (tmdb-31 …); for people from other sources the name is searched.
pub fn person_details(settings: &Settings, key: &str, name: &str) -> Result<Value, String> {
    if settings.tmdb_api_key.trim().is_empty() {
        return Err("Add a TMDB API key in the settings to load details about people".into());
    }
    let lang = lang_of(settings);
    let id = match key.strip_prefix("tmdb-").and_then(|n| n.parse::<u64>().ok()) {
        Some(n) => n,
        None => {
            if name.trim().is_empty() { return Err("Unknown person".into()); }
            let r = tmdb_get(settings, &format!("/search/person?query={}&language={lang}", enc(name)))?;
            r["results"][0]["id"].as_u64().ok_or("No person of that name found at TMDB")?
        }
    };
    let v = tmdb_get(settings, &format!("/person/{id}?language={lang}&append_to_response=images,combined_credits,external_ids"))?;
    let mut bio = v["biography"].as_str().unwrap_or("").trim().to_string();
    if bio.is_empty() && lang != "en" {
        bio = tmdb_get(settings, &format!("/person/{id}?language=en")).ok().and_then(|e| e["biography"].as_str().map(|s| s.trim().to_string())).unwrap_or_default();
    }
    let photos: Vec<Value> = v["images"]["profiles"].as_array().cloned().unwrap_or_default().iter().take(30)
        .filter_map(|p| Some(json!({"url": tmdb_img(&p["file_path"], "w342")?, "full": tmdb_img(&p["file_path"], "h632")?}))).collect();
    // One line per movie or show; a person can be actor and director in the same one.
    let mut credits: Vec<Value> = vec![];
    let mut add = |c: &Value, role: String| {
        let kind = if c["media_type"] == "tv" { "show" } else { "movie" };
        let tid = c["id"].as_u64().unwrap_or(0);
        if c["genre_ids"].as_array().is_some_and(|g| g.iter().any(|x| x == 10767 || x == 10763)) { return; } // talk shows, news
        if let Some(e) = credits.iter_mut().find(|e| e["tmdb_id"] == tid && e["type"] == kind) {
            let old = e["role"].as_str().unwrap_or("").to_string();
            if !role.is_empty() && !old.split(" · ").any(|r| r == role) { e["role"] = json!(if old.is_empty() { role } else { format!("{old} · {role}") }); }
            return;
        }
        let date = c["release_date"].as_str().or(c["first_air_date"].as_str()).unwrap_or("");
        credits.push(json!({"tmdb_id": tid, "type": kind, "title": c["title"].as_str().or(c["name"].as_str()).unwrap_or(""),
            "year": date.get(..4).and_then(|y| y.parse::<u32>().ok()), "role": role, "poster": tmdb_img(&c["poster_path"], "w185"),
            "votes": c["vote_count"]}));
    };
    for c in v["combined_credits"]["cast"].as_array().cloned().unwrap_or_default() {
        add(&c, c["character"].as_str().unwrap_or("").to_string());
    }
    for c in v["combined_credits"]["crew"].as_array().cloned().unwrap_or_default() {
        let job = c["job"].as_str().unwrap_or("");
        if matches!(job, "Director" | "Writer" | "Screenplay" | "Creator" | "Producer" | "Executive Producer") {
            add(&c, job.to_string());
        }
    }
    credits.sort_by(|a, b| b["year"].as_u64().unwrap_or(9999).cmp(&a["year"].as_u64().unwrap_or(9999)));
    credits.truncate(300);
    Ok(json!({"tmdb_id": id, "name": v["name"], "biography": bio, "birthday": v["birthday"], "deathday": v["deathday"],
        "place_of_birth": v["place_of_birth"], "department": v["known_for_department"], "imdb_id": v["external_ids"]["imdb_id"],
        "homepage": v["homepage"], "photos": photos, "credits": credits}))
}

fn tmdb_img(path: &Value, size: &str) -> Option<String> {
    path.as_str().filter(|p| !p.is_empty()).map(|p| format!("{TMDB_IMG}/{size}{p}"))
}

pub(super) fn tmdb_search(settings: &Settings, kind: ItemKind, title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
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

/// The languages asked for with a title's videos: the library's language,
/// then English (when "Also look for English trailers" is on), then videos without a language.
pub(super) fn video_langs(lang: &str, english: bool) -> String {
    let short = lang.split(['-', '_']).next().unwrap_or("en").to_lowercase();
    if short == "en" { "en,null".into() } else if english { format!("{short},en,null") } else { format!("{short},null") }
}

/// Order of the kinds of online videos (trailers first).
pub const VIDEO_KINDS: &[&str] = &["Trailer", "Teaser", "Clip", "Featurette", "Behind the Scenes"];

/// Online trailers and other videos of a movie or show: YouTube only (the
/// web app plays them in place), red band trailers left out (unless
/// "Allow red band trailers" is on), trailers first, the
/// library's language before English, official ones before the rest, newest first.
pub fn tmdb_trailers(results: &Value, lang: &str, red_band: bool) -> Vec<Value> {
    let short = lang.split(['-', '_']).next().unwrap_or("en").to_lowercase();
    let mut v: Vec<&Value> = results.as_array().map(|a| a.iter().collect()).unwrap_or_default();
    v.retain(|x| x["site"] == "YouTube" && x["key"].as_str().is_some_and(|k| !k.is_empty())
        && VIDEO_KINDS.contains(&x["type"].as_str().unwrap_or(""))
        && (red_band || !x["name"].as_str().unwrap_or("").to_lowercase().contains("red band")));
    let rank = |x: &Value| {
        let kind = VIDEO_KINDS.iter().position(|k| *k == x["type"].as_str().unwrap_or("")).unwrap_or(9);
        let l = x["iso_639_1"].as_str().unwrap_or("");
        let lang_rank = if l == short { 0 } else if l == "en" { 1 } else { 2 };
        (kind, lang_rank, !x["official"].as_bool().unwrap_or(false))
    };
    v.sort_by(|a, b| rank(a).cmp(&rank(b)).then(b["published_at"].as_str().unwrap_or("").cmp(a["published_at"].as_str().unwrap_or(""))));
    v.into_iter().take(12).map(|x| json!({
        "name": x["name"], "type": x["type"], "site": "YouTube", "key": x["key"], "language": x["iso_639_1"],
        "official": x["official"].as_bool().unwrap_or(false), "source": "tmdb",
    })).collect()
}

pub(super) fn tmdb_fetch(settings: &Settings, kind: ItemKind, id: &str, seasons_wanted: &[u32]) -> Result<Details, String> {
    let lang = lang_of(settings);
    let reg = region(&lang);
    let show = kind == ItemKind::Show;
    let v = if show {
        tmdb_get(settings, &format!("/tv/{id}?language={lang}&append_to_response=aggregate_credits,content_ratings,external_ids,videos&include_video_language={}", video_langs(&lang, settings.trailers_english)))?
    } else {
        tmdb_get(settings, &format!("/movie/{id}?language={lang}&append_to_response=credits,release_dates,external_ids,videos&include_video_language={}", video_langs(&lang, settings.trailers_english)))?
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
            "url": format!("https://www.themoviedb.org/{}/{id}", if show { "tv" } else { "movie" }), "as_of": crate::data::db::today()});
    }
    extra["ratings"] = ratings;
    extra["trailers"] = json!(tmdb_trailers(&v["videos"]["results"], &lang, settings.trailers_red_band));
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
    // No poster in this language: TMDB often has one in English or without text.
    if d.poster_url.is_none() || d.art_url.is_none() {
        if let Ok(im) = tmdb_get(settings, &format!("/{}/{id}/images?include_image_language=en,null", if show { "tv" } else { "movie" })) {
            if d.poster_url.is_none() { d.poster_url = tmdb_img(&im["posters"][0]["file_path"], "w500"); }
            if d.art_url.is_none() { d.art_url = tmdb_img(&im["backdrops"][0]["file_path"], "w1280"); }
        }
    }
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
