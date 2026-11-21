//! Keeping the library tidy after a scan: replaced files, merged versions and shows, empty seasons and the trash.

use super::*;

/// Titles whose file is gone (not in `seen`, on a reachable folder) that another title of the
/// library now stands in for: a movie with the same title and year (or the only other movie in the
/// gone file's own folder), an episode with the same season and number. The other title's files
/// move into the old one and the other title goes; what someone already did with the new one
/// (watched, rated) moves along where the old one has nothing. Returns (old, new) ids.
pub(super) fn adopt_replacements(db: &mut Db, section: u64, seen: &HashSet<String>, later_parts: &HashSet<String>, offline: &[PathBuf], only: Option<&Path>) -> Vec<(u64, u64)> {
    let roots: Vec<PathBuf> = db.sections.iter().filter(|s| s.id == section).flat_map(|s| s.paths.iter().map(PathBuf::from)).collect();
    let present = |i: &Item| i.file.as_ref().is_some_and(|f| seen.contains(&f.path)) && !i.extra.contains_key(MISSING);
    let candidate = |i: &Item| i.section_id == section && matches!(i.kind, ItemKind::Movie | ItemKind::Episode) && !i.is_extra();
    let olds: Vec<u64> = db.items.values()
        .filter(|i| candidate(i) && i.version_of().is_none())
        .filter(|i| i.file.as_ref().is_some_and(|f| {
            let p = Path::new(&f.path);
            !seen.contains(&f.path) && !later_parts.contains(&f.path) && !p.is_file()
                && only.is_none_or(|o| p.starts_with(o)) && !offline.iter().any(|r| p.starts_with(r))
        }))
        .map(|i| i.id).collect();
    let dir_of = |i: &Item| i.file.as_ref().and_then(|f| Path::new(&f.path).parent().map(Path::to_path_buf));
    let mut out = vec![];
    let mut taken: HashSet<u64> = HashSet::new();
    for old_id in olds {
        let Some(old) = db.items.get(&old_id).cloned() else { continue };
        let others: Vec<&Item> = db.items.values()
            .filter(|i| candidate(i) && i.id != old_id && !taken.contains(&i.id) && present(i) && i.kind == old.kind)
            // A version of a different title is not a stand-in.
            .filter(|i| i.version_of().is_none_or(|v| v == old_id))
            .collect();
        let pick = if old.kind == ItemKind::Episode {
            others.iter().filter(|i| old.parent_id.is_some() && i.parent_id == old.parent_id && old.index.is_some() && i.index == old.index).map(|i| i.id).min()
        } else {
            let same_year = |i: &Item| old.year.is_none() || i.year.is_none() || i.year == old.year;
            let by_title = others.iter().filter(|i| i.title.to_lowercase() == old.title.to_lowercase() && same_year(i)).map(|i| i.id).min();
            // A movie in its own folder (not a library folder itself): the one other movie there.
            let by_dir = || {
                let dir = dir_of(&old).filter(|d| !roots.iter().any(|r| r == d))?;
                let here: Vec<u64> = others.iter().filter(|i| dir_of(i).as_ref() == Some(&dir)).map(|i| i.id).collect();
                (here.len() == 1).then(|| here[0])
            };
            by_title.or_else(by_dir)
        };
        let Some(new_id) = pick else { continue };
        let Some(new) = db.items.remove(&new_id) else { continue };
        taken.insert(new_id);
        if let Some(it) = db.items.get_mut(&old_id) {
            it.file = new.file;
            it.parts = new.parts;
            it.duration_ms = new.duration_ms;
            it.updated_at = now();
            it.extra.remove(MISSING);
            if it.poster.is_none() { it.poster = new.poster; }
        }
        // Its extras and its own versions now belong to the old title.
        let kids: Vec<u64> = db.items.values().filter(|i| i.parent_id == Some(new_id) || i.version_of() == Some(new_id)).map(|i| i.id).collect();
        for k in kids {
            if let Some(it) = db.items.get_mut(&k) {
                if it.parent_id == Some(new_id) { it.parent_id = Some(old_id); }
                if it.version_of() == Some(new_id) { it.extra.insert("version_of".into(), Value::from(old_id)); }
            }
        }
        let suffix = format!(":{new_id}");
        let moved: Vec<String> = db.views.keys().filter(|k| k.ends_with(&suffix)).cloned().collect();
        for k in moved {
            let Some(v) = db.views.remove(&k) else { continue };
            let to = format!("{}:{old_id}", &k[..k.len() - suffix.len()]);
            if db.views.get(&to).is_none() { db.views.insert(to, v); }
        }
        out.push((old_id, new_id));
    }
    out
}

/// Several files of one movie (or one episode) become one title with several versions:
/// the same title and year in a movie library, the same season and episode number in a show. What
/// was split apart by hand (`no_merge`) stays alone, and files with an `{edition-…}` stay titles of
/// their own. Returns how many files were put under another.
pub(super) fn auto_merge_versions(db: &mut Db, section: u64) -> usize {
    let mut groups: HashMap<(bool, Option<u64>, String, Option<u32>), Vec<u64>> = HashMap::new();
    for i in db.items.values() {
        if i.section_id != section || !matches!(i.kind, ItemKind::Movie | ItemKind::Episode) || i.is_extra() { continue; }
        if i.file.is_none() { continue; }
        if i.extra.contains_key(MISSING) || i.extra.contains_key("no_merge") { continue; }
        if !crate::library::query::item_edition(i).is_empty() { continue; }
        let key = if i.kind == ItemKind::Movie {
            (true, None, i.title.to_lowercase(), i.year)
        } else {
            let (Some(p), Some(n)) = (i.parent_id, i.index) else { continue };
            (false, Some(p), String::new(), Some(n))
        };
        groups.entry(key).or_default().push(i.id);
    }
    let mut moves: Vec<(u64, u64)> = vec![];
    for (_, mut ids) in groups {
        if ids.len() < 2 { continue; }
        ids.sort();
        let is_main = |id: &u64| db.items.get(id).is_some_and(|i| i.version_of().is_none());
        // The title that already has versions stays the main one; else the oldest.
        let main = ids.iter().copied().find(|m| is_main(m) && ids.iter().any(|o| db.items.get(o).is_some_and(|x| x.version_of() == Some(*m))))
            .or_else(|| ids.iter().copied().find(is_main));
        let Some(main) = main else { continue };
        moves.extend(ids.iter().copied().filter(|id| *id != main && is_main(id)).map(|id| (id, main)));
    }
    let mut n = 0;
    for (id, main) in moves {
        // What it had as versions moves along.
        let moved: Vec<u64> = db.items.values().filter(|x| x.version_of() == Some(id)).map(|x| x.id).collect();
        for i in std::iter::once(id).chain(moved) {
            if let Some(it) = db.items.get_mut(&i) { it.extra.insert("version_of".into(), Value::from(main)); }
        }
        n += 1;
    }
    n
}

/// Marks an item whose file is gone from disk but kept (the library's trash).
pub const MISSING: &str = "missing_since";

/// Which items have children (all libraries: a child is found by its parent only).
pub(super) fn children_map(db: &Db) -> HashMap<u64, Vec<u64>> {
    let mut m: HashMap<u64, Vec<u64>> = HashMap::new();
    for i in db.items.values() {
        if let Some(p) = i.parent_id {
            m.entry(p).or_default().push(i.id);
        }
    }
    m
}

/// Takes out seasons and shows left without episodes, and extras whose movie
/// is gone. Returns what was taken out.
pub fn prune_empty(db: &mut Db, section: u64) -> Vec<u64> {
    let mut out = vec![];
    let orphans: Vec<u64> = db.items.values()
        .filter(|i| i.section_id == section && i.is_extra() && i.parent_id.is_some_and(|p| !db.items.contains_key(&p)))
        .map(|i| i.id).collect();
    for id in orphans {
        db.items.remove(&id);
        out.push(id);
    }
    // Versions whose main item is gone show up on their own again.
    let loose: Vec<u64> = db.items.values().filter(|i| i.version_of().is_some_and(|p| !db.items.contains_key(&p))).map(|i| i.id).collect();
    for id in loose {
        if let Some(i) = db.items.get_mut(&id) { i.extra.remove("version_of"); }
    }
    for _ in 0..2 {
        let kids = children_map(db);
        let empty: Vec<u64> = db.items.values()
            .filter(|i| i.section_id == section && !i.kind.playable() && !kids.contains_key(&i.id))
            .map(|i| i.id).collect();
        for id in empty {
            db.items.remove(&id);
            out.push(id);
        }
    }
    out
}

/// "Empty trash": takes the unavailable items of a library out for good (those
/// whose file is still not there), then the seasons and shows left empty.
pub fn empty_trash(db: &mut Db, section: u64) -> Vec<u64> {
    let gone: Vec<u64> = db.items.values()
        .filter(|i| i.section_id == section && i.extra.contains_key(MISSING))
        .filter(|i| i.file.as_ref().is_none_or(|f| !Path::new(&f.path).is_file()))
        .map(|i| i.id).collect();
    for id in &gone {
        db.items.remove(id);
    }
    let mut out = gone;
    out.extend(prune_empty(db, section));
    out
}

#[allow(clippy::too_many_arguments)]
pub(super) fn find_or_make(
    db: &mut Db,
    cache: &mut HashMap<ContainerKey, u64>,
    section: u64,
    kind: ItemKind,
    parent: Option<u64>,
    title: &str,
    year: Option<u32>,
    index: Option<u32>,
    poster: Option<String>,
) -> u64 {
    let key = sort_title(title);
    // A show matched online carries the found title; the name it was scanned under still finds it.
    let matches = |i: &Item| {
        i.section_id == section
            && i.kind == kind
            && i.parent_id == parent
            && match kind {
                ItemKind::Season => i.index == index,
                _ => i.sort_title == key || scanned_key(i).is_some_and(|k| k == key),
            }
    };
    let ck = container_key(kind, parent, index, &key);
    // The cached id is checked against the item itself (it may have been
    // renamed or removed since); only then is the whole library searched (the oldest first).
    let cached = cache.get(&ck).and_then(|id| db.items.get(id)).filter(|i| matches(i));
    let found = cached.or_else(|| db.items.values().filter(|i| matches(i)).min_by_key(|i| i.id));
    if let Some(it) = found {
        cache.insert(ck, it.id);
        let id = it.id;
        if it.poster.is_none() && poster.is_some() {
            if let Some(it) = db.items.get_mut(&id) {
                it.poster = poster;
            }
        }
        return id;
    }
    let id = db.new_id();
    let t = now();
    db.items.insert(
        id,
        Item {
            id,
            section_id: section,
            kind,
            parent_id: parent,
            title: title.to_string(),
            sort_title: key,
            year,
            index,
            summary: String::new(),
            tagline: String::new(),
            genres: vec![],
            rating: None,
            content_rating: String::new(),
            studio: String::new(),
            added_at: t,
            updated_at: t,
            duration_ms: 0,
            file: None,
            parts: vec![],
            poster,
            extra: Default::default(),
            locked: vec![],
        },
    );
    cache.insert(ck, id);
    id
}

/// What find_or_make tells shows and seasons apart by: a season by its
/// number under its show, anything else by its sort title.
pub(super) type ContainerKey = (&'static str, Option<u64>, Option<u32>, String);

fn container_key(kind: ItemKind, parent: Option<u64>, index: Option<u32>, sort_title: &str) -> ContainerKey {
    match kind {
        ItemKind::Season => (kind.as_str(), parent, index, String::new()),
        _ => (kind.as_str(), parent, None, sort_title.to_string()),
    }
}

/// The shows and seasons of a library, by their [`ContainerKey`].
pub(super) fn container_index(db: &Db, section: u64) -> HashMap<ContainerKey, u64> {
    let mut m = HashMap::new();
    let mut all: Vec<&Item> = db.items.values().filter(|i| i.section_id == section && !i.kind.playable()).collect();
    all.sort_by_key(|i| i.id);
    // The oldest wins, as in a search through the library; the title it has now before the one it was scanned under.
    for i in &all {
        m.entry(container_key(i.kind, i.parent_id, i.index, &i.sort_title)).or_insert(i.id);
    }
    for i in &all {
        if let Some(k) = scanned_key(i) { m.entry(container_key(i.kind, i.parent_id, i.index, &k)).or_insert(i.id); }
    }
    m
}

/// The sort key of the name a show (artist, album) was scanned under, kept when an online match renamed it.
fn scanned_key(i: &Item) -> Option<String> {
    i.extra.get("file_title").and_then(|v| v.as_str()).filter(|t| !t.trim().is_empty()).map(sort_title)
}

/// Shows of a library that are one and the same (the same folder name; one was renamed by an online match
/// before more episodes came, and those made a second): the newer one's seasons and episodes move into the
/// oldest, it is left empty and goes. Returns the shows that took something in.
pub fn merge_duplicate_shows(db: &mut Db, section: u64) -> Vec<u64> {
    let mut shows: Vec<(u64, String)> = db.items.values()
        .filter(|i| i.section_id == section && i.kind == ItemKind::Show)
        .map(|i| (i.id, scanned_key(i).unwrap_or_else(|| i.sort_title.clone())))
        .collect();
    shows.sort();
    let mut keeper: HashMap<String, u64> = HashMap::new();
    let mut merged = vec![];
    for (id, key) in shows {
        let Some(&keep) = keeper.get(&key) else { keeper.insert(key, id); continue };
        let kids = children_map(db);
        for season in kids.get(&id).cloned().unwrap_or_default() {
            let index = db.items.get(&season).and_then(|s| s.index);
            let target = kids.get(&keep).into_iter().flatten()
                .find(|s| db.items.get(s).is_some_and(|s| s.kind == ItemKind::Season && s.index == index)).copied();
            match target {
                // That season is there already: its episodes move over, the empty season goes with the show.
                Some(t) => for e in kids.get(&season).cloned().unwrap_or_default() {
                    if let Some(e) = db.items.get_mut(&e) { e.parent_id = Some(t); }
                },
                None => if let Some(s) = db.items.get_mut(&season) { s.parent_id = Some(keep); },
            }
        }
        if !merged.contains(&keep) { merged.push(keep); }
    }
    merged
}
