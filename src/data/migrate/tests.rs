//! Tests for the import from the old server.

use super::*;

#[test]
fn reads_plex_external_ids() {
    let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let ext = plex_external_id;
    assert_eq!(ext("plex://movie/5d776", &v(&["imdb://tt0133093", "tmdb://603", "tvdb://169"])), Some(("tmdb", "603".into())));
    assert_eq!(ext("plex://movie/5d776", &v(&["imdb://tt0133093"])), Some(("imdb", "tt0133093".into())));
    assert_eq!(ext("com.plexapp.agents.imdb://tt0133093?lang=en", &[]), Some(("imdb", "tt0133093".into())));
    assert_eq!(ext("com.plexapp.agents.themoviedb://603?lang=de", &[]), Some(("tmdb", "603".into())));
    assert_eq!(ext("com.plexapp.agents.thetvdb://81189?lang=en", &[]), Some(("tvdb", "81189".into())));
    assert_eq!(ext("com.plexapp.agents.none://1", &[]), None);
    assert_eq!(ext("local://5", &v(&["imdb://xx"])), None);
}
#[test]
fn paths() {
    let maps = vec![("D:\\Filme".to_string(), "/media/Filme".to_string()), ("D:\\".to_string(), "/media/other".to_string())];
    assert_eq!(map_path("D:\\Filme\\Alien (1979)\\Alien.mkv", &maps), "/media/Filme/Alien (1979)/Alien.mkv");
    assert_eq!(map_path("d:\\filme\\x.mkv", &maps), "/media/Filme/x.mkv");
    assert_eq!(map_path("D:\\Serien\\a.mkv", &maps), "/media/other/Serien/a.mkv");
    assert_eq!(map_path("/volume1/video/a.mkv", &maps), "/volume1/video/a.mkv");
    assert_eq!(map_path("/volume1/video/a.mkv", &[("/volume1/video".into(), "/media".into())]), "/media/a.mkv");
    assert_eq!(path_key("C:\\Movies\\A.mkv"), path_key("c:/movies/a.mkv"));
    assert_eq!(date_to_epoch("1970-01-02 00:00:10"), Some(86410));
    assert_eq!(db_date(86400 * 365), "1971-01-01");
}

#[cfg(test)]
mod credits_index_tests {
    use super::*;
    #[test]
    fn finds_files_under_another_root() {
        let mut idx = CreditsIndex::default();
        let v = vec![2_648_297u64];
        let k = path_key("/volume1/Serien/Daisy Jones/Season 1/S01E01.mkv");
        for (n, t) in [&mut idx.tail3, &mut idx.tail2].into_iter().enumerate() {
            t.insert(path_tail(&k, 3 - n), Some(v.clone()));
        }
        idx.exact.insert(k, v.clone());
        assert_eq!(idx.get("/data/tv/Daisy Jones/Season 1/S01E01.mkv"), Some(&v));
        assert_eq!(idx.get(r"D:\Daisy Jones\Season 1\S01E01.mkv"), Some(&v));
        assert_eq!(idx.get("/data/tv/Other/Season 1/S01E02.mkv"), None);
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use crate::data::db::{Item, MediaFile};

    /// A tiny old-server database: two old movies with the same guid ("Movies" and "Movies 4K"),
    /// watched and rated by account 1, with two logged viewings.
    fn old_db(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let f = dir.join("plex.db");
        let c = Connection::open(&f).unwrap();
        c.execute_batch("create table metadata_items (id, parent_id, metadata_type, guid, title, original_title, summary, tagline, studio, content_rating, rating, year, originally_available_at, added_at);
            create table media_items (id, metadata_item_id); create table media_parts (id, media_item_id, file);
            create table metadata_item_settings (account_id, guid, rating, view_offset, view_count, last_viewed_at);
            create table metadata_item_views (account_id, guid, viewed_at); create table accounts (id, name);
            insert into accounts values (1, 'ste');
            insert into metadata_items values (100, null, 1, 'plex://movie/x', 'X', '', '', '', '', '', null, 2000, null, 0), (101, null, 1, 'plex://movie/x', 'X', '', '', '', '', '', null, 2000, null, 0);
            insert into media_items values (1, 100), (2, 101); insert into media_parts values (1, 1, '/m/a.mkv'), (2, 2, '/m4k/a.mkv');
            insert into metadata_item_settings values (1, 'plex://movie/x', 8, 0, 1, 1000);
            insert into metadata_item_views values (1, 'plex://movie/x', 500), (1, 'plex://movie/x', 900);").unwrap();
        f
    }

    fn app_with_two_copies(name: &str) -> (Arc<App>, PathBuf, Options) {
        let (app, root) = crate::server::tests::test_app(name);
        {
            let mut db = app.db.lock().unwrap();
            for (id, path) in [(10u64, "/m/a.mkv"), (11, "/m4k/a.mkv")] {
                let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "X", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
                i.file = Some(MediaFile { path: path.into(), ..Default::default() });
                db.items.insert(id, i);
            }
        }
        let o = Options { db: old_db(&root.join("old")), path_map: vec![], libraries: false, watched: true, playlists: false, markers: false, details: false, accounts: HashMap::from([(1, 1)]) };
        (app, root, o)
    }

    #[test]
    fn the_same_movie_in_two_old_libraries_is_watched_in_both() {
        let (app, root, o) = app_with_two_copies("import-guid");
        run(&app, &o).unwrap();
        let db = app.db.lock().unwrap();
        let got: Vec<(u32, Option<f32>, usize)> = [10, 11].iter().map(|i| (db.view(1, *i).view_count, db.view(1, *i).rating, db.history.iter().filter(|h| h.item_id == *i).count())).collect();
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(got, vec![(1, Some(8.0), 2), (1, Some(8.0), 2)], "(watched, rating, history entries) of each copy");
    }

    #[test]
    fn importing_twice_keeps_the_play_history_as_it_was() {
        let (app, root, o) = app_with_two_copies("import-twice");
        run(&app, &o).unwrap();
        let first = app.db.lock().unwrap().history.len();
        run(&app, &o).unwrap();
        let second = app.db.lock().unwrap().history.len();
        let _ = std::fs::remove_dir_all(&root);
        assert!(first > 0);
        assert_eq!(second, first, "the second import added the viewings again");
    }

    #[test]
    fn the_old_database_is_copied_again_only_when_it_changed() {
        let root = std::env::temp_dir().join(format!("lichtspiel-open-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = old_db(&root);
        let work = root.join("work");
        drop(open_copy(&src, &work).unwrap());
        // Something only the copy has: a fresh copy would lose it.
        Connection::open(work.join("library.db")).unwrap().execute_batch("create table only_in_copy (x)").unwrap();
        let kept = open_copy(&src, &work).unwrap().prepare("select * from only_in_copy").is_ok();
        // The old server writes to its database: copied again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        Connection::open(&src).unwrap().execute_batch("insert into accounts values (2, 'kid')").unwrap();
        let fresh = open_copy(&src, &work).unwrap().prepare("select * from only_in_copy").is_err();
        let _ = std::fs::remove_dir_all(&root);
        assert!(kept, "the unchanged old database was copied again");
        assert!(fresh, "a changed old database was not copied again");
    }
}
