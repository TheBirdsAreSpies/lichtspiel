//! What every source shares: the result types, HTTP, pictures, and comparing titles.

use super::*;

pub(super) const UA: &str = "Lichtspiel/0.1 (self-hosted personal streaming server; metadata lookup)";
pub(super) const TMDB: &str = "https://api.themoviedb.org/3";
pub(super) const TMDB_IMG: &str = "https://image.tmdb.org/t/p";
pub(super) const TVMAZE: &str = "https://api.tvmaze.com";

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

pub(super) fn get_json(url: &str, bearer: Option<&str>) -> Result<Value, String> {
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
pub(super) fn raster_version(url: &str) -> Option<String> {
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
pub(super) fn is_logo(file: &str) -> bool {
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

pub(super) fn strip_html(s: &str) -> String {
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

pub(super) fn year_of(date: &str) -> Option<u32> {
    let d = date.trim_start_matches('+');
    d.get(0..4).and_then(|y| y.parse().ok()).filter(|y| *y > 1800)
}

/// Lower-case letters and digits only, articles dropped, "&" → "and". Umlauts and accents are
/// folded (ä = ae = a-with-dots), apostrophes disappear ("Angel's" = "Angels").
pub fn normalize(t: &str) -> String {
    let t = t.to_lowercase().replace('&', " and ");
    let mut cleaned = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '\'' | '’' | '`' | '´' | '‘' => {}
            'ä' => cleaned.push_str("ae"),
            'ö' => cleaned.push_str("oe"),
            'ü' => cleaned.push_str("ue"),
            'ß' => cleaned.push_str("ss"),
            'à' | 'á' | 'â' | 'ã' | 'å' => cleaned.push('a'),
            'è' | 'é' | 'ê' | 'ë' => cleaned.push('e'),
            'ì' | 'í' | 'î' | 'ï' => cleaned.push('i'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ø' => cleaned.push('o'),
            'ù' | 'ú' | 'û' => cleaned.push('u'),
            'ç' => cleaned.push('c'),
            'ñ' => cleaned.push('n'),
            'ý' | 'ÿ' => cleaned.push('y'),
            c if c.is_alphanumeric() => cleaned.push(c),
            _ => cleaned.push(' '),
        }
    }
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let skip = matches!(words.first(), Some(&("the" | "a" | "an" | "der" | "die" | "das" | "ein" | "eine" | "le" | "la" | "les" | "el" | "il"))) && words.len() > 1;
    words[usize::from(skip)..].join(" ")
}

/// At most one letter different (a typo, a missing letter) in words of five letters or more.
fn near(a: &str, b: &str) -> bool {
    if a == b { return true; }
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if x.len() < 5 || y.len() < 5 || x.len().abs_diff(y.len()) > 1 { return false; }
    let (mut i, mut j, mut diff) = (0, 0, 0);
    while i < x.len() && j < y.len() {
        if x[i] == y[j] { i += 1; j += 1; continue; }
        diff += 1;
        if diff > 1 { return false; }
        if x.len() > y.len() { i += 1; } else if y.len() > x.len() { j += 1; } else { i += 1; j += 1; }
    }
    diff + (x.len() - i) + (y.len() - j) <= 1
}

/// Word overlap of two normalized titles, 0 … 1 (words one letter apart count).
fn overlap(a: &str, b: &str) -> f64 {
    let wa: Vec<&str> = a.split(' ').filter(|w| !w.is_empty()).collect();
    let wb: Vec<&str> = b.split(' ').filter(|w| !w.is_empty()).collect();
    let mut used = vec![false; wb.len()];
    let mut hit = 0usize;
    for w in &wa {
        if let Some(k) = wb.iter().enumerate().position(|(k, v)| !used[k] && near(w, v)) { used[k] = true; hit += 1; }
    }
    let union = (wa.len() + wb.len() - hit).max(1);
    hit as f64 / union as f64
}

/// The title without its subtitle: "Die Abbotts - Wenn Hass die Liebe toetet" → "die abbotts".
fn main_part(t: &str) -> &str {
    let cut = [" - ", " – ", " — ", ": "].iter().filter_map(|p| t.find(p)).min();
    match cut { Some(i) if i >= 3 => t[..i].trim(), _ => t.trim() }
}

/// How well a found title/year fits what the file name said.
pub fn score(want_title: &str, want_year: Option<u32>, title: &str, year: Option<u32>) -> f64 {
    let a = normalize(want_title);
    let b = normalize(title);
    let sim = if a == b {
        1.0
    } else {
        let mut best = overlap(&a, &b) * 0.9;
        // One of them is the other's main title without the subtitle ("Die Abbotts" / "Die Abbotts - Wenn …").
        let (ma, mb) = (normalize(main_part(want_title)), normalize(main_part(title)));
        if !ma.is_empty() && !mb.is_empty() {
            if ma == b || mb == a || ma == mb { best = best.max(0.93); }
            else { best = best.max(overlap(&ma, &mb) * 0.9); }
        }
        best
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

pub(super) fn region(lang: &str) -> &'static str {
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

pub(super) fn lang_of(settings: &Settings) -> String {
    let l = settings.metadata_language.trim().to_lowercase();
    if l.len() == 2 && l.chars().all(|c| c.is_ascii_lowercase()) { l } else { "en".into() }
}
