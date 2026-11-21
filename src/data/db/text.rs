//! One spelling for age ratings and genres, and titles for sorting (without leading articles).

use super::*;

/// One spelling for age ratings: "12", "FSK12", "DE-12", "ab 12" all become "FSK 12".
pub fn normalize_age_rating(r: &str) -> String {
    let t = r.trim();
    let lower = t.to_lowercase();
    let rest = ["fsk", "de-", "de:", "germany:", "ab", "rated"].iter().find_map(|p| lower.strip_prefix(p)).unwrap_or(&lower);
    let rest = rest.trim_matches(|c: char| c == ' ' || c == '-' || c == '_' || c == ':');
    let rest = rest.strip_suffix("jahren").or_else(|| rest.strip_suffix(" j.")).or_else(|| rest.strip_suffix('j')).unwrap_or(rest).trim();
    match rest {
        "0" | "6" | "12" | "16" | "18" => format!("FSK {rest}"),
        _ => t.to_string(),
    }
}

/// One spelling for genres: "Horrorfilm" → "Horror", "Familienfilm" → "Familie", "Sci-Fi" → "Science Fiction".
pub fn normalize_genre(g: &str) -> String {
    let t = g.trim();
    let lower = t.to_lowercase();
    let mapped = match lower.as_str() {
        "sci-fi" | "scifi" | "science-fiction" | "science fiction" | "sci-fi & fantasy" => Some("Science Fiction"),
        "familienfilm" | "familien" | "family" => Some("Familie"),
        "kriegsfilm" => Some("Krieg"),
        "liebesfilm" | "liebesfilme" | "romanze" => Some("Liebesfilm"),
        _ => None,
    };
    if let Some(m) = mapped { return if lower == "family" || lower == "sci-fi & fantasy" { t.to_string() } else { m.to_string() }; }
    for suffix in ["film", "filme"] {
        if let Some(stem) = lower.strip_suffix(suffix) {
            // "Buddy-Film" → "Buddy" (not "Buddy-"), "Buddy-Cop-Film" → "Buddy-Cop".
            let stem = stem.trim_end_matches('-');
            if stem.chars().count() >= 4 && !stem.ends_with(' ') {
                return stem.split('-').map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join("-");
            }
        }
    }
    t.to_string()
}

impl Item {
    /// Age rating and genres in one spelling (see above).
    pub fn normalize(&mut self) {
        if !self.content_rating.is_empty() { self.content_rating = normalize_age_rating(&self.content_rating); }
        let mut seen = std::collections::HashSet::new();
        self.genres = self.genres.iter().map(|g| normalize_genre(g)).filter(|g| !g.is_empty() && seen.insert(g.to_lowercase())).collect();
    }
}

/// The articles left off when sorting titles (lower case, each with a trailing space); empty = none.
static ARTICLES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
pub const DEFAULT_ARTICLES: &str = "the, a, an, der, die, das, ein, eine, le, la, les, el";

/// Applies the settings "ignore articles when sorting" and their list.
pub fn set_articles(enabled: bool, list: &str) {
    let v: Vec<String> = if enabled {
        list.split([',', ';', '\n']).map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty()).map(|a| format!("{a} ")).collect()
    } else { vec![] };
    if let Ok(mut a) = ARTICLES.lock() { *a = v; }
}

/// Lower-case title with leading articles moved off, for sorting.
pub fn sort_title(title: &str) -> String {
    let t = title.trim();
    let lower = t.to_lowercase();
    if let Ok(articles) = ARTICLES.lock() {
        for article in articles.iter() {
            if lower.starts_with(article.as_str()) && lower.len() > article.len() {
                return lower[article.len()..].trim_start().to_string();
            }
        }
    }
    lower
}
