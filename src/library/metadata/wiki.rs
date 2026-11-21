//! Wikidata and Wikipedia for movies without a TMDB key: search, plot, cast, crew, genres and posters.

use super::*;

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

pub(super) fn wiki_api(lang: &str, query: &str) -> Result<Value, String> {
    get_json(&format!("https://{lang}.wikipedia.org/w/api.php?format=json&formatversion=2&{query}"), None)
}

pub(super) fn wd_entities(ids: &[String], lang: &str, props: &str) -> Result<Value, String> {
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

pub(super) fn label(e: &Value, lang: &str) -> String {
    e["labels"][lang]["value"].as_str().or(e["labels"]["en"]["value"].as_str()).unwrap_or("").to_string()
}

fn claim_ids(e: &Value, prop: &str) -> Vec<String> {
    e["claims"][prop].as_array().map(|a| a.iter().filter_map(|c| c["mainsnak"]["datavalue"]["value"]["id"].as_str().map(String::from)).collect()).unwrap_or_default()
}

pub(super) fn claim_str(e: &Value, prop: &str) -> Option<String> {
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

pub(super) fn wiki_search(settings: &Settings, title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
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
pub(super) fn shorten(s: &str, max: usize) -> String {
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
pub(super) fn commons(file: &str, width: u32) -> String {
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



/// Wikidata genre names are often "drama film", "Buddy-Film" or "Buddy-Cop-Film":
/// keep them short ("Drama", "Buddy", "Buddy-Cop"), with a capital first letter.
pub fn clean_genre(g: &str) -> String {
    let mut g = g.trim().to_string();
    for suffix in [" film", "-film", " movie", "-movie"] {
        let l = g.to_lowercase();
        if l.ends_with(suffix) && g.len() > suffix.len() + 1 {
            g.truncate(g.len() - suffix.len());
            break;
        }
    }
    let g = g.trim_matches(|c: char| c == '-' || c.is_whitespace());
    let mut it = g.chars();
    match it.next() {
        Some(c) => c.to_uppercase().collect::<String>() + it.as_str(),
        None => String::new(),
    }
}

pub(super) fn wiki_fetch(settings: &Settings, q: &str) -> Result<Details, String> {
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
    let genres: Vec<String> = list("P136", 6).iter().map(|g| clean_genre(g)).filter(|g| !g.is_empty()).collect();
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
    // Wikidata's "YouTube video ID" (P1651) on a film is usually its trailer.
    if let Some(k) = claim_str(&e, "P1651").filter(|k| !k.is_empty()) {
        meta["extra"]["trailers"] = json!([{"name": "Trailer", "type": "Trailer", "site": "YouTube", "key": k, "language": null, "official": false, "source": "wikidata"}]);
    }
    Ok(Details { meta, poster_url: poster, ..Default::default() })
}
