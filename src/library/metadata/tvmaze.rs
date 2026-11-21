//! TVmaze for shows without a TMDB key: search, details and every episode.

use super::*;

pub(super) fn tvmaze_search(title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
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

pub(super) fn tvmaze_fetch(settings: &Settings, id: &str) -> Result<Details, String> {
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
