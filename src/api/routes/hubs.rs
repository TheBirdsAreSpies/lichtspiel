//! The home page rows and search.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    handled(|hit| match (m, segs) {
        ("GET", ["hubs"]) | ("GET", ["hubs", "home"]) => {
            let db = app.db.lock().unwrap();
            let ix = Index::build(&db);
            let mut hubs = vec![hub_ix(&db, &user, "continueWatching", "Continue Watching", continue_watching(&db, &user), &ix)];
            let mut wl = crate::api::dashboard::watchlist_ids(&db, &user, "", None);
            wl.truncate(20);
            hubs.push(hub_ix(&db, &user, "watchlist", "Watchlist", wl, &ix));
            // Rows for the time of year (Halloween, Christmas ...); ?date=MM-DD tries another day.
            let day = ctx.param("date").and_then(related::month_day).or_else(|| related::month_day(&db::today())).unwrap_or((1, 1));
            for o in related::occasion_rows(&db, &user, day, 20) {
                hubs.push(hub_ix(&db, &user, &o.identifier, &o.title, o.ids, &ix));
            }
            let episodes = if db.settings.group_recent_episodes {
                let eps = recently_added(&db, &user, Some(ItemKind::Episode), 200);
                shows_of(&db, &eps, 20)
            } else {
                recently_added(&db, &user, Some(ItemKind::Episode), 20)
            };
            hubs.extend([
                hub_ix(&db, &user, "recentlyAddedMovies", "Recently Added Movies", recently_added(&db, &user, Some(ItemKind::Movie), 20), &ix),
                hub_ix(&db, &user, "recentlyAddedEpisodes", "Recently Added TV", episodes, &ix),
                hub_ix(&db, &user, "recentlyAddedAlbums", "Recently Added Music", recently_added(&db, &user, Some(ItemKind::Album), 20), &ix),
            ]);
            for p in db.playlists.iter().filter(|p| p.pinned && visible_playlist(p, &user)) {
                if matches!(p.default_key.as_deref(), Some("continue" | "recent_movies" | "recent_episodes")) { continue; }
                let mut ids = query::playlist_items(&db, &user, p);
                ids.truncate(20);
                hubs.push(hub_ix(&db, &user, &format!("playlist.{}", p.id), &p.title, ids, &ix));
            }
            hubs.retain(|h| h["size"].as_u64().unwrap_or(0) > 0);
            ok(json!({"MediaContainer": {"size": hubs.len(), "Hub": hubs}}))
        }
        ("GET", ["hubs", "sections", sid]) => {
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let db = app.db.lock().unwrap();
            if !db.sections.iter().any(|s| s.id == sid) { return fail(404, "No such library") }
            if !db.can_see_section(&user, sid) { return fail(403, "You don't have access to this library"); }
            let day = ctx.param("date").and_then(related::month_day).or_else(|| related::month_day(&db::today())).unwrap_or((1, 1));
            let count = ctx.param("count").and_then(|c| c.parse().ok()).filter(|c: &usize| *c > 0).unwrap_or(related::SECTION_ROW_COUNT);
            let ix = Index::build(&db);
            let hubs: Vec<Value> = related::section_rows(&db, &user, sid, day, db::now() / 86400, count).into_iter().map(|hb| {
                let mut v = hub_ix(&db, &user, &hb.identifier, &hb.title, hb.ids, &ix);
                v["context"] = json!(hb.context);
                v["more"] = json!(hb.more);
                v
            }).collect();
            ok(json!({"MediaContainer": {"size": hubs.len(), "librarySectionID": sid, "Hub": hubs}}))
        }
        ("GET", ["hubs", "home", "continueWatching"]) | ("GET", ["hubs", "continueWatching"]) | ("GET", ["hubs", "continueWatching", "items"]) => {
            let db = app.db.lock().unwrap();
            let ids = continue_watching(&db, &user);
            list_resp(&db, &user, &ids, "continueWatching")
        }
        ("GET", ["hubs", "home", "onDeck"]) | ("GET", ["library", "onDeck"]) => {
            let db = app.db.lock().unwrap();
            let ids = on_deck(&db, &user);
            list_resp(&db, &user, &ids, "onDeck")
        }
        ("GET", ["hubs", "home", "recentlyAdded"]) | ("GET", ["library", "recentlyAdded"]) => {
            let db = app.db.lock().unwrap();
            let kind = ctx.param("type").and_then(ItemKind::parse);
            let ids = recently_added(&db, &user, kind, ctx.num("limit").unwrap_or(50) as usize);
            list_resp(&db, &user, &ids, "recentlyAdded")
        }
        ("GET", ["hubs", "search"]) | ("GET", ["library", "search"]) | ("GET", ["search"]) => {
            let q = ctx.param("query").or(ctx.param("q")).unwrap_or("").trim().to_lowercase();
            let db = app.db.lock().unwrap();
            if q.is_empty() { return list_resp(&db, &user, &[], "search"); }
            // Not only titles: genres, the year, the studio, and the people of a movie or show.
            let year: Option<u32> = q.parse().ok().filter(|y| (1800..=2200).contains(y));
            let by_title = |i: &db::Item| i.title.to_lowercase().contains(&q);
            let by_other = |i: &db::Item| {
                i.genres.iter().any(|g| g.to_lowercase() == q)
                    || year.is_some_and(|y| i.year == Some(y))
                    || (q.len() >= 3 && i.studio.to_lowercase().contains(&q))
                    || (q.len() >= 3 && matches!(i.kind, ItemKind::Album | ItemKind::Track) && match query::field(&db, user.id, i, "artist") {
                        query::FieldValue::Text(v) => v.iter().any(|n| n.to_lowercase().contains(&q)),
                        _ => false,
                    })
                    || (q.len() >= 3 && i.kind != ItemKind::Episode && ["director", "actor", "writer", "producer"].iter().any(|f| match query::field(&db, user.id, i, f) {
                        query::FieldValue::Text(v) => v.iter().any(|n| n.to_lowercase().contains(&q)),
                        _ => false,
                    }))
            };
            let mut ids: Vec<u64> = db.items.values()
                .filter(|i| db.can_see_item(&user, i) && i.kind != ItemKind::Season && !i.hidden())
                .filter(|i| by_title(i) || by_other(i))
                .map(|i| i.id)
                .collect();
            ids.sort_by_key(|i| {
                let it = &db.items[i];
                (!it.title.to_lowercase().starts_with(&q), !by_title(it), matches!(it.kind, ItemKind::Episode | ItemKind::Track), it.sort_title.clone())
            });
            ids.truncate(ctx.num("limit").unwrap_or(60) as usize);
            list_resp(&db, &user, &ids, "search")
        }
        _ => { *hit = false; fail(404, "") }
    })
}
