//! Tests for the stored data.

use super::*;

#[test]
fn shown_name_is_the_full_name_or_the_user_name() {
    let mut u: User = serde_json::from_value(serde_json::json!({"id": 1, "username": "fab92", "admin": false, "password_hash": "", "salt": "", "created_at": 0})).unwrap();
    assert_eq!(u.display_name(), "fab92");
    u.full_name = "  ".into();
    assert_eq!(u.display_name(), "fab92");
    u.full_name = " Fabian Bauer ".into();
    assert_eq!(u.display_name(), "Fabian Bauer");
}

#[cfg(test)]
mod normalize_tests {
    use super::*;
    #[test]
    fn age_ratings_and_genres_get_one_spelling() {
        for r in ["12", "FSK 12", "FSK12", "DE-12", "ab 12", "Germany:12", "FSK-12"] { assert_eq!(normalize_age_rating(r), "FSK 12", "{r}"); }
        assert_eq!(normalize_age_rating("0"), "FSK 0");
        assert_eq!(normalize_age_rating("PG-13"), "PG-13");
        assert_eq!(normalize_age_rating("TV-MA"), "TV-MA");
        assert_eq!(normalize_genre("Horrorfilm"), "Horror");
        assert_eq!(normalize_genre("Familienfilm"), "Familie");
        assert_eq!(normalize_genre("Sci-Fi"), "Science Fiction");
        assert_eq!(normalize_genre("Film Noir"), "Film Noir");
        assert_eq!(normalize_genre("Drama"), "Drama");
        assert_eq!(normalize_genre("Buddy-Film"), "Buddy");
        assert_eq!(normalize_genre("Buddy-Cop-Film"), "Buddy-Cop");
    }
    #[test]
    fn articles_are_left_off_when_asked() {
        set_articles(true, "the, die");
        assert_eq!(sort_title("The Matrix"), "matrix");
        assert_eq!(sort_title("Die Abbotts"), "abbotts");
        set_articles(false, "the");
        assert_eq!(sort_title("The Matrix"), "the matrix");
        set_articles(true, DEFAULT_ARTICLES);
    }
}
