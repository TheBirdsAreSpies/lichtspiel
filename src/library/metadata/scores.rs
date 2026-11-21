//! Review scores (IMDb, Rotten Tomatoes, Metacritic) from Wikidata, and IMDb's own ratings file.

use super::*;

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
    // Rotten Tomatoes' audience score ("8.8/10", "4.4/5") next to the Tomatometer.
    let aud = all.iter().filter(|s| s.by == BY_ROTTEN_TOMATOES && !s.score.is_empty() && !s.score.ends_with('%'))
        .max_by_key(|s| (PREFERRED_METHODS.contains(&s.method.as_str()), s.time.clone()));
    if let Some(s) = aud {
        let mut v = json!({"score": s.score, "source": "Wikidata", "as_of": s.time.get(..10).unwrap_or("")});
        if let Some(n) = s.count { v["count"] = json!(n); v["count_label"] = json!("ratings"); }
        if let Some(i) = rt_id { v["url"] = json!(format!("https://www.rottentomatoes.com/{i}")); }
        out["rotten_tomatoes_audience"] = v;
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
        "url": format!("https://www.imdb.com/title/{imdb_id}/"), "as_of": crate::data::db::today()})
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
