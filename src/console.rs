//! A desktop status window for the server, drawn with macroquad: libraries,
//! continue watching, recently added, playlists, users and what is playing.
//! `--console` shows it live next to the running server; `--screenshot`
//! draws it about 120 frames, saves a PNG and exits. The full interface for
//! people is the web app the server serves; this window mirrors its home page,
//! and clicking anything in it (or "Open web app") opens that page in the browser.

use crate::db::{self, now, Db, Item, ItemKind, PlaylistKind, SectionKind};
use crate::server::{self, App};
use macroquad::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

const BG: Color = Color::new(0.075, 0.086, 0.102, 1.0);
const PANEL: Color = Color::new(0.11, 0.125, 0.145, 1.0);
const PANEL2: Color = Color::new(0.145, 0.16, 0.185, 1.0);
const ACCENT: Color = Color::new(0.949, 0.647, 0.086, 1.0);
const TEXT: Color = Color::new(0.93, 0.94, 0.95, 1.0);
const DIM: Color = Color::new(0.6, 0.63, 0.67, 1.0);
const GREEN: Color = Color::new(0.33, 0.78, 0.45, 1.0);

fn conf() -> Conf {
    Conf { window_title: crate::server::APP_NAME.into(), window_width: 1280, window_height: 800, high_dpi: false, window_resizable: true, ..Default::default() }
}

struct View {
    name: String,
    address: String,
    sections: Vec<(u64, String, SectionKind, usize, usize)>,
    continue_watching: Vec<(String, String, f32, u64)>,
    recent_movies: Vec<(String, String, u64)>,
    recent_episodes: Vec<(String, String, u64)>,
    playlists: Vec<(String, &'static str, usize)>,
    users: Vec<(String, bool, String)>,
    invites_open: usize,
    playing: Vec<String>,
    transcodes: usize,
    plugins: Vec<(String, bool)>,
    demo: bool,
}

fn view_of(db: &Db, playing: Vec<String>, transcodes: usize, plugins: Vec<(String, bool)>, address: String, demo: bool) -> View {
    let admin = db.users.iter().find(|u| u.admin).cloned().unwrap_or(db::User {
        id: 0, username: "owner".into(), email: String::new(), password_hash: String::new(), salt: String::new(), admin: true,
        created_at: 0, allowed_sections: None, invited_by: None, allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new(),
    });
    // Episodes show the show's name with "S1 E2" underneath; movies their year.
    let label = |i: &Item| -> (String, String, u64) {
        match i.kind {
            ItemKind::Episode => {
                let season = i.parent_id.and_then(|p| db.items.get(&p));
                let show = season.and_then(|s| s.parent_id).and_then(|p| db.items.get(&p));
                let title = show.map(|s| s.title.clone()).unwrap_or_else(|| i.title.clone());
                (title, format!("S{} E{} - {}", season.and_then(|s| s.index).unwrap_or(0), i.index.unwrap_or(0), i.title), show.map(|s| s.id).unwrap_or(i.id))
            }
            _ => (i.title.clone(), i.year.map(|y| y.to_string()).unwrap_or_default(), i.id),
        }
    };
    let pick = |ids: Vec<u64>| -> Vec<(String, String, u64)> { ids.iter().filter_map(|i| db.items.get(i)).map(label).collect() };
    View {
        name: crate::server::display_name(&db.settings),
        address,
        sections: db.sections.iter().map(|s| {
            let top = db.items.values().filter(|i| i.section_id == s.id && i.parent_id.is_none()).count();
            let vids = db.items.values().filter(|i| i.section_id == s.id && i.kind.playable() && !i.is_extra()).count();
            (s.id, s.name.clone(), s.kind, top, vids)
        }).collect(),
        continue_watching: server::continue_watching(db, &admin).iter().filter_map(|i| db.items.get(i)).map(|i| {
            let v = db.view(admin.id, i.id);
            let p = if i.duration_ms > 0 { v.view_offset_ms as f32 / i.duration_ms as f32 } else { 0.0 };
            let (t, s, id) = label(i);
            (t, s, p, id)
        }).collect(),
        recent_movies: pick(server::recently_added(db, &admin, Some(ItemKind::Movie), 12)),
        recent_episodes: pick(server::recently_added(db, &admin, Some(ItemKind::Episode), 12)),
        playlists: db.playlists.iter().filter(|p| p.owner_id == 0 || p.owner_id == admin.id).map(|p| {
            let kind = if p.is_default() { "default" } else if p.kind == PlaylistKind::Smart { "smart" } else { "manual" };
            (p.title.clone(), kind, crate::query::playlist_items(db, &admin, p).len())
        }).collect(),
        users: db.users.iter().map(|u| (u.username.clone(), u.admin, match &u.allowed_sections { None => "all libraries".to_string(), Some(l) => format!("{} libraries", l.len()) })).collect(),
        invites_open: db.invites.iter().filter(|i| i.used_by.is_none() && i.expires_at >= now()).count(),
        playing,
        transcodes,
        plugins,
        demo,
    }
}

fn snapshot(app: &Arc<App>) -> View {
    let db = app.db.lock().unwrap();
    let mut playing = vec![];
    for p in app.playing.lock().unwrap().values().chain(app.progressive.lock().unwrap().values()) {
        let who = db.user(p.user_id).map(|u| u.username.clone()).unwrap_or_default();
        let what = db.items.get(&p.item_id).map(|i| i.title.clone()).unwrap_or_default();
        playing.push(format!("{who}: {what} ({})", p.method));
    }
    let plugins = {
        let pl = app.plugins.lock().unwrap();
        pl.list.iter().map(|p| (p.manifest.name.clone(), db.plugins.get(&p.manifest.id).is_some_and(|s| s.enabled))).collect()
    };
    let port = *app.port.lock().unwrap();
    view_of(&db, playing, app.active_transcodes(), plugins, format!("http://{}:{port}", crate::lan_address()), false)
}

/// A made-up library so an empty server still shows what the window looks like.
fn demo_view(address: String) -> View {
    let mut db = Db::default();
    db.settings.server_name = crate::server::APP_NAME.into();
    let t = now();
    let add = |db: &mut Db, section: u64, kind: ItemKind, parent: Option<u64>, title: &str, year: Option<u32>, index: Option<u32>, age_days: u64| {
        let id = db.new_id();
        db.items.insert(id, Item {
            id, section_id: section, kind, parent_id: parent, title: title.into(), sort_title: db::sort_title(title), year, index,
            summary: String::new(), tagline: String::new(), genres: vec![], rating: None, content_rating: String::new(), studio: String::new(),
            added_at: t - age_days * 86400, updated_at: t, duration_ms: if kind.playable() { 6_000_000 } else { 0 }, file: None, parts: vec![], poster: None,
            extra: Default::default(), locked: vec![],
        });
        id
    };
    db.sections.push(db::Section { id: 9001, name: "Movies".into(), kind: SectionKind::Movie, paths: vec!["/volume1/video/Movies".into()], created_at: t, scanned_at: t, credits_detection: true, language: String::new() });
    db.sections.push(db::Section { id: 9002, name: "TV Shows".into(), kind: SectionKind::Show, paths: vec!["/volume1/video/TV".into()], created_at: t, scanned_at: t, credits_detection: true, language: String::new() });
    let movies = [("Northern Lights", 2021), ("The Long Road", 2019), ("Harbor City", 2023), ("Quiet Signal", 2017), ("Paper Moons", 2022), ("Iron Valley", 2015), ("Summer at Sea", 2020), ("Last Orbit", 2024)];
    let mut ids = vec![];
    for (n, (title, year)) in movies.iter().enumerate() {
        ids.push(add(&mut db, 9001, ItemKind::Movie, None, title, Some(*year), None, n as u64 * 3));
    }
    let mut eps = vec![];
    for (s, show) in ["Station Eleven Lane", "Kitchen Rules", "Deep Blue"].iter().enumerate() {
        let sh = add(&mut db, 9002, ItemKind::Show, None, show, None, None, 2);
        let se = add(&mut db, 9002, ItemKind::Season, Some(sh), "Season 1", None, Some(1), 2);
        for e in 1..=4 {
            eps.push(add(&mut db, 9002, ItemKind::Episode, Some(se), &format!("Episode {e}"), None, Some(e), (s * 4) as u64 + e as u64));
        }
    }
    db.users.push(db::User { id: 1, username: "owner".into(), email: String::new(), password_hash: String::new(), salt: String::new(), admin: true, created_at: t, allowed_sections: None, invited_by: None, allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new() });
    db.users.push(db::User { id: 2, username: "family".into(), email: String::new(), password_hash: String::new(), salt: String::new(), admin: false, created_at: t, allowed_sections: Some(vec![9001]), invited_by: Some(1), allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new() });
    for (i, frac) in [(ids[1], 0.42), (eps[1], 0.7), (ids[4], 0.15)] {
        let v = db.view_mut(1, i);
        v.view_offset_ms = (6_000_000.0 * frac) as u64;
        v.last_viewed_at = t - (frac * 1000.0) as u64;
    }
    db.view_mut(1, eps[0]).view_count = 1;
    db.view_mut(1, ids[2]).favorite = true;
    let pid = db.new_id();
    db.playlists.push(db::Playlist { id: pid, owner_id: 1, title: "Movie night".into(), summary: String::new(), kind: PlaylistKind::Manual, items: vec![ids[0], ids[3], ids[5]], query: None, default_key: None, shared: true, pinned: true, created_at: t, updated_at: t });
    let pid = db.new_id();
    db.playlists.push(db::Playlist { id: pid, owner_id: 1, title: "Short episodes".into(), summary: String::new(), kind: PlaylistKind::Smart, items: vec![], query: Some(db::SmartQuery { match_all: true, rules: vec![db::rule("kind", "is", serde_json::json!("episode"))], sort: "added_at:desc".into(), limit: Some(25), sections: vec![] }), default_key: None, shared: false, pinned: false, created_at: t, updated_at: t });
    db.invites.push(db::Invite { code: "demo".into(), email: "friend@example.com".into(), note: String::new(), created_by: 1, created_at: t, expires_at: t + 7 * 86400, allowed_sections: None, admin: false, used_by: None, used_at: None });
    view_of(&db, vec!["family: The Long Road (direct)".into(), "owner: Kitchen Rules S1E2 (transcode 720p)".into()], 1, vec![("NFO reader".into(), true), ("Webhook notify".into(), false)], address, true)
}

fn poster_color(id: u64, light: bool) -> Color {
    let hue = (id.wrapping_mul(47) % 360) as f32 / 360.0;
    let (s, l) = if light { (0.45, 0.38) } else { (0.5, 0.14) };
    macroquad::color::hsl_to_rgb(hue, s, l)
}

fn fit(text: &str, size: f32, width: f32) -> String {
    let mut t = text.to_string();
    while t.chars().count() > 1 && measure_text(&t, None, size as u16, 1.0).width > width {
        t.pop();
        let trimmed = t.trim_end().to_string();
        t = trimmed;
        if measure_text(&format!("{t}.."), None, size as u16, 1.0).width <= width {
            return format!("{t}..");
        }
    }
    t
}

/// Clickable areas of the window and the web-app page each one opens.
type Targets = Vec<(Rect, String)>;

/// Only the live window reacts to the mouse (the saved picture shouldn't depend on it).
static INTERACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn hovered(r: Rect) -> bool {
    if !INTERACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let (x, y) = mouse_position();
    r.contains(vec2(x, y))
}

fn poster(x: f32, y: f32, w: f32, h: f32, id: u64, title: &str, sub: &str, progress: Option<f32>) {
    // Two-tone gradient poster (procedural, like the server's placeholder pictures).
    let steps = 24;
    let top = poster_color(id, true);
    let bottom = poster_color(id + 40, false);
    for i in 0..steps {
        let f = i as f32 / steps as f32;
        let c = Color::new(top.r + (bottom.r - top.r) * f, top.g + (bottom.g - top.g) * f, top.b + (bottom.b - top.b) * f, 1.0);
        draw_rectangle(x, y + h * f, w, h / steps as f32 + 1.0, c);
    }
    let initial: String = title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let d = measure_text(&initial, None, 46, 1.0);
    draw_text(&initial, x + (w - d.width) / 2.0, y + h * 0.5 + 14.0, 46.0, Color::new(1.0, 1.0, 1.0, 0.85));
    if let Some(p) = progress {
        draw_rectangle(x, y + h - 5.0, w, 5.0, Color::new(0.0, 0.0, 0.0, 0.6));
        draw_rectangle(x, y + h - 5.0, w * p.clamp(0.0, 1.0), 5.0, ACCENT);
    }
    if hovered(Rect::new(x, y, w, h + 40.0)) {
        draw_rectangle_lines(x - 2.0, y - 2.0, w + 4.0, h + 4.0, 3.0, ACCENT);
    }
    draw_text(&fit(title, 17.0, w), x, y + h + 18.0, 17.0, TEXT);
    draw_text(&fit(sub, 15.0, w), x, y + h + 35.0, 15.0, DIM);
}

fn row_title(x: f32, y: f32, t: &str) {
    draw_text(t, x, y, 22.0, TEXT);
}

fn draw(v: &View, frame: u32) -> Targets {
    let mut targets: Targets = vec![];
    clear_background(BG);
    let w = screen_width();
    let h = screen_height();

    // top bar
    draw_rectangle(0.0, 0.0, w, 56.0, PANEL);
    draw_triangle(vec2(22.0, 16.0), vec2(22.0, 40.0), vec2(42.0, 28.0), ACCENT);
    draw_text(&v.name, 56.0, 36.0, 26.0, TEXT);
    let pulse = 0.6 + 0.4 * ((frame as f32) * 0.08).sin().abs();
    let status = format!("Running - {}", v.address);
    let sw = measure_text(&status, None, 18, 1.0).width;
    draw_circle(w - sw - 38.0, 28.0, 6.0, Color::new(GREEN.r, GREEN.g, GREEN.b, pulse));
    draw_text(&status, w - sw - 24.0, 34.0, 18.0, DIM);
    // "Open web app" button: the full interface is in the browser
    let label = "Open web app";
    let bw = measure_text(label, None, 18, 1.0).width + 28.0;
    let button = Rect::new(w - sw - 58.0 - bw, 12.0, bw, 32.0);
    let hot = hovered(button);
    draw_rectangle(button.x, button.y, button.w, button.h, if hot { Color::new(1.0, 0.75, 0.25, 1.0) } else { ACCENT });
    draw_text(label, button.x + 14.0, button.y + 22.0, 18.0, BG);
    targets.push((button, String::new()));
    // search box
    let search = Rect::new(w * 0.34, 13.0, (button.x - 24.0 - w * 0.34).min(w * 0.22).max(60.0), 30.0);
    draw_rectangle(search.x, search.y, search.w, search.h, if hovered(search) { PANEL2 } else { Color::new(0.13, 0.145, 0.17, 1.0) });
    draw_text(&fit("Search movies and shows", 17.0, search.w - 20.0), search.x + 12.0, 33.0, 17.0, DIM);
    targets.push((search, String::new()));

    // sidebar
    let sx = 0.0;
    draw_rectangle(sx, 56.0, 210.0, h - 56.0, PANEL);
    let mut y = 96.0;
    let mut item = |label: &str, active: bool, badge: String, route: String, y: &mut f32| {
        let r = Rect::new(0.0, *y - 22.0, 210.0, 34.0);
        let hot = hovered(r);
        if active || hot {
            draw_rectangle(r.x, r.y, r.w, r.h, PANEL2);
        }
        if active {
            draw_rectangle(0.0, *y - 22.0, 4.0, 34.0, ACCENT);
        }
        draw_text(label, 24.0, *y, 19.0, if active || hot { TEXT } else { DIM });
        if !badge.is_empty() {
            let d = measure_text(&badge, None, 15, 1.0);
            draw_text(&badge, 194.0 - d.width, *y, 15.0, DIM);
        }
        targets.push((r, route));
        *y += 40.0;
    };
    item("Home", true, String::new(), String::new(), &mut y);
    for (id, name, kind, top, vids) in &v.sections {
        let badge = match kind { SectionKind::Show => format!("{top} shows"), _ => format!("{vids}") };
        item(name, false, badge, format!("section/{id}"), &mut y);
    }
    item("Playlists", false, v.playlists.len().to_string(), "playlists".into(), &mut y);
    y += 14.0;
    draw_text("MANAGE", 24.0, y, 14.0, DIM);
    y += 30.0;
    item("Libraries", false, String::new(), "libraries".into(), &mut y);
    item("Users & invites", false, v.users.len().to_string(), "users".into(), &mut y);
    item("Plugins", false, v.plugins.len().to_string(), "plugins".into(), &mut y);
    item("Settings", false, String::new(), "settings".into(), &mut y);
    item("API", false, String::new(), "api".into(), &mut y);
    draw_text("Click anything to open", 24.0, h - 36.0, 14.0, DIM);
    draw_text("it in your browser", 24.0, h - 18.0, 14.0, DIM);

    // main rows
    let mx = 236.0;
    let right_w = 300.0;
    let main_w = w - mx - right_w - 30.0;
    let pw = if h < 860.0 { 100.0 } else { 112.0 };
    let ph = if h < 860.0 { 150.0 } else { 168.0 };
    let gap = 18.0;
    let per_row = ((main_w + gap) / (pw + gap)).floor().max(1.0) as usize;
    let mut y = 96.0;
    if v.sections.is_empty() {
        row_title(mx, y, "Welcome! Add a library to get started");
        draw_text("Click \"Open web app\" above, sign in, then go to Libraries > Add.", mx, y + 30.0, 18.0, DIM);
        y += 80.0;
    }
    let mut row = |title: &str, list: Vec<(&String, &String, Option<f32>, u64)>, y: &mut f32| {
        row_title(mx, *y, title);
        for (i, (t, s, p, id)) in list.into_iter().take(per_row).enumerate() {
            let x = mx + i as f32 * (pw + gap);
            poster(x, *y + 14.0, pw, ph, id, t, s, p);
            targets.push((Rect::new(x, *y + 14.0, pw, ph + 40.0), format!("item/{id}")));
        }
        *y += ph + 72.0;
    };
    if !v.continue_watching.is_empty() {
        row("Continue Watching", v.continue_watching.iter().map(|(t, s, p, id)| (t, s, Some(*p), *id)).collect(), &mut y);
    }
    if !v.recent_movies.is_empty() {
        row("Recently Added Movies", v.recent_movies.iter().map(|(t, s, id)| (t, s, None, *id)).collect(), &mut y);
    }
    if !v.recent_episodes.is_empty() && y + ph + 70.0 < h {
        row("Recently Added TV", v.recent_episodes.iter().map(|(t, s, id)| (t, s, None, *id)).collect(), &mut y);
    }

    // right column
    let rx = w - right_w - 14.0;
    let mut ry = 72.0;
    let mut card = |title: &str, lines: &[(String, Color)], route: &str, ry: &mut f32| {
        let hgt = 44.0 + lines.len() as f32 * 24.0;
        let r = Rect::new(rx, *ry, right_w, hgt);
        draw_rectangle(r.x, r.y, r.w, r.h, if hovered(r) { PANEL2 } else { PANEL });
        draw_text(title, rx + 14.0, *ry + 28.0, 19.0, ACCENT);
        for (i, (l, c)) in lines.iter().enumerate() {
            draw_text(&fit(l, 16.0, right_w - 28.0), rx + 14.0, *ry + 54.0 + i as f32 * 24.0, 16.0, *c);
        }
        targets.push((r, route.to_string()));
        *ry += hgt + 12.0;
    };
    let mut now_lines: Vec<(String, Color)> = v.playing.iter().map(|p| (p.clone(), TEXT)).collect();
    if now_lines.is_empty() {
        now_lines.push(("Nothing is playing".into(), DIM));
    }
    now_lines.push((format!("{} transcode(s) running", v.transcodes), DIM));
    card("Now Playing", &now_lines, "", &mut ry);
    let pl: Vec<(String, Color)> = v.playlists.iter().take(8).map(|(t, k, n)| (format!("{t}  ({n}){}", match *k { "default" => "", "smart" => "  - smart", _ => "" }), if *k == "default" { DIM } else { TEXT })).collect();
    card("Playlists", &pl, "playlists", &mut ry);
    let mut us: Vec<(String, Color)> = v.users.iter().map(|(n, a, s)| (format!("{n}{} - {s}", if *a { " (admin)" } else { "" }), TEXT)).collect();
    us.push((format!("{} open invite(s)", v.invites_open), DIM));
    card("Users & invites", &us, "users", &mut ry);
    if ry < h - 80.0 {
        let mut pls: Vec<(String, Color)> = v.plugins.iter().map(|(n, on)| (format!("{n}  ({})", if *on { "on" } else { "off" }), if *on { TEXT } else { DIM })).collect();
        if pls.is_empty() {
            pls.push(("No plugins installed".into(), DIM));
        }
        card("Plugins", &pls, "plugins", &mut ry);
    }
    if v.demo {
        draw_text("Sample library shown - this server has no libraries yet", mx, h - 16.0, 15.0, DIM);
    }
    targets
}

pub fn screenshot(out: PathBuf, opts: crate::Options) {
    // Read the real library if there is one; otherwise show the sample.
    let saved = crate::store::read_library(&opts.data);
    let port = opts.port.or(saved.as_ref().map(|d| d.settings.port)).unwrap_or(crate::server::DEFAULT_PORT);
    let address = format!("http://{}:{}", crate::lan_address(), port);
    let view = if let Some(db) = saved {
        if db.sections.is_empty() { demo_view(address) } else { view_of(&db, vec![], 0, vec![], address, false) }
    } else {
        demo_view(address)
    };
    macroquad::Window::from_config(conf(), async move {
        for frame in 0..120u32 {
            draw(&view, frame);
            next_frame().await;
        }
        draw(&view, 120);
        let img = get_screen_data();
        if let Some(dir) = out.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        img.export_png(&out.to_string_lossy());
        println!("saved {}", out.display());
        std::process::exit(0);
    });
}

pub fn live(app: Arc<App>) {
    INTERACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    macroquad::Window::from_config(conf(), async move {
        let mut frame = 0u32;
        let mut view = snapshot(&app);
        let port = *app.port.lock().unwrap();
        loop {
            if frame % 60 == 0 {
                view = snapshot(&app);
            }
            let targets = draw(&view, frame);
            if is_mouse_button_pressed(MouseButton::Left) {
                let (x, y) = mouse_position();
                // The last-drawn area wins (it's on top).
                if let Some((_, route)) = targets.iter().rev().find(|(r, _)| r.contains(vec2(x, y))) {
                    crate::open_browser(&format!("http://localhost:{port}/#/{route}"));
                }
            }
            frame = frame.wrapping_add(1);
            next_frame().await;
        }
    });
}
