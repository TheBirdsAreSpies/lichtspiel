//! A library's pages: all items, first letters, genres, collections and filters.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    handled(|hit| match (m, segs) {
        ("GET", ["library", "sections", sid, "all"]) => {
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let db = app.db.lock().unwrap();
            let Some(sec) = db.sections.iter().find(|s| s.id == sid) else { return fail(404, "No such library") };
            if !db.can_see_section(&user, sid) { return fail(403, "You don't have access to this library"); }
            let (kind, mut ids) = section_ids(&db, &user, sid, sec, ctx);
            let default_sort = if kind == ItemKind::Episode { "episode_order" } else { "title" };
            query::sort_items(&db, user.id, &mut ids, ctx.param("sort").filter(|s| !s.is_empty()).unwrap_or(default_sort));
            let (pg, off) = page(ctx, &ids);
            let ix = Index::build(&db);
            let list: Vec<Value> = pg.iter().filter_map(|i| db.items.get(i)).map(|i| item_json_ix(&db, &user, i, false, Some(&ix))).collect();
            ok(container(list, ids.len(), off, json!({"librarySectionID": sid, "librarySectionTitle": sec.name, "viewGroup": kind.as_str()})))
        }
        // How many items start with each letter (everything else under "#"), in A-Z order, for the
        // alphabet at the side of the library; the same filters as /all.
        ("GET", ["library", "sections", sid, "firstCharacter"]) => {
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let db = app.db.lock().unwrap();
            let Some(sec) = db.sections.iter().find(|s| s.id == sid) else { return fail(404, "No such library") };
            if !db.can_see_section(&user, sid) { return fail(403, "You don't have access to this library"); }
            let (_, mut ids) = section_ids(&db, &user, sid, sec, ctx);
            // In the order the list has (A-Z): where each letter's titles begin and end. Worked out from the
            // sorted list itself, so the letters land exactly, however titles with signs and umlauts sort.
            query::sort_items(&db, user.id, &mut ids, "title:asc");
            // Runs of the same letter, one after the other: a letter whose titles are not all in one place (Ä, Ö, Ü
            // sort behind Z; signs are scattered) has several runs.
            let mut runs: Vec<(char, usize, usize)> = vec![];
            for (pos, i) in ids.iter().filter_map(|i| db.items.get(i)).enumerate() {
                let c = i.sort_title.chars().next().map(|c| c.to_uppercase().next().unwrap_or(c)).filter(|c| c.is_alphabetic()).unwrap_or('#');
                let c = match c { 'Ä' => 'A', 'Ö' => 'O', 'Ü' => 'U', c => c };
                match runs.last_mut() { Some(r) if r.0 == c => r.2 = pos + 1, _ => runs.push((c, pos, pos + 1)) }
            }
            ok(json!({"MediaContainer": {"size": runs.len(), "total": ids.len(), "Directory": runs.iter().map(|(c, from, to)| json!({"key": c.to_string(), "size": to - from, "start": from, "end": to})).collect::<Vec<_>>()}}))
        }
        ("GET", ["library", "sections", sid, "genres"]) => {
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let db = app.db.lock().unwrap();
            if !db.can_see_section(&user, sid) { return fail(403, "You don't have access to this library"); }
            let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
            for i in db.items.values().filter(|i| i.section_id == sid && i.parent_id.is_none()) {
                for g in &i.genres { *counts.entry(g.clone()).or_default() += 1; }
            }
            let years: std::collections::BTreeSet<u32> = db.items.values().filter(|i| i.section_id == sid).filter_map(|i| i.year).collect();
            ok(json!({"MediaContainer": {"Directory": counts.iter().map(|(g, c)| json!({"title": g, "count": c})).collect::<Vec<_>>(), "years": years}}))
        }
        ("GET", ["library", "sections", sid, "collections"]) | ("GET", ["library", "sections", sid, "categories"]) => {
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let db = app.db.lock().unwrap();
            if !db.sections.iter().any(|s| s.id == sid) { return fail(404, "No such library") }
            if !db.can_see_section(&user, sid) { return fail(403, "You don't have access to this library"); }
            let cats = ctx.path.ends_with("categories");
            let list = if cats { related::categories(&db, &user, sid) } else { related::collections(&db, &user, sid) };
            let (pg, off) = page(ctx, &list);
            ok(container(pg, list.len(), off, json!({"librarySectionID": sid, "viewGroup": if cats { "category" } else { "collection" }})))
        }
        ("GET", ["library", "metadata", iid, "related"]) => with_item(app, &user, iid, |db, it| {
            let count = ctx.param("count").and_then(|c| c.parse().ok()).filter(|c: &usize| *c > 0).unwrap_or(related::HUB_COUNT);
            // One index for all rows: without it every title of every row searched the whole library.
            let ix = Index::build(db);
            let hubs: Vec<Value> = related::related(db, &user, it, count).into_iter().map(|hb| {
                let list: Vec<Value> = hb.ids.iter().filter_map(|i| db.items.get(i)).map(|i| item_json_ix(db, &user, i, false, Some(&ix))).collect();
                json!({"title": hb.title, "hubIdentifier": hb.identifier, "context": hb.context, "type": it.kind.as_str(), "size": list.len(), "more": hb.more, "Metadata": list})
            }).collect();
            ok(json!({"MediaContainer": {"size": hubs.len(), "Hub": hubs}}))
        }),
        ("GET", ["library", "sections", sid, "filters"]) => {
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let db = app.db.lock().unwrap();
            let Some(sec) = db.sections.iter().find(|s| s.id == sid) else { return fail(404, "No such library") };
            if !db.can_see_section(&user, sid) { return fail(403, "You don't have access to this library"); }
            let kind = ctx.param("type").and_then(ItemKind::parse).unwrap_or(match sec.kind {
                SectionKind::Movie => ItemKind::Movie,
                SectionKind::Show => ItemKind::Show,
                SectionKind::Other => ItemKind::Video,
                SectionKind::Music => ItemKind::Artist,
            });
            let items: Vec<&db::Item> = db.items.values().filter(|i| i.section_id == sid && i.kind == kind).collect();
            // Labels and folders only for the owner; duplicates only with ?includeAdvanced=1.
            let advanced = matches!(ctx.param("includeAdvanced"), Some("1") | Some("true"));
            let list = query::section_filters(&db, user.id, &items, kind, user.admin, advanced);
            ok(json!({"MediaContainer": {"size": list.len(), "type": kind.as_str(), "Filter": list}}))
        }
        _ => { *hit = false; fail(404, "") }
    })
}
