//! Finding a title: searching every source with title variants, ids in names, and fetching the chosen match.

use super::*;

/// One lookup with exactly this title.
fn search_one(settings: &Settings, kind: ItemKind, title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
    let free = |settings: &Settings| if kind == ItemKind::Show { tvmaze_search(title, year) } else { wiki_search(settings, title, year) };
    if settings.tmdb_api_key.trim().is_empty() {
        return free(settings);
    }
    // What TMDB does not know (or cannot answer) is looked for at TVmaze (shows) or Wikidata (films).
    match tmdb_search(settings, kind, title, year) {
        Ok(found) if !found.is_empty() => Ok(found),
        tmdb => match free(settings) {
            Ok(found) if !found.is_empty() => Ok(found),
            other => tmdb.and(other),
        },
    }
}

/// Spellings of a title that are worth trying besides the typed one: "ae/oe/ue" as umlauts (and back), "ss" as ß
/// (and back), without accents, and without punctuation. File names often lack the umlauts the title has.
/// "Affe im Menschen, Der" -> "Der Affe im Menschen" (library-style names put the article last).
pub fn move_article_front(title: &str) -> Option<String> {
    const ARTICLES: [&str; 16] = ["der", "die", "das", "ein", "eine", "the", "a", "an", "le", "la", "les", "il", "el", "un", "une", "los"];
    let t = title.trim();
    // An optional year in brackets may follow: "Affe im Menschen, Der (1990)".
    let (head, tail) = match t.rfind('(') {
        Some(i) if t.ends_with(')') && t[i + 1..t.len() - 1].chars().all(|c| c.is_ascii_digit()) => (t[..i].trim_end(), &t[i..]),
        _ => (t, ""),
    };
    let (rest, art) = head.rsplit_once(',')?;
    let art = art.trim();
    if rest.trim().is_empty() || !ARTICLES.contains(&art.to_lowercase().as_str()) {
        return None;
    }
    let out = format!("{} {}", art, rest.trim());
    Some(if tail.is_empty() { out } else { format!("{out} {tail}") })
}

pub(super) fn title_variants(title: &str) -> Vec<String> {
    if let Some(front) = move_article_front(title) {
        let mut v = title_variants_plain(&front);
        let orig = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if !v.iter().any(|x| x.eq_ignore_ascii_case(&orig)) {
            v.insert(1.min(v.len()), orig);
        }
        return v;
    }
    title_variants_plain(title)
}

fn title_variants_plain(title: &str) -> Vec<String> {
    fn rep(s: &str, pairs: &[(&str, &str)]) -> String {
        let mut out = s.to_string();
        for (a, b) in pairs {
            out = out.replace(a, b).replace(&a.to_uppercase(), &b.to_uppercase());
            // "Ae" at the start of a word
            let mut cap = a.chars();
            if let Some(f) = cap.next() {
                let a2 = format!("{}{}", f.to_uppercase(), cap.as_str());
                let mut bc = b.chars();
                let b2 = bc.next().map(|f| format!("{}{}", f.to_uppercase(), bc.as_str())).unwrap_or_default();
                out = out.replace(&a2, &b2);
            }
        }
        out
    }
    let mut v: Vec<String> = Vec::new();
    let mut add = |x: String| {
        let x = x.split_whitespace().collect::<Vec<_>>().join(" ");
        if !x.is_empty() && !v.iter().any(|y| y.eq_ignore_ascii_case(&x)) {
            v.push(x);
        }
    };
    add(title.to_string());
    add(rep(title, &[("ae", "ä"), ("oe", "ö"), ("ue", "ü")]));
    add(rep(title, &[("ae", "ä"), ("oe", "ö"), ("ue", "ü"), ("ss", "ß")]));
    add(rep(title, &[("ä", "ae"), ("ö", "oe"), ("ü", "ue"), ("ß", "ss")]));
    add(rep(title, &[("ä", "a"), ("ö", "o"), ("ü", "u"), ("ß", "ss"), ("é", "e"), ("è", "e"), ("à", "a"), ("ô", "o"), ("î", "i"), ("ç", "c")]));
    add(title.chars().map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' }).collect());
    v
}

/// Looks the title up online. Tries other spellings (umlauts, accents, punctuation) as well, and when nothing
/// turns up, the year is dropped and then the last words of the title one by one.
pub fn search(settings: &Settings, kind: ItemKind, title: &str, year: Option<u32>) -> Result<Vec<Candidate>, String> {
    if let Some((src, id)) = typed_id(title) {
        return search_by_id(settings, kind, src, &id);
    }
    let mut list: Vec<Candidate> = Vec::new();
    let mut first_err: Option<String> = None;
    let mut run = |t: &str, y: Option<u32>, list: &mut Vec<Candidate>| {
        match search_one(settings, kind, t, y) {
            Ok(found) => {
                for c in found {
                    if !list.iter().any(|x| x.source == c.source && x.id == c.id) {
                        list.push(c);
                    }
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    };
    for t in title_variants(title).iter().take(5) {
        run(t, year, &mut list);
    }
    if list.is_empty() && year.is_some() {
        for t in title_variants(title).iter().take(5) {
            run(t, None, &mut list);
        }
    }
    if list.is_empty() {
        // Fewer and fewer words from the front ("Arsen und Spitzenhaeubchen" -> "Arsen und" -> "Arsen").
        let words: Vec<&str> = title.split_whitespace().collect();
        for n in (1..words.len()).rev() {
            let part = words[..n].join(" ");
            for t in title_variants(&part).iter().take(3) {
                run(t, None, &mut list);
            }
            if !list.is_empty() {
                break;
            }
        }
    }
    // TMDB has nothing (or is not reachable): TVmaze for shows and Wikidata for movies may still know it.
    if list.is_empty() && !settings.tmdb_api_key.trim().is_empty() {
        let mut free = settings.clone();
        free.tmdb_api_key.clear();
        if let Ok(found) = search(&free, kind, title, year) {
            list = found;
        }
    }
    if list.is_empty() {
        if let Some(e) = first_err {
            return Err(e);
        }
    }
    list.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    Ok(list)
}

/// An id typed into the search box: "tt0068646", "imdb:tt0068646", "tmdb:603", "tmdb-603".
/// Also accepts the address of an IMDb / TMDB page.
pub(super) fn typed_id(s: &str) -> Option<(&'static str, String)> {
    let s = s.trim().to_lowercase();
    let digits = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit());
    let imdb = |x: &str| x.strip_prefix("tt").is_some_and(|d| d.len() >= 6 && digits(d));
    // From a page address: …/title/tt0068646/ or …/movie/603-the-matrix
    // (also with a language in between: imdb.com/de/title/tt…)
    if let Some(p) = s.contains("imdb.com/").then(|| s.split("/title/").nth(1)).flatten() {
        let id: String = p.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        return imdb(&id).then_some(("imdb", id));
    }
    for part in ["themoviedb.org/movie/", "themoviedb.org/tv/"] {
        if let Some(p) = s.split(part).nth(1) {
            let id: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
            return digits(&id).then_some(("tmdb", id));
        }
    }
    let rest = s.strip_prefix("imdb").map(|r| ("imdb", r)).or_else(|| s.strip_prefix("tmdb").map(|r| ("tmdb", r)));
    if let Some((src, r)) = rest {
        let id = r.trim_start_matches([':', '-', ' ', '=']).trim().to_string();
        return match src {
            "imdb" if imdb(&id) => Some(("imdb", id)),
            "tmdb" if digits(&id) => Some(("tmdb", id)),
            _ => None,
        };
    }
    // A bare number stays a title ("1917", "300"); a TMDB id needs the "tmdb" prefix.
    imdb(&s).then_some(("imdb", s))
}



/// The one entry an id points to, shaped like a search hit.
fn search_by_id(settings: &Settings, kind: ItemKind, src: &str, id: &str) -> Result<Vec<Candidate>, String> {
    let free = || {
        let mut f = settings.clone();
        f.tmdb_api_key.clear();
        f
    };
    // Not on TMDB: TVmaze (shows) or Wikidata (movies) may still have it.
    let found = resolve_id(settings, kind, src, id).or_else(|e| if settings.tmdb_api_key.trim().is_empty() { Err(e) } else { resolve_id(&free(), kind, src, id).map_err(|_| e) });
    let (source, id) = match found {
        Ok(r) => r,
        Err(e) => {
            // Maybe the id is of the other kind (a series for a movie or the other way round): say so.
            let (other, name, mine) = if kind == ItemKind::Show { (ItemKind::Movie, "movie", "show") } else { (ItemKind::Show, "show", "movie") };
            return Err(if resolve_id(settings, other, src, id).is_ok() {
                format!("{src}-{id} is a {name}, but this item is a {mine}")
            } else {
                e
            });
        }
    };
    let d = fetch(settings, kind, &source, &id, &[])?;
    let m = &d.meta;
    Ok(vec![Candidate {
        source,
        id,
        title: m["title"].as_str().unwrap_or("").to_string(),
        year: m["year"].as_u64().map(|y| y as u32),
        summary: m["summary"].as_str().unwrap_or("").to_string(),
        thumb: d.poster_url.clone(),
        score: 1.2,
    }])
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
