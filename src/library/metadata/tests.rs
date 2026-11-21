//! Tests for the online details.

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

#[cfg(test)]
mod trailer_tests {
    use super::*;

    #[test]
    fn online_trailers_order() {
        let r = json!([
            {"site": "YouTube", "key": "c1", "type": "Clip", "name": "A clip", "iso_639_1": "de", "official": true},
            {"site": "Vimeo", "key": "v1", "type": "Trailer", "name": "Elsewhere", "iso_639_1": "de"},
            {"site": "YouTube", "key": "e1", "type": "Trailer", "name": "Trailer", "iso_639_1": "en", "official": true},
            {"site": "YouTube", "key": "d1", "type": "Trailer", "name": "Offizieller Trailer", "iso_639_1": "de", "official": true},
            {"site": "YouTube", "key": "r1", "type": "Trailer", "name": "Red Band Trailer", "iso_639_1": "en", "official": true},
            {"site": "YouTube", "key": "t1", "type": "Teaser", "name": "Teaser", "iso_639_1": "de", "official": false},
        ]);
        let keys: Vec<String> = tmdb_trailers(&r, "de-DE", false).iter().map(|v| v["key"].as_str().unwrap().to_string()).collect();
        assert_eq!(keys, vec!["d1", "e1", "t1", "c1"]);
        assert_eq!(video_langs("de-DE", true), "de,en,null");
        assert_eq!(video_langs("de-DE", false), "de,null");
        assert_eq!(video_langs("en-US", true), "en,null");
        assert!(tmdb_trailers(&r, "de-DE", true).iter().any(|v| v["key"] == "r1"));
    }
}

#[cfg(test)]
mod match_tests {
    use super::*;
    #[test]
    fn titles_match_despite_umlauts_apostrophes_and_subtitles() {
        assert!(score("angels share ein schluck für die engel", Some(2012), "Angel's Share - Ein Schluck für die Engel", Some(2012)) >= 1.15);
        assert!(score("arsen und spitzenhaeubchen", Some(1944), "Arsen und Spitzenhäubchen", Some(1944)) >= 1.15);
        assert!(score("die abbotts", Some(1997), "Die Abbotts - Wenn Haß die Liebe tötet", Some(1997)) >= 1.1);
        assert!(score("die abbotts", Some(1997), "Die Simpsons", Some(1997)) < 0.85);
    }
}

#[cfg(test)]
mod variant_tests {
    use super::title_variants;

    #[test]
    fn umlaut_spellings_are_tried() {
        let v = title_variants("Arsen und Spitzenhaeubchen");
        assert!(v.contains(&"Arsen und Spitzenhäubchen".to_string()), "{v:?}");
        let w = title_variants("Käse für Größe");
        assert!(w.contains(&"Kaese fuer Groesse".to_string()), "{w:?}");
        assert!(w.contains(&"Kase fur Grosse".to_string()), "{w:?}");
        let a = title_variants("Affe im Menschen, Der");
        assert_eq!(a[0], "Der Affe im Menschen", "{a:?}");
        assert_eq!(title_variants("Affe im Menschen, Der (1990)")[0], "Der Affe im Menschen (1990)");
        assert_eq!(title_variants("Hallo, Welt")[0], "Hallo, Welt");
    }
}

#[cfg(test)]
mod genre_tests {
    use super::clean_genre;
    #[test]
    fn genre_suffixes_are_cut() {
        assert_eq!(clean_genre("Buddy-Film"), "Buddy");
        assert_eq!(clean_genre("Buddy-Cop-Film"), "Buddy-Cop");
        assert_eq!(clean_genre("drama film"), "Drama");
        assert_eq!(clean_genre("buddy-"), "Buddy");
        assert_eq!(clean_genre("Film"), "Film");
        assert_eq!(clean_genre("Komödie"), "Komödie");
    }
}

#[cfg(test)]
mod typed_id_tests {
    use super::typed_id;
    #[test]
    fn ids_in_the_search_box() {
        assert_eq!(typed_id("tt0068646"), Some(("imdb", "tt0068646".into())));
        assert_eq!(typed_id(" IMDb: tt0068646 "), Some(("imdb", "tt0068646".into())));
        assert_eq!(typed_id("tmdb-603"), Some(("tmdb", "603".into())));
        assert_eq!(typed_id("https://www.themoviedb.org/movie/603-the-matrix"), Some(("tmdb", "603".into())));
        assert_eq!(typed_id("https://www.imdb.com/title/tt0068646/?ref_=x"), Some(("imdb", "tt0068646".into())));
        assert_eq!(typed_id("https://www.imdb.com/de/title/tt0813980/"), Some(("imdb", "tt0813980".into())));
        assert_eq!(typed_id("1917"), None);
        assert_eq!(typed_id("Tt Man"), None);
    }
}

#[cfg(test)]
mod coming_tests {
    use super::*;

    #[test]
    fn tvmaze_episodes_from_today_on_and_no_specials_without_a_number() {
        let v = json!({"_embedded": {"episodes": [
            {"season": 2, "number": 7, "name": "Old", "airdate": "2026-11-14"},
            {"season": 2, "number": 8, "name": "Today", "airdate": "2026-11-21"},
            {"season": 2, "number": null, "name": "Special", "airdate": "2026-11-25"},
            {"season": 2, "number": 9, "name": "Unknown", "airdate": ""},
            {"season": 2, "number": 10, "name": "Later", "airdate": "2026-12-05"}]}});
        let got = tvmaze_coming(&v, "2026-11-21");
        assert_eq!(got, vec![(2, 8, "Today".into(), "2026-11-21".into()), (2, 10, "Later".into(), "2026-12-05".into())]);
    }
}
