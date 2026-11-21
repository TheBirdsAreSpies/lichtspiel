//! Extras around playback: movie pre-roll, theme music, chapters and lyrics.

use super::*;

/// The video files named in the settings that exist.
fn preroll_files(app: &Arc<App>) -> Vec<String> {
    let list = app.settings().preroll_paths;
    list.split(|c| c == ';' || c == '\n').map(|p| p.trim().trim_matches('"').to_string()).filter(|p| !p.is_empty() && Path::new(p).is_file()).collect()
}

/// GET /api/preroll → {url, urls}: what to play before a movie, by the setting "preroll_mode"
/// (one at random, all in the order listed, or always the first); {url: null} when there is none.
pub fn preroll(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let files = preroll_files(app);
    if files.is_empty() { return ok(json!({"url": null, "urls": []})); }
    let tok = match &ctx.user { Some(u) => format!("?ticket={}", crate::accounts::auth::ticket(&app.db.lock().unwrap().ticket_key, u.id, "p", ctx.token.as_deref(), now()).0), None => String::new() };
    let url = |n: usize| format!("/api/preroll/{n}/file{tok}");
    let picked: Vec<usize> = match app.settings().preroll_mode.as_str() {
        "all" => (0..files.len()).collect(),
        "first" => vec![0],
        _ => vec![(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) as usize) % files.len()],
    };
    let urls: Vec<String> = picked.into_iter().map(url).collect();
    ok(json!({"url": urls.first(), "urls": urls, "count": files.len()}))
}

/// GET /api/preroll/{n}/file: the n-th pre-roll video, with seeking.
pub fn preroll_file(app: &Arc<App>, ctx: &Ctx, n: &str) -> Resp {
    let files = preroll_files(app);
    let Some(path) = n.parse::<usize>().ok().and_then(|i| files.get(i)) else { return fail(404, "No such pre-roll video") };
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    serve_file_as(Path::new(path), transcode::content_type(&ext), ctx.range.as_deref(), None)
}


const THEME_EXTS: [&str; 6] = ["mp3", "m4a", "flac", "ogg", "wav", "aac"];

/// A theme file's name without its ending, "theme-?[0-9]?": theme, theme1, theme-1.
fn is_theme_name(stem: &str) -> bool {
    let s = stem.to_lowercase();
    let Some(rest) = s.strip_prefix("theme") else { return false };
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    rest.is_empty() || (rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit())
}

/// The first theme file in a folder, by name.
fn theme_in(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_stem().is_some_and(|s| is_theme_name(&s.to_string_lossy())))
        .filter(|p| p.extension().is_some_and(|e| THEME_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str())))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.into_iter().next()
}

/// A theme file (theme.mp3, theme-1.mp3 …, also .m4a, .flac, .ogg, .wav, .aac) next to the first
/// file of a movie or show, or one folder above it (a show's season folder).
pub(crate) fn theme_path(db: &Db, item: &Item) -> Option<PathBuf> {
    let first = db.leaves(item.id).first().and_then(|id| db.items.get(id))?;
    let file = first.file.as_ref().map(|f| f.path.clone()).or_else(|| first.all_files().first().map(|f| f.path.clone()))?;
    let dir = Path::new(&file).parent()?;
    let mut dirs = vec![dir];
    if first.kind != ItemKind::Movie {
        if let Some(up) = dir.parent() { dirs.push(up); }
    }
    dirs.into_iter().find_map(theme_in)
}

/// GET /library/metadata/{id}/theme: the theme music of a movie or show, with seeking: a theme file in
/// its folder, else for a show the one cut from its intro during maintenance.
pub fn theme(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (local, show) = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)).filter(|i| db.can_see_item(user, i)) else { return fail(404, "Not found") };
        if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) { return fail(404, "No theme music"); }
        (theme_path(&db, it), (it.kind == ItemKind::Show && db.settings.theme_from_intros).then_some(it.id))
    };
    let cut = show.map(|id| app.data_dir.join(crate::admin::maintenance::THEME_DIR).join(format!("{id}.m4a"))).filter(|p| p.is_file());
    let Some(path) = local.filter(|p| app.may_send(p, false)).or(cut) else { return fail(404, "No theme music") };
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let ctype = match ext.as_str() { "mp3" => "audio/mpeg", "m4a" | "aac" => "audio/mp4", "flac" => "audio/flac", "ogg" => "audio/ogg", _ => "audio/wav" };
    serve_file_as(&path, ctype, ctx.range.as_deref(), None)
}


/// GET /library/metadata/{id}/chapters: [{index, title, start, end, thumb}] (ms), over all parts of a split movie.
pub fn chapters(app: &Arc<App>, user: &User, iid: &str) -> Resp {
    let (it, _) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    let ffprobe = app.settings().ffprobe;
    let (mut list, mut offset) = (vec![], 0u64);
    for f in it.all_files() {
        for (s, e, title) in scanner::read_chapters(&ffprobe, &f.path).unwrap_or_default() {
            list.push((offset + s, offset + e, title));
        }
        offset += f.duration_ms;
    }
    let out: Vec<Value> = list.iter().enumerate().map(|(i, (s, e, title))| {
        // The picture is taken a little after the start (a chapter often begins on a black frame).
        let at = chapter_picture_at(*s, *e);
        json!({"index": i + 1, "title": title, "start": s, "end": e, "thumb": format!("/library/parts/{}/frame?exact=1&t={at}", it.id)})
    }).collect();
    ok(json!({"MediaContainer": {"size": out.len(), "Chapter": out}}))
}

/// GET /library/metadata/{id}/lyrics: {synced, lines: [{t (ms, only when synced), text}]}, 404 without words.
pub fn lyrics(app: &Arc<App>, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    if it.kind != ItemKind::Track { return fail(404, "Only songs have lyrics"); }
    let ffprobe = app.settings().ffprobe;
    let found = crate::library::music::lyrics_of(Path::new(&f.path), || {
        let out = Command::new(&ffprobe).args(["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams"]).arg(&f.path).output().ok()?;
        serde_json::from_slice::<Value>(&out.stdout).ok().map(|v| crate::library::music::tags_of(&v))
    });
    match found {
        Some(l) => ok(json!({"synced": l.synced, "lines": l.lines.iter().map(|(t, w)| match t { Some(t) => json!({"t": t, "text": w}), None => json!({"text": w}) }).collect::<Vec<_>>()})),
        None => fail(404, "No lyrics"),
    }
}


#[cfg(test)]
mod theme_tests {
    use super::is_theme_name;

    #[test]
    fn theme_file_names() {
        for n in ["theme", "Theme", "theme1", "theme-2"] { assert!(is_theme_name(n), "{n}"); }
        for n in ["themes", "theme12", "theme-song", "my theme"] { assert!(!is_theme_name(n), "{n}"); }
    }
}
