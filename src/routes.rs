//! Every API address. GET /api/docs returns the ROUTES table.

use crate::auth;
use crate::db::{self, now, Invite, ItemKind, Playlist, PlaylistKind, Section, SectionKind, SmartQuery};
use crate::handlers::*;
use crate::plugins;
use crate::query;
use crate::related;
use crate::server::*;
use crate::transcode::{self, Method};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// (method, path, what it does).
pub const ROUTES: &[(&str, &str, &str)] = &[
    ("GET", "/api/status", "Server name, version and whether first-time setup is needed (no sign-in)"),
    ("POST", "/api/setup", "First start only: create the owner account {username, password}"),
    ("POST", "/api/login", "Sign in {username, password} → {token}; send the token as the X-Token header, Authorization: Bearer, or ?token="),
    ("POST", "/api/logout", "Forget this token"),
    ("GET", "/api/home", "Home profiles to pick on the sign-in screen (only answered in the local network; no sign-in)"),
    ("POST", "/api/home/{id}/login", "Sign in as a home profile {pin} → {token} (local network only; five wrong PINs lock it for five minutes)"),
    ("GET", "/api/tokens", "Your tokens (devices and scripts) by short id; the token itself is never shown again"),
    ("POST", "/api/tokens", "Make an extra token for a script {device} → token (shown once)"),
    ("POST", "/api/tokens/rotate", "Swap the token you are using for a new one; the old one stops working. {all: true} also signs out every other device"),
    ("DELETE", "/api/tokens/{id}", "Revoke one of your tokens"),
    ("GET", "/api/me", "Who is signed in"),
    ("POST", "/api/me/password", "Change your password {old, new}"),
    ("GET", "/api/docs", "This list"),
    ("GET", "/api/filters", "Fields, operators and sort keys for filters and smart playlists"),
    ("GET", "/identity", "Server id and version (no sign-in)"),
    ("GET", "/library/sections", "Libraries you may see (movies and TV shows are separate libraries)"),
    ("POST", "/library/sections", "Admin: add a library {name, type: movie|show|other, paths: [..], language}"),
    ("GET", "/library/sections/{id}", "One library"),
    ("PUT", "/library/sections/{id}", "Admin: change {name, paths, credits_detection, intro_detection, scan_interval_minutes (null = the server's, 0 = never), language} (language: details language for this library, empty = the server's; changing it downloads the details again)"),
    ("DELETE", "/library/sections/{id}", "Admin: remove a library (files stay on disk)"),
    ("POST", "/library/sections/{id}/refresh", "Admin: scan the library now (?wait=1 to wait for the result, ?path= to scan only one folder of it)"),
    ("POST", "/library/sections/{id}/analyze", "Admin: read every file of the library again with ffprobe (in the background)"),
    ("POST", "/library/sections/{id}/deepanalyze", "Admin: deep analysis of the library: read every packet to work out the connection speed each file needs (in the background)"),
    ("POST", "/library/sections/{id}/emptyTrash", "Admin: take the unavailable items (files gone from disk) out of the library for good"),
    ("GET", "/library/sections/{id}/all", "Browse: ?type=movie|show|episode &sort=title|year:desc|added_at:desc|rating:desc|random &genre=Drama &year>=2000 &unwatched=1 &title~=star &q= &offset &limit"),
    ("GET", "/library/sections/{id}/genres", "Genres and years in a library with counts"),
    ("GET", "/library/sections/{id}/collections", "The library's collections, each with its size, a picture and the address of its items"),
    ("GET", "/library/sections/{id}/categories", "The library's categories (one per genre) with a picture and size"),
    ("GET", "/library/metadata/{id}/related", "Rows for a movie or show page: Other Editions, Related Movies/Shows, More by <first director>, More with <first two actors> (?count=, default 6)"),
    ("GET", "/library/sections/{id}/filters", "Every filter the library page offers (?type=show|season|episode), with the values found and counts (labels and folders for the owner only; duplicates with ?includeAdvanced=1); use them in /all as field=value (several: a,b)"),
    ("GET", "/library/metadata/{id}", "One movie, show, season or episode with media details"),
    ("PUT", "/library/metadata/{id}", "Admin: edit {language_override, credits_detection (on/off/empty = library), title, sort_title, summary, year, genres, rating, content_rating, tagline, studio, original_title, originally_available, directors, writers, producers, countries, labels, collections, edition}"),
    ("GET", "/library/metadata/{id}/children", "Seasons of a show, episodes of a season"),
    ("GET", "/library/metadata/{id}/allLeaves", "Every episode of a show"),
    ("GET", "/library/metadata/{id}/thumb", "Poster picture"),
    ("GET", "/library/metadata/{id}/art", "Wide background picture (from the online details)"),
    ("GET", "/library/metadata/{id}/matches", "Admin: search online details for a movie or show ?title= &year= (for Fix match)"),
    ("PUT", "/library/metadata/{id}/match", "Admin: use this online match {source: tmdb|tvmaze|wikidata, id} → plot, cast, posters, episodes"),
    ("DELETE", "/library/metadata/{id}/match", "Admin: unmatch: forget the online details and go back to the file names (also PUT …/unmatch); automatic matching then leaves it alone until a new match"),
    ("POST", "/library/metadata/{id}/refresh", "Admin: download the details again"),
    ("GET", "/library/metadata/{id}/cast/{n}", "Photo of the n-th cast member (older downloads)"),
    ("GET", "/library/people/{key}", "An actor and every movie and show you can see that they are in (with their role) ?name= "),
    ("GET", "/library/people/{key}/media", "The same list at a second address"),
    ("GET", "/library/people/{key}/thumb","An actor's photo; one photo per person, shared by everything they are in"),
    ("GET", "/library/metadata/{id}/markers", "Credits markers (ms), where they came from and where the video counts as watched"),
    ("GET", "/library/metadata/{id}/chapters", "The chapters of a movie or episode [{index, title, start, end (ms), thumb}]; thumb is a picture from the chapter"),
    ("PUT", "/library/metadata/{id}/markers", "Admin: set credits markers {credits: [ms, …]} or {credits: null} to use the chapters again"),
    ("POST", "/library/sections/{id}/credits", "Admin: run credits detection for a library now (in the background; see /api/metadata/status)"),
    ("POST", "/library/metadata/{id}/credits", "Admin: run credits detection for one movie, show, season or episode"),
    ("POST", "/library/sections/{id}/intros", "Admin: run intro detection for a TV library now (compares the sound at the start of each season's episodes; in the background)"),
    ("POST", "/library/metadata/{id}/intros", "Admin: run intro detection for one show, season or episode (its whole season)"),
    ("POST", "/library/sections/{id}/refreshMetadata", "Admin: download details again for a whole library (in the background)"),
    ("GET", "/api/metadata/status", "Background work: details waiting and being downloaded, the last result, and any running job (credits detection) with progress"),
    ("GET", "/library/metadata/{id}/history", "Play history of a movie, show, season or episode, newest first (the owner sees everyone's, others their own)"),
    ("POST", "/library/metadata/{id}/report", "Report a problem {kind: playback|video|audio|subtitles|metadata|other, note}"),
    ("DELETE", "/library/metadata/{id}", "Delete the files of a movie or episode (or every episode of a season or show) from disk; needs media deletion allowed in the settings and, for others than admins, the right to delete"),
    ("POST", "/library/metadata/{id}/analyze", "Admin: read the files again with ffprobe (streams, length; in the background)"),
    ("POST", "/library/metadata/{id}/deepanalyze", "Admin: deep analysis: read every packet to work out the connection speed the files need (in the background)"),
    ("POST", "/library/metadata/{id}/optimize", "Admin: save a version every browser plays directly {quality: original|tv|mobile} (in the background; played instead of converting)"),
    ("DELETE", "/library/metadata/{id}/optimize", "Admin: delete the optimized versions"),
    ("GET", "/api/log", "Admin: the server's recent messages (the last 500) → {lines: [{t, msg}]}, oldest first"),
    ("GET", "/api/optimized", "Admin: every optimized version → {items: [{id, title, quality, size}]}"),
    ("DELETE", "/api/optimized", "Admin: delete all optimized versions → {removed}"),
    ("POST", "/api/maintenance/optimize", "Admin: compact the library file → {before, after} bytes"),
    ("POST", "/api/maintenance/cleanup", "Admin: remove leftover conversions and old cache files → {freed} bytes"),
    ("GET", "/api/log/download", "Admin: the whole server log as a text file (?gz=1: compressed)"),
    ("GET", "/api/maintenance/backups", "Admin: the backups in the backup folder → {backups: [{name, size, time}]}, newest first"),
    ("POST", "/api/maintenance/restore", "Admin: put a backup back in place of the library {name}; the current state is backed up first"),
    ("GET", "/library/sections/{id}/firstCharacter", "How many items start with each letter, with the filters of /all → {Directory: [{key, size}]}"),
    ("POST", "/api/maintenance/backup", "Admin: save a copy of the library file into the backup folder now → {path}"),
    ("GET", "/api/maintenance/database", "Admin: download a copy of the library file"),
    ("GET", "/api/maintenance/report", "Admin: download a text report (version, system, libraries, settings without keys, plugin messages)"),
    ("GET", "/api/reports", "Admin: reported problems ?open=1"),
    ("PUT", "/api/reports/{id}", "Admin: {resolved: true|false}"),
    ("DELETE", "/api/reports/{id}", "Admin: delete a report"),
    ("POST", "/api/together", "Watch together: open a room {item, position} → {code}; send the link #/together/{code}"),
    ("GET", "/api/together/{code}", "A room's state {item, position (ms), paused, seq, members}; asking keeps you in it"),
    ("POST", "/api/together/{code}", "Play, pause, seek or switch video for everyone {position, paused, item}"),
    ("DELETE", "/api/together/{code}", "Leave a room"),
    ("GET", "/library/metadata/{id}/decision", "How it would play: direct, remux or transcode ?quality=720p-4"),
    ("GET", "/library/parts/{id}/file", "The original file, with seeking (Range requests); ?download=1"),
    ("GET", "/library/parts/{id}/subtitles/{n}", "A subtitle track as WebVTT (n, or xN for side files), with its timing set by synchronizing or by hand"),
    ("POST", "/library/parts/{id}/subtitles/{n}/sync", "Admin: listen to the sound and move the subtitle track so its lines fall on the speech (also fixes 23.976 / 25 fps stretches); only kept when certain. Takes a while"),
    ("PUT", "/library/parts/{id}/subtitles/{n}/timing", "Admin: set the timing of a subtitle track {offset_ms, scale} or move it {delta_ms}, or {reset: true}"),
    ("GET", "/library/parts/{id}/frame", "A small still for the player's seek bar ?t=seconds (rounded to 10 s, cached)"),
    ("GET", "/video/transcode/{id}/stream.mp4", "Converted stream for any container ?quality= &offset=seconds &audio=streamIndex &transcode=1 &level=4.1 (H.264 level the player needs)"),
    ("GET", "/video/transcode/{id}/start.m3u8", "HLS (MPEG-TS, H.264/AAC) stream ?quality= &offset= &level=4.1 (redirects to the playlist)"),
    ("GET", "/status/sessions", "What is playing right now"),
    ("GET", "/video/transcode/sessions", "Admin: running HLS transcodes"),
    ("DELETE", "/video/transcode/sessions/{id}", "Stop a transcode"),
    ("POST", "/:/timeline", "Report progress ?ratingKey= &time=ms &state=playing|paused|stopped"),
    ("POST", "/:/scrobble", "Mark watched ?key= (several: key=1,2,3)"),
    ("POST", "/:/unscrobble", "Mark unwatched ?key= (several: key=1,2,3)"),
    ("POST", "/:/rate", "Your rating ?key= &rating=0-10"),
    ("PUT", "/api/items/{id}/favorite", "{favorite: true|false}"),
    ("PUT", "/api/items/{id}/watchlist", "Put a movie, show, season or episode on your watchlist or take it off {watchlist: true|false}; watching it takes it off by itself (a show once every episode is watched)"),
    ("POST", "/api/me/prefs", "Settings that follow you from browser to browser {key: text, …}; null removes a key (GET /api/me returns them as prefs)"),
    ("POST", "/api/me/profile", "Your full name {full_name}"),
    ("POST", "/api/me/2fa/setup", "Start two-step sign-in → {secret, uri, qr (an SVG picture of the link)} for an authenticator app (not active yet)"),
    ("POST", "/api/me/2fa/enable", "Switch it on with a code from the app {code}"),
    ("POST", "/api/me/2fa/disable", "Switch it off {password, code}"),
    ("GET", "/library/metadata/{id}/theme", "The theme music of a movie or show: a theme.mp3 or theme-1.mp3 (also .m4a .flac .ogg .wav .aac) in its folder, else for a show the online list by its TheTVDB number; 404 when there is none"),
    ("GET", "/api/preroll", "A video to play before a movie (from the pre-roll list in the settings) → {url} or {url: null}"),
    ("GET", "/api/preroll/{n}/file", "One of the pre-roll videos, with seeking"),
    ("GET", "/api/rss", "Your watchlist feed {enabled, url}; the address holds a private key, so a feed reader needs no sign-in"),
    ("POST", "/api/rss", "{enabled: true|false, regenerate: true} switch your watchlist feed on or off, or make a new address"),
    ("GET", "/rss/watchlist/{key}.xml", "Public: the watchlist as RSS (the key is the private part of the address)"),
    ("GET", "/api/watchlist", "Your watchlist ?sort=added (newest first)|oldest|title|year|rating|added_at &type=movie|show|season|episode"),
    ("POST", "/api/watchlist", "Put several on your watchlist or take them off {ids: [..], watchlist: true|false}"),
    ("GET", "/api/downloads", "Your downloads, newest first, with state (queued|converting|ready|failed), progress and size ?all=1 (admin: everyone's)"),
    ("POST", "/api/downloads", "Download movies or episodes (a season or show: every episode) {ids: [..], quality: original|1080p-8|720p-4|…}; the original, or a file that already fits, is ready at once, others are converted in the background"),
    ("GET", "/api/downloads/{id}/file", "Save a ready download (?part=n for a movie in parts, original quality)"),
    ("POST", "/api/downloads/{id}/retry", "Try a failed download again"),
    ("DELETE", "/api/downloads/{id}", "Remove a download (stops its conversion, deletes the converted copy)"),
    ("DELETE", "/api/downloads", "Remove several ?state=finished (ready and failed)|ready|failed|all &all=1 (admin: everyone's)"),
    ("GET", "/api/dashboard", "Admin: what is playing (who, how, bandwidth), processor and memory of the computer and the server, bytes sent, the last hour in samples, users and who is active"),
    ("GET", "/api/dashboard/stats", "Admin: bandwidth (all and outside the local network), processor and memory (computer and server) and streams over ?range=realtime|12h|24h|7d|30d|90d|365d|all, as samples on an even grid"),
    ("POST", "/api/dashboard/stop", "Admin: stop what a user is playing {user, item, message}: converted streams end at once, the player shows the message on its next report, and the same video can't start again for 30 s unless the player heard it"),
    ("GET", "/api/dashboard/history", "Admin: viewings of the last days ?days=30 &user=id as [[at, user, kind, minutes], …] to count per day, plus most played titles, top users and the latest viewings"),
    ("GET", "/hubs", "Home rows: continue watching, rows for the time of year (Halloween, Christmas, New Year's Eve, Valentine's Day; ?date=MM-DD tries a day), recently added movies and TV, pinned playlists"),
    ("GET", "/hubs/home/continueWatching", "Started but not finished, plus next episodes"),
    ("GET", "/hubs/home/onDeck", "Next episode of shows you are watching"),
    ("GET", "/hubs/home/recentlyAdded", "Newest items ?type=movie|episode &limit="),
    ("GET", "/hubs/search", "Search titles and genres ?query="),
    ("GET", "/playlists", "Playlists: filter ?kind=default|smart|manual &smart=1 &owner=me|others|server &title=… &min_items= &pinned=1; sort ?sort=title|updated_at:desc|item_count:desc|duration|created_at"),
    ("POST", "/playlists", "Create {title, summary, kind: manual|smart, ids: [..], query: {match_all, rules: [{field, op, value}], sort, limit, sections}, shared, pinned}"),
    ("POST", "/playlists/preview", "Try smart rules without saving {query}"),
    ("GET", "/playlists/{id}", "One playlist"),
    ("PUT", "/playlists/{id}", "Change {title, summary, query, shared, pinned}"),
    ("DELETE", "/playlists/{id}", "Delete (default playlists can only be unpinned)"),
    ("GET", "/playlists/{id}/items", "Contents, with the same filters and sorts as browsing (no sort or ?sort=playlist keeps your order)"),
    ("POST", "/playlists/{id}/items", "Add {ids: [..]} or ?ids=1,2 (a show or season adds all its episodes)"),
    ("DELETE", "/playlists/{id}/items/{itemId}", "Remove one entry (a show or season removes all its episodes)"),
    ("DELETE", "/playlists/{id}/items", "Remove several ?ids=1,2 (or {ids: [..]}), or everything ?all=1"),
    ("POST", "/playlists/{id}/items/remove", "Same as above for clients that can't send a body with DELETE {ids: [..]} or {all: true}"),
    ("PUT", "/playlists/{id}/items/{itemId}/move", "Reorder ?after=itemId (0 = to the top)"),
    ("POST", "/playlists/{id}/copy", "Save a playlist's current contents as a new manual playlist"),
    ("GET", "/api/users", "Admin: every user"),
    ("POST", "/api/users", "Admin: add a local user directly, without an invite {username, password, email, admin, allowed_sections: [ids]|null, home, pin} (a home profile needs no password)"),
    ("PATCH", "/api/users/{id}", "Admin: {admin, allowed_sections: [ids]|null, allow_transcode, allow_delete, allow_download, reset_2fa (true: switch off their two-step sign-in), max_age (age limit, null = none), block_unrated, password, email, home, pin (empty removes it)}"),
    ("DELETE", "/api/users/{id}", "Admin: remove a user"),
    ("DELETE", "/api/users/{id}/tokens", "Admin: sign a user out everywhere"),
    ("GET", "/api/invites", "Admin: open and used invites"),
    ("POST", "/api/invites", "Admin: invite someone {email, note, days, allowed_sections, admin} → link to send"),
    ("DELETE", "/api/invites/{code}", "Admin: cancel an invite"),
    ("GET", "/api/invites/{code}", "Check an invite (no sign-in)"),
    ("POST", "/api/invites/{code}/accept", "Join with an invite {username, password, email} → token (no sign-in)"),
    ("GET", "/api/plugins", "Admin: installed plugins, their hooks, settings and errors"),
    ("POST", "/api/plugins/reload", "Admin: look for new or changed plugins"),
    ("POST", "/api/plugins/{id}/enable", "Admin: switch a plugin on"),
    ("POST", "/api/plugins/{id}/disable", "Admin: switch a plugin off"),
    ("PUT", "/api/plugins/{id}/settings", "Admin: plugin settings {..}"),
    ("GET", "/api/plugins/log", "Admin: recent plugin runs"),
    ("GET", "/api/plugins/panels", "Switched-on plugins that give each person a panel in Settings (e.g. Connect Trakt)"),
    ("POST", "/api/plugins/{id}/call/{action}", "Run a plugin's own action (any JSON body is passed on); an answer with mark_watched marks those items watched for you"),
    ("GET", "/api/settings", "Admin: server settings"),
    ("PUT", "/api/settings", "Admin: change {server_name, hw_accel: auto|none|vaapi|qsv|nvenc, transcoder_quality: automatic|speed|quality|max, h264_preset, h264_minimum_crf, hw_constant_quality, scheduled_scans_enabled, scan_interval_minutes, watched_threshold_percent, played_at_behaviour: 0-3, credits_markers_behaviour: never|scheduled|asap, max_transcodes, continue_watching_weeks, continue_watching_limit, continue_watching_include_premieres, metadata_enabled, metadata_language (see languages), cast_photos_limit, tmdb_api_key, imdb_ratings_file, watch_folders, credits_from_picture, port (after a restart), public_url, home_profiles, trusted_proxies, ffmpeg, ffprobe, transcoding_enabled, group_recent_episodes, preroll_paths, max_gpu_transcodes, max_cpu_transcodes, transcode_dir, downloads_dir, remote_max_mbps (0 = no limit for people outside the home network), max_remote_plays, paused_stop_minutes, no_auth_networks, maintenance_start_hour, maintenance_end_hour, utc_offset_minutes, backup_enabled, backup_dir, backups_keep, optimize_weekly, cleanup_weekly, deep_analysis}"),
    ("GET", "/api/transcoder", "Admin: is ffmpeg there, which hardware encoders"),
    ("GET", "/api/browse", "Admin: list folders on the server ?path= (to pick library folders)"),
    ("POST", "/api/imdb-ratings/refresh", "Admin: download IMDb's ratings file now and update every rating (in the background; progress in /api/metadata/status)"),
    ("GET", "/api/import/old-server", "Admin: is the old server's data found here, is an import running, and the last import's report"),
    ("GET", "/api/import/old-server/preview", "Admin: what the old server's database holds ?db=folder or file: libraries with suggested folders here, accounts, counts"),
    ("GET", "/api/plex-sync", "Admin: syncing with a Plex server: address, interval, what is synced, folder translations, Plex accounts (tokens shown as their last letters), whether it runs and the last run's report"),
    ("PUT", "/api/plex-sync", "Admin: change {enabled, url, interval_minutes, watched, resume, ratings, path_map: [{from, to}], accounts: [{id?, token?, user_id}]} (a known id without a token keeps its token)"),
    ("POST", "/api/plex-sync/test", "Admin: check {url?, token? or account_id?}: the Plex server's name, its movie and show libraries and their folders with the folder here"),
    ("POST", "/api/plex-sync/run", "Admin: sync with Plex now (in the background; the report is in GET /api/plex-sync)"),
    ("POST", "/api/import/old-server", "Admin: import {db, path_map: [{from, to}], accounts: {oldAccountId: userId}, libraries, watched, playlists, markers, details} (in the background; progress in /api/metadata/status)"),
];

/// The topic an address belongs to, in the order the API page shows them.
pub const GROUPS: &[&str] = &[
    "Server and sign-in",
    "Libraries",
    "Movies, shows and episodes",
    "Playback and streaming",
    "Watched, ratings and favourites",
    "Home and search",
    "Playlists",
    "Users and invites",
    "Plugins",
    "Settings",
    "Dashboard",
];

pub fn route_group(path: &str) -> &'static str {
    let g = if path.starts_with("/library/sections") || path.starts_with("/api/browse") || path.starts_with("/api/import") || path.starts_with("/api/plex-sync") {
        1
    } else if path.starts_with("/library/metadata/{id}/decision") || path.starts_with("/library/parts") || path.starts_with("/video") || path.starts_with("/status") || path.starts_with("/api/downloads") {
        3
    } else if path.starts_with("/library/metadata") || path.starts_with("/library/people") || path.starts_with("/api/metadata") {
        2
    } else if path.starts_with("/api/dashboard") {
        10
    } else if path.starts_with("/:/") || path.starts_with("/api/items") || path.starts_with("/api/watchlist") {
        4
    } else if path.starts_with("/hubs") || path.starts_with("/api/filters") {
        5
    } else if path.starts_with("/playlists") {
        6
    } else if path.starts_with("/api/users") || path.starts_with("/api/invites") {
        7
    } else if path.starts_with("/api/plugins") {
        8
    } else if path.starts_with("/api/settings") || path.starts_with("/api/transcoder") {
        9
    } else if path.starts_with("/library") {
        // Any other library address belongs with the movies and shows, never with sign-in.
        2
    } else {
        0
    };
    GROUPS[g]
}

pub fn route(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let segs: Vec<&str> = ctx.path.split('/').filter(|s| !s.is_empty()).collect();
    let m = ctx.method.as_str();

    // --- open to everyone
    match (m, segs.as_slice()) {
        ("GET", ["web", "app.js"]) => return bytes(200, "text/javascript; charset=utf-8", WEB_JS.as_bytes().to_vec()),
        ("GET", ["web", "pages.js"]) => return bytes(200, "text/javascript; charset=utf-8", WEB_PAGES_JS.as_bytes().to_vec()),
        ("GET", ["web", "dashboard.js"]) => return bytes(200, "text/javascript; charset=utf-8", WEB_DASHBOARD_JS.as_bytes().to_vec()),
        ("GET", ["web", "downloads.js"]) => return bytes(200, "text/javascript; charset=utf-8", WEB_DOWNLOADS_JS.as_bytes().to_vec()),
        ("GET", ["web", "i18n.js"]) => return bytes(200, "text/javascript; charset=utf-8", WEB_I18N_JS.as_bytes().to_vec()),
        ("GET", ["web", "lang", file]) => {
            return match WEB_LANGS.iter().find(|l| l.0 == *file) {
                Some(l) => bytes(200, "text/javascript; charset=utf-8", l.1.as_bytes().to_vec()),
                None => fail(404, "Not found"),
            };
        }
        ("GET", []) | ("GET", ["web", ..]) | ("GET", ["index.html"]) => {
            // The server's own name goes in right away, so a reload does not show
            // "Lichtspiel" first and swap it for the name a moment later.
            let name = crate::server::display_name(&app.db.lock().unwrap().settings);
            let esc = name.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
            let tab = if name == crate::server::APP_NAME { esc.clone() } else { format!("{esc} · {}", crate::server::APP_NAME) };
            let page = WEB_APP
                .replacen("<title>Lichtspiel</title>", &format!("<title>{tab}</title>"), 1)
                .replacen("<span id=\"sname\">Lichtspiel</span>", &format!("<span id=\"sname\">{esc}</span>"), 1);
            return bytes(200, "text/html; charset=utf-8", page.into_bytes());
        }
        ("GET", ["identity"]) => {
            let db = app.db.lock().unwrap();
            return ok(json!({"MediaContainer": {"machineIdentifier": db.machine_id, "version": VERSION, "friendlyName": crate::server::display_name(&db.settings)}}));
        }
        ("GET", ["api", "status"]) => {
            let db = app.db.lock().unwrap();
            return ok(json!({"name": crate::server::display_name(&db.settings), "version": VERSION, "setup_needed": db.users.is_empty(), "machine_id": db.machine_id,
                "started_at": app.started_at, "signed_in": ctx.user.as_ref().map(|u| u.username.clone())}));
        }
        ("POST", ["api", "setup"]) => return setup(app, ctx),
        ("POST", ["api", "login"]) => return login(app, ctx),
        ("GET", ["api", "home"]) => return home_profiles(app, ctx),
        ("POST", ["api", "home", uid, "login"]) => return home_login(app, ctx, uid),
        ("GET", ["api", "invites", code]) => return invite_info(app, code),
        ("POST", ["api", "invites", code, "accept"]) => return invite_accept(app, ctx, code),
        ("GET", ["video", "transcode", "hls", sid, file]) => return hls_file(app, sid, file),
        ("GET", ["rss", "watchlist", key]) => return crate::rss::feed(app, ctx, key),
        ("GET", ["favicon.ico"]) => return bytes(200, "image/svg+xml", FAVICON.as_bytes().to_vec()),
        _ => {}
    }

    let Some(user) = ctx.user.clone() else {
        return fail(401, "Please sign in. Send your token as the X-Token header or ?token=");
    };
    let admin = user.admin;
    let need_admin = || fail(403, "Only an admin can do this");
    let id = |s: &str| s.parse::<u64>().ok();

    match (m, segs.as_slice()) {
        ("POST", ["api", "logout"]) => {
            let mut db = app.db.lock().unwrap();
            match &ctx.token {
                Some(t) => db.tokens.retain(|x| &x.token != t),
                None => db.tokens.retain(|x| x.user_id != user.id),
            }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "me"]) => {
            let mut v = user_json(&user);
            v["can_delete"] = json!(crate::actions::may_delete(&app.db.lock().unwrap(), &user));
            v["can_download"] = json!(crate::downloads::may_download(&user));
            v["sidebar"] = user.sidebar.clone();
            v["prefs"] = json!(user.prefs);
            ok(v)
        }
        ("POST", ["api", "me", "profile"]) => {
            let name: String = ctx.body.get("full_name").and_then(|v| v.as_str()).unwrap_or("").trim().chars().take(80).collect();
            let mut db = app.db.lock().unwrap();
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) { u.full_name = name.clone(); }
            drop(db);
            app.save();
            ok(json!({"ok": true, "full_name": name}))
        }
        ("POST", ["api", "me", "prefs"]) => {
            // {key: text, …}: merged into this person's settings; null/"" removes a key.
            let Some(obj) = ctx.body.as_object() else { return fail(400, "Send an object") };
            let mut db = app.db.lock().unwrap();
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) {
                for (k, v) in obj {
                    if k.is_empty() || k.len() > 40 || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { continue; }
                    match v.as_str() {
                        Some(s) => { if u.prefs.len() < 64 || u.prefs.contains_key(k) { u.prefs.insert(k.clone(), s.chars().take(200).collect()); } }
                        None => { u.prefs.remove(k); }
                    }
                }
            }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("POST", ["api", "me", "2fa", "setup"]) => {
            let secret = crate::totp::new_secret();
            let mut db = app.db.lock().unwrap();
            let issuer = display_name(&db.settings);
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) { u.totp_pending = secret.clone(); }
            drop(db);
            app.save();
            let uri = crate::totp::uri(&issuer, &user.username, &secret);
            ok(json!({"secret": secret, "qr": crate::totp::qr_svg(&uri), "uri": uri}))
        }
        ("POST", ["api", "me", "2fa", "enable"]) => {
            let code = ctx.body_or_param("code").unwrap_or_default();
            let mut db = app.db.lock().unwrap();
            let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) else { return fail(404, "No such user") };
            if u.totp_pending.is_empty() { return fail(400, "Start the setup first"); }
            if !crate::totp::verify(&u.totp_pending, &code, now()) { return fail(400, "That code is wrong"); }
            u.totp_secret = std::mem::take(&mut u.totp_pending);
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("POST", ["api", "me", "2fa", "disable"]) => {
            let password = ctx.body_or_param("password").unwrap_or_default();
            let code = ctx.body_or_param("code").unwrap_or_default();
            if !auth::verify(&password, &user.salt, &user.password_hash) { return fail(403, "The password is wrong"); }
            let mut db = app.db.lock().unwrap();
            let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) else { return fail(404, "No such user") };
            if !u.totp_secret.is_empty() && !crate::totp::verify(&u.totp_secret, &code, now()) { return fail(403, "That code is wrong"); }
            u.totp_secret.clear();
            u.totp_pending.clear();
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("PUT", ["api", "me", "sidebar"]) => {
            // {order: [keys], hidden: [keys]}; a key is "s<library id>" or "playlists".
            let keys = |k: &str| -> Vec<Value> {
                ctx.body.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).filter(|x| x.len() <= 40).take(500).map(|x| json!(x)).collect()).unwrap_or_default()
            };
            let v = json!({"order": keys("order"), "hidden": keys("hidden")});
            let mut db = app.db.lock().unwrap();
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) { u.sidebar = v.clone(); }
            drop(db);
            app.save();
            ok(json!({"ok": true, "sidebar": v}))
        }
        ("POST", ["api", "me", "password"]) => {
            let old = ctx.body_or_param("old").unwrap_or_default();
            let new = ctx.body_or_param("new").unwrap_or_default();
            if !auth::verify(&old, &user.salt, &user.password_hash) {
                return fail(403, "The old password is wrong");
            }
            if new.chars().count() < 6 {
                return fail(400, "Use at least 6 characters");
            }
            let (h, s) = auth::hash_password(&new);
            let mut db = app.db.lock().unwrap();
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) {
                u.password_hash = h;
                u.salt = s;
            }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "docs"]) => ok(json!({
            "routes": ROUTES.iter().map(|r| json!({"method": r.0, "path": r.1, "description": r.2, "group": route_group(r.1)})).collect::<Vec<_>>(),
            "groups": GROUPS.iter().map(|g| json!({"name": g, "routes": ROUTES.iter().filter(|r| route_group(r.1) == *g)
                .map(|r| json!({"method": r.0, "path": r.1, "description": r.2})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "auth": "Send X-Token: <token> (or Authorization: Bearer <token>, or ?token=<token>)",
        })),
        ("GET", ["api", "filters"]) => ok(json!({
            "fields": query::ITEM_FIELDS.iter().map(|f| json!({"field": f.0, "type": f.1, "label": f.2})).collect::<Vec<_>>(),
            "operators": query::OPERATORS.iter().map(|o| json!({"op": o.0, "label": o.1})).collect::<Vec<_>>(),
            "url_shortcuts": {"field=value": "is", "field!=value": "is not", "field>=value": "at least", "field<=value": "at most", "field~=text": "contains", "field.op=value": "any operator", "a,b": "one of"},
            "playlist_sorts": ["title", "created_at", "updated_at", "item_count", "duration", "kind", "pinned", "owner"],
            "playlist_filters": ["kind=default|smart|manual", "smart=0|1", "owner=me|others|server|<id>", "title=<text>", "shared=0|1", "pinned=0|1", "min_items", "max_items"],
            "qualities": transcode::QUALITIES.iter().map(|q| q.0).collect::<Vec<_>>(),
            "plugin_events": plugins::EVENTS,
        })),

        // ---------------- libraries
        ("GET", ["library", "sections"]) => {
            let db = app.db.lock().unwrap();
            let scanning = app.scanning.lock().unwrap().clone();
            let list: Vec<Value> = db.sections.iter().filter(|s| db.can_see_section(&user, s.id)).map(|s| {
                let mut v = section_json(&db, s);
                v["scanning"] = json!(scanning.contains(&s.id));
                v
            }).collect();
            ok(json!({"MediaContainer": {"size": list.len(), "Directory": list}}))
        }
        ("POST", ["library", "sections"]) => {
            if !admin { return need_admin(); }
            let name = ctx.body_or_param("name").unwrap_or_default();
            let kind = ctx.body_or_param("type").or(ctx.body_or_param("kind")).and_then(|k| SectionKind::parse(&k));
            let Some(kind) = kind else { return fail(400, "type must be movie, show, music or other") };
            let paths = paths_from(ctx);
            if paths.is_empty() { return fail(400, "Give at least one folder in paths"); }
            if let Some(missing) = paths.iter().find(|p| !Path::new(p).is_dir()) { return fail(400, &format!("Folder not found: {missing}")); }
            let mut db = app.db.lock().unwrap();
            let sid = db.new_id();
            let name = if name.trim().is_empty() {
                match kind { SectionKind::Movie => "Movies", SectionKind::Show => "TV Shows", SectionKind::Other => "Videos", SectionKind::Music => "Music" }.to_string()
            } else { name };
            let s = Section { id: sid, name, kind, paths, created_at: now(), scanned_at: 0, credits_detection: true, intro_detection: true, language: ctx.body_or_param("language").unwrap_or_default(), scan_interval_minutes: None };
            let v = section_json(&db, &s);
            db.sections.push(s);
            drop(db);
            app.save();
            if ctx.param("scan") == Some("wait") {
                app.scan_now(sid);
            } else if ctx.param("scan") != Some("0") {
                app.scan_in_background(sid);
            }
            js(201, v)
        }
        ("GET", ["library", "sections", sid]) => {
            let db = app.db.lock().unwrap();
            match id(sid).and_then(|s| db.sections.iter().find(|x| x.id == s)) {
                Some(s) if db.can_see_section(&user, s.id) => {
                    let mut v = section_json(&db, s);
                    v["scanning"] = json!(app.scanning.lock().unwrap().contains(&s.id));
                    ok(v)
                }
                _ => fail(404, "No such library"),
            }
        }
        ("PUT", ["library", "sections", sid]) | ("PATCH", ["library", "sections", sid]) => {
            if !admin { return need_admin(); }
            let paths = paths_from(ctx);
            if let Some(missing) = paths.iter().find(|p| !Path::new(p).is_dir()) { return fail(400, &format!("Folder not found: {missing}")); }
            let mut db = app.db.lock().unwrap();
            let Some(s) = id(sid).and_then(|s| db.sections.iter_mut().find(|x| x.id == s)) else { return fail(404, "No such library") };
            if let Some(n) = ctx.body_or_param("name").filter(|n| !n.trim().is_empty()) { s.name = n; }
            let new_paths = !paths.is_empty();
            if new_paths { s.paths = paths; }
            if let Some(b) = ctx.body.get("credits_detection").and_then(|v| v.as_bool()) { s.credits_detection = b; }
            if let Some(b) = ctx.body.get("intro_detection").and_then(|v| v.as_bool()) { s.intro_detection = b; }
            let interval_set = ctx.body.get("scan_interval_minutes").is_some();
            if let Some(v) = ctx.body.get("scan_interval_minutes") {
                // null = the server's setting, 0 = never, else minutes (at least 5).
                s.scan_interval_minutes = match v.as_u64() { Some(m) if m == 0 || (5..=10080).contains(&m) => Some(m), Some(_) => return fail(400, "scan_interval_minutes must be 0 or 5-10080"), None if v.is_null() => None, None => return fail(400, "scan_interval_minutes must be a number or null") };
            }
            let mut language_changed = false;
            if let Some(l) = ctx.body.get("language").and_then(|v| v.as_str()) {
                let l = l.trim().to_lowercase();
                if !l.is_empty() && !db::LANGUAGES.iter().any(|x| x.0 == l) { return fail(400, "Unknown language"); }
                language_changed = l != s.language;
                s.language = l;
            }
            if language_changed {
                // New language: download this library's details again in it.
                let sid = s.id;
                drop(db);
                app.save();
                if new_paths { app.scan_in_background(sid); }
                let n = app.refetch_details(|sec| sec.id == sid);
                return ok(json!({"ok": true, "id": sid, "refetching": n}));
            }
            if !new_paths && (interval_set || ctx.body.get("credits_detection").is_some() || ctx.body.get("intro_detection").is_some()) {
                // Only a switch changed: no rescan needed.
                let sid = s.id;
                drop(db);
                app.save();
                return ok(json!({"ok": true, "id": sid}));
            }
            let sid = s.id;
            drop(db);
            app.save();
            app.scan_in_background(sid);
            ok(json!({"ok": true, "id": sid}))
        }
        ("DELETE", ["library", "sections", sid]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let mut db = app.db.lock().unwrap();
            db.sections.retain(|s| s.id != sid);
            db.items.retain(|_, i| i.section_id != sid);
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("POST", ["library", "sections", sid, "refresh"]) | ("GET", ["library", "sections", sid, "refresh"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let only = match ctx.param("path").filter(|p| !p.trim().is_empty()) {
                None => None,
                Some(p) => {
                    let p = PathBuf::from(p);
                    let inside = app.db.lock().unwrap().sections.iter().find(|s| s.id == sid)
                        .is_some_and(|s| s.paths.iter().any(|r| p.starts_with(r)) && !p.components().any(|c| matches!(c, std::path::Component::ParentDir)));
                    if !inside { return fail(400, "That folder is not inside the library's folders"); }
                    if !p.is_dir() { return fail(400, "Folder not found"); }
                    Some(p)
                }
            };
            if ctx.param("wait") == Some("1") {
                return match app.scan_folder_now(sid, only) {
                    Some(r) => ok(json!({"added": r.added.len(), "updated": r.updated.len(), "removed": r.removed.len(), "files": r.files_seen})),
                    None => fail(409, "That library is already being scanned"),
                };
            }
            app.scan_folder_in_background(sid, only);
            ok(json!({"ok": true, "scanning": true}))
        }
        ("POST", ["library", "sections", sid, "deepanalyze"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            match crate::actions::deep_analyze_section(app, sid) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "sections", sid, "analyze"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            match crate::actions::analyze_section(app, sid) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "sections", sid, "emptyTrash"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            match crate::actions::empty_trash(app, sid) { Ok(n) => ok(json!({"ok": true, "removed": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "sections", sid, "refreshMetadata"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let ids: Vec<u64> = app.db.lock().unwrap().items.values().filter(|i| i.section_id == sid && i.parent_id.is_none()).map(|i| i.id).collect();
            app.queue_online_details(&ids, true);
            ok(json!({"ok": true, "queued": ids.len()}))
        }
        ("POST", ["library", "sections", sid, "credits"]) | ("POST", ["library", "metadata", sid, "credits"]) => {
            if !admin { return need_admin(); }
            let Some(n) = id(sid) else { return fail(404, "Not found") };
            let ids: Vec<u64> = if ctx.path.starts_with("/library/sections/") {
                app.db.lock().unwrap().items.values().filter(|i| i.section_id == n && i.parent_id.is_none()).map(|i| i.id).collect()
            } else {
                vec![n]
            };
            match app.detect_credits(ids, true) {
                Ok(total) => ok(json!({"ok": true, "videos": total})),
                Err(e) => fail(409, &e),
            }
        }
        ("POST", ["library", "sections", sid, "intros"]) | ("POST", ["library", "metadata", sid, "intros"]) => {
            if !admin { return need_admin(); }
            let Some(n) = id(sid) else { return fail(404, "Not found") };
            let ids: Vec<u64> = if ctx.path.starts_with("/library/sections/") {
                app.db.lock().unwrap().items.values().filter(|i| i.section_id == n && i.parent_id.is_none()).map(|i| i.id).collect()
            } else {
                vec![n]
            };
            match app.detect_intros(ids, true) {
                Ok(total) => ok(json!({"ok": true, "episodes": total})),
                Err(e) => fail(409, &e),
            }
        }
        // ---------------- people (actors are shared by the whole library)
        ("GET", ["library", "people", key, "thumb"]) => {
            if key.is_empty() || key.len() > 40 || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { return fail(404, "Not found"); }
            let p = app.person_photo(key);
            match crate::metadata::file_image_type(&p) {
                Some(mime) => with_cache(serve_file(&p, mime, None, false)),
                None => fail(404, "No photo"),
            }
        }
        // A person's list is also at /library/people/{id}/media.
        ("GET", ["library", "people", key]) | ("GET", ["library", "people", key, "media"]) => {
            let db = app.db.lock().unwrap();
            let mut name = ctx.param("name").unwrap_or("").to_string();
            let mut photo = Value::Null;
            // Everything this person is in, matched by their id or, across
            // different details sources, by their name.
            let cast_of = |it: &db::Item| it.extra.get("cast").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            if name.is_empty() {
                for it in db.items.values() {
                    if let Some(c) = cast_of(it).into_iter().find(|c| c["key"].as_str() == Some(*key) || crate::server::person_key(c) == *key) {
                        name = c["name"].as_str().unwrap_or("").to_string();
                        break;
                    }
                }
            }
            let lname = name.to_lowercase();
            let mut found: Vec<(Option<u32>, Value)> = vec![];
            for it in db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show) && db.can_see_item(&user, i)) {
                let cast = cast_of(it);
                let Some(c) = cast.iter().find(|c| c["key"].as_str() == Some(*key) || crate::server::person_key(c) == *key || (!lname.is_empty() && c["name"].as_str().is_some_and(|n| n.to_lowercase() == lname))) else { continue };
                if photo.is_null() && c["photo"].is_string() { photo = c["photo"].clone(); }
                let mut v = item_json(&db, &user, it, false);
                v["role"] = c["role"].clone();
                found.push((it.year, v));
            }
            found.sort_by(|a, b| b.0.cmp(&a.0));
            let has_photo = crate::metadata::file_image_type(&app.person_photo(key)).is_some();
            ok(json!({"key": key, "type": "person", "name": name, "media": format!("/library/people/{key}/media"), "thumb": if has_photo { json!(format!("/library/people/{key}/thumb")) } else { Value::Null }, "photo": photo,
                "size": found.len(), "Metadata": found.into_iter().map(|x| x.1).collect::<Vec<_>>()}))
        }

        // ---------------- import from the old server
        ("POST", ["api", "imdb-ratings", "refresh"]) => {
            if !admin { return need_admin(); }
            if app.job.lock().unwrap().is_some() { return fail(409, "Another job is running; try again when it has finished"); }
            let a = app.clone();
            std::thread::spawn(move || {
                if let Err(e) = a.update_imdb_ratings(true) { elog!("IMDb ratings: {e}"); }
            });
            js(202, json!({"ok": true, "started": true}))
        }
        ("GET", ["api", "import", "old-server"]) => {
            if !admin { return need_admin(); }
            let last = std::fs::read_to_string(app.data_dir.join("cache").join("old-server").join("last-import.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
            let running = app.job.lock().unwrap().as_ref().is_some_and(|j| j.0.starts_with("Importing"));
            ok(json!({"detected": crate::migrate::detect().map(|p| p.to_string_lossy().to_string()), "running": running, "last": last,
                "looked_in": crate::migrate::candidates().iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>()}))
        }
        ("GET", ["api", "import", "old-server", "preview"]) => {
            if !admin { return need_admin(); }
            let src = match ctx.param("db").map(str::trim).filter(|s| !s.is_empty()) {
                Some(p) => crate::migrate::find_db(Path::new(p)),
                None => crate::migrate::detect(),
            };
            let Some(src) = src else { return fail(404, &format!("No {} found there. Enter the old server's data folder or the file itself.", crate::migrate::DB_NAME)) };
            match crate::migrate::preview(app, &src) {
                Ok(v) => ok(v),
                Err(e) => fail(400, &e),
            }
        }
        ("POST", ["api", "import", "old-server"]) => {
            if !admin { return need_admin(); }
            let o = match crate::migrate::options_from(&ctx.body, user.id) {
                Ok(o) => o,
                Err(e) => return fail(400, &e),
            };
            {
                let mut job = app.job.lock().unwrap();
                if job.is_some() { return fail(409, "Another job is running; try again when it has finished"); }
                *job = Some(("Importing: starting".into(), 0, 6));
            }
            let a = app.clone();
            std::thread::spawn(move || {
                let r = crate::migrate::run(&a, &o);
                *a.job.lock().unwrap() = None;
                if let Err(e) = &r { elog!("import: {e}"); }
                let mut p = a.meta_progress.lock().unwrap();
                p.done += 1;
                p.last = json!({"id": null, "title": "Import", "ok": r.is_ok(), "message": match &r { Ok(v) => crate::migrate::summary(v), Err(e) => format!("Import failed: {e}") }, "at": now()});
            });
            js(202, json!({"ok": true, "started": true}))
        }
        ("GET", ["api", "plex-sync"]) => {
            if !admin { return need_admin(); }
            ok(crate::plexsync::public(app))
        }
        ("PUT", ["api", "plex-sync"]) => {
            if !admin { return need_admin(); }
            match crate::plexsync::update(app, &ctx.body) { Ok(v) => ok(v), Err(e) => fail(400, &e) }
        }
        ("POST", ["api", "plex-sync", "test"]) => {
            if !admin { return need_admin(); }
            match crate::plexsync::test(app, &ctx.body) { Ok(v) => ok(v), Err(e) => fail(400, &e) }
        }
        ("POST", ["api", "plex-sync", "run"]) => {
            if !admin { return need_admin(); }
            if crate::plexsync::start_now(app) { js(202, json!({"ok": true, "started": true})) } else { fail(409, "A sync with Plex is already running") }
        }
        ("GET", ["api", "metadata", "status"]) => {
            let q = app.meta_queue.lock().unwrap().len();
            let busy = *app.meta_busy.lock().unwrap();
            let busy_title = if busy == 0 { None } else { app.db.lock().unwrap().items.get(&busy).map(|i| i.title.clone()) };
            let p = app.meta_progress.lock().unwrap();
            let job = app.job.lock().unwrap().clone();
            // Libraries being scanned right now (by name), so the web app can show it.
            let scanning_ids = app.scanning.lock().unwrap().clone();
            let scanning: Vec<String> = if scanning_ids.is_empty() { vec![] } else {
                let db = app.db.lock().unwrap();
                db.sections.iter().filter(|s| scanning_ids.contains(&s.id) && db.can_see_section(&user, s.id)).map(|s| s.name.clone()).collect()
            };
            ok(json!({"queued": q, "scanning": scanning, "working_on": if busy == 0 { Value::Null } else { json!(busy) }, "working_title": busy_title,
                "done": p.done, "failed": p.failed, "last": p.last,
                "job": job.map(|(label, done, total)| json!({"label": label, "done": done, "total": total})),
                // For the owner's "playing now" button in the top bar.
                "playing": if user.admin { json!(app.stream_count()) } else { Value::Null }}))
        }
        ("GET", ["library", "sections", sid, "all"]) => {
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let db = app.db.lock().unwrap();
            let Some(sec) = db.sections.iter().find(|s| s.id == sid) else { return fail(404, "No such library") };
            if !db.can_see_section(&user, sid) { return fail(403, "You don't have access to this library"); }
            let (kind, mut ids) = section_ids(&db, &user, sid, sec, ctx);
            let default_sort = if kind == ItemKind::Episode { "episode_order" } else { "title" };
            query::sort_items(&db, user.id, &mut ids, ctx.param("sort").filter(|s| !s.is_empty()).unwrap_or(default_sort));
            let (pg, off) = page(ctx, &ids);
            let list: Vec<Value> = pg.iter().filter_map(|i| db.items.get(i)).map(|i| item_json(&db, &user, i, false)).collect();
            ok(container(list, ids.len(), off, json!({"librarySectionID": sid, "librarySectionTitle": sec.name, "viewGroup": kind.as_str()})))
        }
        // How many items start with each letter (everything else under "#"), in A-Z order, for the
        // alphabet at the side of the library; the same filters as /all.
        ("GET", ["library", "sections", sid, "firstCharacter"]) => {
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let db = app.db.lock().unwrap();
            let Some(sec) = db.sections.iter().find(|s| s.id == sid) else { return fail(404, "No such library") };
            if !db.can_see_section(&user, sid) { return fail(403, "You don't have access to this library"); }
            let (_, ids) = section_ids(&db, &user, sid, sec, ctx);
            let mut counts: std::collections::BTreeMap<char, usize> = Default::default();
            for i in ids.iter().filter_map(|i| db.items.get(i)) {
                let c = i.sort_title.chars().next().map(|c| c.to_uppercase().next().unwrap_or(c)).filter(|c| c.is_alphabetic()).unwrap_or('#');
                let c = match c { 'Ä' => 'A', 'Ö' => 'O', 'Ü' => 'U', c => c };
                *counts.entry(c).or_default() += 1;
            }
            // "#" first, as the titles sort: digits before letters.
            let mut list: Vec<(char, usize)> = counts.into_iter().collect();
            list.sort_by_key(|(c, _)| if *c == '#' { (0, *c) } else { (1, *c) });
            ok(json!({"MediaContainer": {"size": list.len(), "total": ids.len(), "Directory": list.iter().map(|(c, n)| json!({"key": c.to_string(), "size": n})).collect::<Vec<_>>()}}))
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
            let hubs: Vec<Value> = related::related(db, &user, it, count).into_iter().map(|hb| {
                let list: Vec<Value> = hb.ids.iter().filter_map(|i| db.items.get(i)).map(|i| item_json(db, &user, i, false)).collect();
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
        ("GET", ["library", "metadata", iid]) => with_item(app, &user, iid, |db, it| ok(container(vec![item_json(db, &user, it, true)], 1, 0, json!({})))),
        ("PUT", ["library", "metadata", iid]) | ("PATCH", ["library", "metadata", iid]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let mut body = if ctx.body.is_object() { ctx.body.clone() } else { Value::Object(ctx.params.iter().map(|(k, v)| (k.clone(), json!(v))).collect()) };
            if let Some(g) = body.get("genres").and_then(|g| g.as_str()).map(String::from) {
                body["genres"] = json!(g.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>());
            }
            // Labels, collections and edition are kept with the other details.
            for k in ["labels", "collections", "edition", "directors", "writers", "producers", "countries", "original_title", "originally_available", "language_override", "credits_detection", "use_original_title"] {
                let Some(v) = body.get(k).cloned() else { continue };
                let v = match (k, v) {
                    ("edition" | "original_title" | "originally_available", v) => json!(v.as_str().unwrap_or("").trim()),
                    ("language_override", v) => {
                        let l = v.as_str().unwrap_or("").trim().to_string();
                        if !l.is_empty() && !db::LANGUAGES.iter().any(|x| x.0 == l) { return fail(400, "Unknown language"); }
                        json!(l)
                    }
                    ("use_original_title", v) => json!(match v.as_str().unwrap_or("") { "yes" => "yes", "no" => "no", _ => "" }),
                    ("credits_detection", v) => json!(match v.as_str().unwrap_or("") { "on" => "on", "off" => "off", _ => "" }),
                    (_, Value::String(t)) => json!(t.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>()),
                    (_, v) => v,
                };
                if !body["extra"].is_object() { body["extra"] = json!({}); }
                body["extra"][k] = v;
            }
            let mut db = app.db.lock().unwrap();
            let Some(it) = db.items.get_mut(&iid) else { return fail(404, "Not found") };
            if body.get("unlock").and_then(|v| v.as_bool()) == Some(true) { it.locked.clear(); }
            let old_title = it.title.clone();
            // What was changed by hand is locked: a refresh from online keeps it.
            let changed: Vec<String> = ["original_title", "originally_available", "directors", "writers", "producers", "countries"].into_iter()
                .filter(|k| body["extra"].get(*k).is_some_and(|v| {
                    let old = it.extra.get(*k).cloned().unwrap_or(Value::Null);
                    old != *v && !(old.is_null() && (v == &json!("") || v == &json!([])))
                }))
                .map(String::from).collect();
            apply_metadata(it, &body, true);
            for k in changed { if !it.locked.contains(&k) { it.locked.push(k); } }
            if body["extra"].get("use_original_title").is_some() && it.title == old_title { use_original_title(it); }
            drop(db);
            app.save();
            with_item(app, &user, &iid.to_string(), |db, it| ok(item_json(db, &user, it, true)))
        }
        ("GET", ["library", "metadata", iid, "children"]) => with_item(app, &user, iid, |db, it| {
            let list: Vec<Value> = db.children(it.id).iter().map(|c| item_json(db, &user, c, false)).collect();
            let n = list.len();
            ok(container(list, n, 0, json!({"parentTitle": it.title, "parentRatingKey": it.id.to_string()})))
        }),
        ("GET", ["library", "metadata", iid, "allLeaves"]) => with_item(app, &user, iid, |db, it| {
            let mut ids = db.leaves(it.id);
            query::sort_items(db, user.id, &mut ids, ctx.param("sort").unwrap_or("episode_order"));
            let list: Vec<Value> = ids.iter().filter_map(|i| db.items.get(i)).map(|c| item_json(db, &user, c, false)).collect();
            let n = list.len();
            ok(container(list, n, 0, json!({})))
        }),
        ("GET", ["library", "metadata", iid, "thumb"]) | ("GET", ["library", "metadata", iid, "thumb", _]) => thumb(app, &user, iid),
        ("GET", ["library", "metadata", iid, "cast", n]) => cast_photo(app, &user, iid, n),
        ("GET", ["library", "metadata", iid, slot @ ("logo" | "square")]) => custom_image(app, &user, iid, slot),
        ("PUT", ["library", "metadata", iid, "image", slot]) => { if !admin { return need_admin(); } set_image(app, iid, slot, &ctx.body) }
        ("DELETE", ["library", "metadata", iid, "image", slot]) => { if !admin { return need_admin(); } clear_image(app, iid, slot) }
        ("GET", ["library", "metadata", iid, "art"]) | ("GET", ["library", "metadata", iid, "art", _]) => art(app, &user, iid),
        ("GET", ["library", "metadata", iid, "matches"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let (kind, title, year) = {
                let db = app.db.lock().unwrap();
                let Some(it) = db.items.get(&iid) else { return fail(404, "Not found") };
                let file_title = it.extra.get("file_title").and_then(|v| v.as_str()).unwrap_or(&it.title).to_string();
                (it.kind, file_title, it.year)
            };
            if !matches!(kind, ItemKind::Movie | ItemKind::Show) { return fail(400, "Details are looked up for movies and shows"); }
            let title = ctx.param("title").filter(|t| !t.trim().is_empty()).map(String::from).unwrap_or(title);
            let year = match ctx.param("year") { Some(y) => y.parse().ok(), None => year };
            match crate::metadata::search(&app.settings(), kind, &title, year) {
                Ok(c) => ok(json!({"title": title, "year": year, "matches": c})),
                Err(e) => fail(502, &e),
            }
        }
        ("PUT", ["library", "metadata", iid, "match"]) | ("POST", ["library", "metadata", iid, "match"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let source = ctx.body["source"].as_str().or(ctx.param("source")).unwrap_or("").to_string();
            let mid = ctx.body["id"].as_str().map(String::from).or_else(|| ctx.body["id"].as_u64().map(|n| n.to_string())).or(ctx.param("id").map(String::from)).unwrap_or_default();
            if source.is_empty() || mid.is_empty() { return fail(400, "Send {source, id} from /matches"); }
            match app.match_online(iid, Some((source, mid)), true) {
                Ok(msg) => {
                    app.save();
                    with_item(app, &user, &iid.to_string(), |db, it| ok(json!({"ok": true, "message": msg, "item": item_json(db, &user, it, true)})))
                }
                Err(e) => fail(502, &e),
            }
        }
        ("DELETE", ["library", "metadata", iid, "match"]) | ("POST", ["library", "metadata", iid, "unmatch"]) | ("PUT", ["library", "metadata", iid, "unmatch"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            match app.unmatch(iid) {
                Ok(msg) => {
                    app.save();
                    with_item(app, &user, &iid.to_string(), |db, it| ok(json!({"ok": true, "message": msg, "item": item_json(db, &user, it, true)})))
                }
                Err(e) => fail(400, &e),
            }
        }
        ("GET", ["library", "metadata", iid, "markers"]) => with_item(app, &user, iid, |db, it| {
            let manual = it.extra.contains_key("credits_ms");
            let credits = db.credits_markers(it);
            let at = db::watched_at_ms(db.settings.played_at_behaviour, db.settings.watched_threshold_percent, it.duration_ms, &credits);
            let f = it.last_file();
            let how = f.map(|f| f.credits_source.as_str()).filter(|s| !s.is_empty()).unwrap_or("chapters");
            let checked = f.is_some_and(|f| f.credits_checked >= db::CREDITS_DETECTOR_VERSION);
            let intro = db.intro_marker(it);
            let intro_source = if it.extra.contains_key("intro_ms") { "manual" } else if intro.is_some() { "sound" }
                else if it.file.as_ref().is_some_and(|f| f.intro_checked > 0) { "none" } else { "pending" };
            ok(json!({"credits": credits, "source": if manual { "manual" } else if !credits.is_empty() { how } else if checked { "none" } else { "pending" },
                "watchedAt": at, "duration": it.duration_ms, "errors": f.map(|f| f.credits_errors).unwrap_or(0),
                "intro": intro.map(|(s, e)| json!([s, e])), "introSource": intro_source,
                "introDetection": it.kind == ItemKind::Episode && db.intro_on(it)}))
        }),
        ("PUT", ["library", "metadata", iid, "markers"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            // {"credits": [ms, …]} sets them; {"credits": null} goes back to the chapters.
            // {"intro": [start ms, end ms]} sets the intro; {"intro": null} goes back to the found one.
            let list: Option<Option<Vec<u64>>> = match ctx.body.get("credits") {
                Some(Value::Array(a)) => Some(Some(a.iter().filter_map(|v| v.as_u64()).collect())),
                Some(Value::Null) => Some(None),
                None if ctx.body.get("intro").is_some() => None,
                _ => return fail(400, "Send {\"credits\": [start in ms, …]} or {\"credits\": null}, and/or {\"intro\": [start ms, end ms]} or {\"intro\": null}"),
            };
            let intro: Option<Option<Vec<u64>>> = match ctx.body.get("intro") {
                Some(Value::Array(a)) => {
                    let v: Vec<u64> = a.iter().filter_map(|v| v.as_u64()).collect();
                    if v.len() != 2 || v[1] <= v[0] { return fail(400, "intro must be [start ms, end ms] with the end after the start"); }
                    Some(Some(v))
                }
                Some(Value::Null) => Some(None),
                None => None,
                _ => return fail(400, "intro must be [start ms, end ms] or null"),
            };
            {
                let mut db = app.db.lock().unwrap();
                let Some(it) = db.items.get_mut(&iid) else { return fail(404, "Not found") };
                if !it.kind.playable() { return fail(400, "Only movies and episodes have credits"); }
                match list {
                    Some(Some(l)) => { it.extra.insert("credits_ms".into(), json!(l)); }
                    Some(None) => { it.extra.remove("credits_ms"); }
                    None => {}
                }
                match intro {
                    Some(Some(v)) => { it.extra.insert("intro_ms".into(), json!(v)); }
                    Some(None) => { it.extra.remove("intro_ms"); }
                    None => {}
                }
            }
            app.save();
            with_item(app, &user, &iid.to_string(), |db, it| ok(json!({"ok": true, "credits": db.credits_markers(it), "intro": db.intro_marker(it).map(|(s, e)| json!([s, e]))})))
        }
        ("POST", ["library", "metadata", iid, "refresh"]) | ("PUT", ["library", "metadata", iid, "refresh"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let top = { let db = app.db.lock().unwrap(); App::top_level(&db, iid) };
            let Some(top) = top else { return fail(404, "Not found") };
            match app.match_online(top, None, true) {
                Ok(msg) => {
                    app.save();
                    with_item(app, &user, &iid.to_string(), |db, it| ok(json!({"ok": true, "message": msg, "item": item_json(db, &user, it, true)})))
                }
                Err(e) => fail(502, &e),
            }
        }
        ("GET", ["library", "metadata", iid, "history"]) => match id(iid) { Some(i) => crate::actions::history(app, &user, i), None => fail(404, "Not found") },
        ("POST", ["library", "metadata", iid, "report"]) => match id(iid) { Some(i) => crate::actions::report_problem(app, ctx, &user, i), None => fail(404, "Not found") },
        ("DELETE", ["library", "metadata", iid]) => match id(iid) { Some(i) => crate::actions::delete_media(app, &user, i), None => fail(404, "Not found") },
        ("POST", ["library", "metadata", iid, "deepanalyze"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            match crate::actions::deep_analyze(app, iid) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "metadata", iid, "analyze"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            match crate::actions::analyze(app, iid) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "metadata", iid, "optimize"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let q = ctx.body_or_param("quality").unwrap_or_else(|| "tv".into());
            match crate::actions::optimize(app, iid, &q) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("DELETE", ["library", "metadata", iid, "optimize"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            ok(json!({"ok": true, "removed": crate::actions::remove_optimized(app, iid)}))
        }
        ("GET", ["api", "reports"]) => if admin { crate::actions::reports(app, ctx) } else { need_admin() },
        ("PUT", ["api", "reports", rid]) | ("PATCH", ["api", "reports", rid]) | ("DELETE", ["api", "reports", rid]) => {
            if !admin { return need_admin(); }
            match id(rid) { Some(r) => crate::actions::update_report(app, ctx, r, m == "DELETE"), None => fail(404, "No such report") }
        }
        ("POST", ["api", "together"]) => crate::actions::together_create(app, ctx, &user),
        ("GET", ["api", "together", code]) => crate::actions::together_get(app, &user, code),
        ("POST", ["api", "together", code]) | ("PUT", ["api", "together", code]) => crate::actions::together_update(app, ctx, &user, code),
        ("DELETE", ["api", "together", code]) => crate::actions::together_leave(&user, code),
        ("GET", ["library", "metadata", iid, "decision"]) => {
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let db = app.db.lock().unwrap();
            let Some(it) = db.items.get(&iid).filter(|i| db.can_see_item(&user, i)) else { return fail(404, "Not found") };
            if it.file.is_none() { return fail(400, "This entry has no video file") }
            // ?part=n: one file of a split movie (0 = the first).
            let part = ctx.num("part").map(|n| n as usize).unwrap_or(0);
            let Some(f) = it.part(part) else { return fail(404, "No such part") };
            let req = transcode_request(db.settings.remote_max_mbps, db.settings.boost_dialogue, ctx);
            let d = transcode::decide(f, &req);
            let allowed = user.allow_transcode.unwrap_or(true) && db.settings.transcoding_enabled;
            let tok = ctx.token.as_ref().map(|t| format!("&token={t}")).unwrap_or_default();
            let tok = if part > 0 { format!("&part={part}{tok}") } else { tok };
            let q = ctx.param("quality").unwrap_or("original");
            let url = match d.method {
                Method::Direct => format!("/library/parts/{iid}/file?x=1{tok}"),
                _ => format!("/video/transcode/{iid}/stream.mp4?quality={q}{tok}"),
            };
            // An optimized version plays as it is instead of converting the original.
            let original_quality = req.max_height == 0 && req.max_kbps == 0 && req.audio_stream.is_none() && !req.force_transcode;
            if d.method != Method::Direct && part == 0 && original_quality && crate::actions::optimized_file(app, it).is_some() {
                let url = format!("/library/parts/{iid}/file?optimized=1{tok}");
                return ok(json!({"method": "direct", "optimized": true, "reasons": ["Plays the optimized version"], "height": d.height, "bitrateKbps": d.bitrate_kbps,
                    "url": url, "decisionCode": 1000, "decisionText": transcode::decision_text(1000), "direct": url, "allowed": true,
                    "hls": format!("/video/transcode/{iid}/start.m3u8?quality={q}{tok}"), "resume": db.view(user.id, iid).view_offset_ms, "duration": it.duration_ms}));
            }
            let code = if !allowed && d.method == Method::Transcode { 2000 } else { transcode::decision_code(&d.method) };
            ok(json!({"method": d.method, "reasons": d.reasons, "height": d.height, "bitrateKbps": d.bitrate_kbps, "url": url,
                "decisionCode": code, "decisionText": transcode::decision_text(code),
                "hls": format!("/video/transcode/{iid}/start.m3u8?quality={q}{tok}"), "direct": format!("/library/parts/{iid}/file?x=1{tok}"),
                "allowed": allowed || d.method == Method::Direct, "resume": db.view(user.id, iid).view_offset_ms, "duration": it.duration_ms}))
        }
        ("GET", ["library", "parts", iid, "file"]) | ("GET", ["library", "parts", iid, _, "file"]) | ("GET", ["library", "parts", iid, "file", _]) => direct_file(app, ctx, &user, iid),
        ("GET", ["library", "parts", iid, "subtitles", n]) => subtitles(app, &user, iid, n),
        ("POST", ["library", "parts", iid, "subtitles", n, "sync"]) => {
            if !admin { return need_admin(); }
            crate::handlers::subtitle_sync(app, &user, iid, n)
        }
        ("PUT", ["library", "parts", iid, "subtitles", n, "timing"]) => {
            if !admin { return need_admin(); }
            crate::handlers::subtitle_set_timing(app, &user, iid, n, &ctx.body)
        }
        ("GET", ["library", "metadata", iid, "chapters"]) => crate::handlers::chapters(app, &user, iid),
        ("GET", ["library", "parts", iid, "frame"]) => frame(app, ctx, &user, iid),

        // ---------------- streaming
        ("GET", ["video", "transcode", iid, "stream.mp4"]) => progressive(app, ctx, &user, iid),
        ("GET", ["video", "transcode", iid, "start.m3u8"]) => hls_start(app, ctx, &user, iid),
        ("GET", ["status", "sessions"]) => sessions(app, &user),
        ("GET", ["video", "transcode", "sessions"]) => {
            if !admin { return need_admin(); }
            let map = app.hls.lock().unwrap();
            ok(json!({"sessions": map.values().map(|s| json!({"id": s.id, "item": s.item_id, "user": s.user_id, "method": s.method, "height": s.height, "started_at": s.started_at})).collect::<Vec<_>>()}))
        }
        ("DELETE", ["video", "transcode", "sessions", sid]) => {
            let mut map = app.hls.lock().unwrap();
            let allowed = map.get(*sid).is_some_and(|s| admin || s.user_id == user.id);
            if !allowed { return fail(404, "No such session"); }
            let mut s = map.remove(*sid).unwrap();
            drop(map);
            let _ = s.child.kill();
            let _ = s.child.wait();
            let _ = std::fs::remove_dir_all(&s.dir);
            ok(json!({"ok": true}))
        }

        // ---------------- watch state
        (_, [":", "timeline"]) => {
            let key = ctx.body_or_param("ratingKey").or(ctx.body_or_param("key"));
            let Some(iid) = key.and_then(|k| k.rsplit('/').next().and_then(|x| x.parse().ok())) else { return fail(400, "ratingKey is needed") };
            let time = ctx.body_or_param("time").and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0).max(0.0) as u64;
            let state = ctx.body_or_param("state").unwrap_or_else(|| "playing".into());
            timeline(app, &user, iid, time, &state, ctx.body_or_param("client").unwrap_or_default(), ctx.viewer())
        }
        (_, [":", "scrobble"]) | (_, [":", "unscrobble"]) => {
            // One item, or several at once: key=1,2,3 (the library's multiple selection).
            let key = ctx.body_or_param("key").or(ctx.body_or_param("ratingKey")).unwrap_or_default();
            let ids: Vec<u64> = key.split(',').filter_map(|k| k.trim().parse::<u64>().ok()).collect();
            if ids.is_empty() { return fail(400, "key is needed") }
            let watched = segs[1] == "scrobble";
            let mut db = app.db.lock().unwrap();
            if ids.iter().any(|i| !db.items.contains_key(i)) { return fail(404, "Not found"); }
            for leaf in ids.iter().flat_map(|i| db.leaves(*i)).collect::<Vec<_>>() {
                let v = db.view_mut(user.id, leaf);
                let was = v.view_count;
                if watched {
                    v.view_count = v.view_count.max(1);
                    v.last_viewed_at = now();
                } else {
                    v.view_count = 0;
                }
                v.view_offset_ms = 0;
                v.finished = false;
                // Marked by hand: in the history once, not again for something already watched.
                if watched && was == 0 { db.record_play(user.id, leaf, true); }
            }
            drop(db);
            app.save();
            if watched { for iid in ids { app.fire("playback.watched", json!({"id": iid, "user": user.username, "manual": true})); } }
            ok(json!({"ok": true}))
        }
        (_, [":", "rate"]) => {
            let Some(iid) = ctx.body_or_param("key").and_then(|k| k.parse::<u64>().ok()) else { return fail(400, "key is needed") };
            let rating = ctx.body_or_param("rating").and_then(|r| r.parse::<f32>().ok()).filter(|r| *r >= 0.0);
            let mut db = app.db.lock().unwrap();
            if !db.items.contains_key(&iid) { return fail(404, "Not found"); }
            db.view_mut(user.id, iid).rating = rating.map(|r| r.min(10.0));
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("PUT", ["api", "items", iid, "favorite"]) | ("POST", ["api", "items", iid, "favorite"]) => {
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let fav = ctx.body.get("favorite").and_then(|v| v.as_bool()).or(ctx.param("favorite").map(|v| v == "1" || v == "true")).unwrap_or(true);
            let mut db = app.db.lock().unwrap();
            if !db.items.contains_key(&iid) { return fail(404, "Not found"); }
            db.view_mut(user.id, iid).favorite = fav;
            drop(db);
            app.save();
            ok(json!({"ok": true, "favorite": fav}))
        }
        ("PUT", ["api", "items", iid, "watchlist"]) | ("POST", ["api", "items", iid, "watchlist"]) => {
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let on = ctx.body.get("watchlist").and_then(|v| v.as_bool()).or(ctx.param("watchlist").map(|v| v == "1" || v == "true")).unwrap_or(true);
            let mut db = app.db.lock().unwrap();
            if let Err(r) = crate::dashboard::set_watchlist(&mut db, &user, &[iid], on) { return r; }
            drop(db);
            app.save();
            ok(json!({"ok": true, "watchlist": on}))
        }
        ("GET", ["library", "metadata", iid, "theme"]) => crate::handlers::theme(app, ctx, &user, iid),
        ("GET", ["api", "preroll"]) => crate::handlers::preroll(app, ctx),
        ("GET", ["api", "preroll", n, "file"]) => crate::handlers::preroll_file(app, ctx, n),
        ("GET", ["api", "rss"]) | ("POST", ["api", "rss"]) => crate::rss::manage(app, ctx, user.id),
        ("GET", ["api", "watchlist"]) => {
            let db = app.db.lock().unwrap();
            let ids = crate::dashboard::watchlist_ids(&db, &user, ctx.param("sort").unwrap_or(""), ctx.param("type").and_then(ItemKind::parse));
            list_resp(&db, &user, &ids, "watchlist")
        }
        ("POST", ["api", "watchlist"]) => {
            let ids: Vec<u64> = ctx.body.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().or(x.as_str().and_then(|s| s.parse().ok()))).collect()).unwrap_or_default();
            if ids.is_empty() { return fail(400, "ids are needed"); }
            let on = ctx.body.get("watchlist").and_then(|v| v.as_bool()).unwrap_or(true);
            let mut db = app.db.lock().unwrap();
            let changed = match crate::dashboard::set_watchlist(&mut db, &user, &ids, on) { Ok(n) => n, Err(r) => return r };
            drop(db);
            app.save();
            ok(json!({"ok": true, "changed": changed}))
        }
        ("GET", ["api", "downloads"]) => crate::downloads::list(app, ctx, &user),
        ("POST", ["api", "downloads"]) => {
            let ids: Vec<u64> = ctx.body.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().or(x.as_str().and_then(|s| s.parse().ok()))).collect()).unwrap_or_default();
            if ids.is_empty() { return fail(400, "ids are needed"); }
            let q = ctx.body_or_param("quality").unwrap_or_else(|| "original".into());
            crate::downloads::add(app, &user, &ids, &q)
        }
        ("DELETE", ["api", "downloads"]) => crate::downloads::remove_many(app, ctx, &user),
        ("GET", ["api", "downloads", did, "file"]) | ("GET", ["api", "downloads", did, "file", _]) => match id(did) { Some(d) => crate::downloads::file(app, ctx, &user, d), None => fail(404, "No such download") },
        ("POST", ["api", "downloads", did, "retry"]) => match id(did) { Some(d) => crate::downloads::retry(app, &user, d), None => fail(404, "No such download") },
        ("DELETE", ["api", "downloads", did]) => match id(did) { Some(d) => crate::downloads::remove(app, &user, d), None => fail(404, "No such download") },
        ("GET", ["api", "log"]) => {
            if !admin { return need_admin(); }
            ok(json!({"lines": crate::log::lines().into_iter().map(|(t, m)| json!({"t": t, "msg": m})).collect::<Vec<_>>()}))
        }
        ("GET", ["api", "log", "download"]) => {
            if !admin { return need_admin(); }
            let text = crate::log::export_text().into_bytes();
            let gz = ctx.param("gz") == Some("1");
            let (body, ctype, name) = if gz {
                use std::io::Write;
                let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                let _ = e.write_all(&text);
                (e.finish().unwrap_or_default(), "application/gzip", "lichtspiel-server-log.txt.gz")
            } else { (text, "text/plain; charset=utf-8", "lichtspiel-server-log.txt") };
            let mut r = crate::server::bytes(200, ctype, body);
            r.add_header(crate::server::header("Content-Disposition", &crate::handlers::attachment(name)));
            r
        }
        ("GET", ["api", "maintenance", "backups"]) => { if !admin { return need_admin(); } ok(json!({"backups": crate::maintenance::backups(app).into_iter().map(|(n, s, t)| json!({"name": n, "size": s, "time": t})).collect::<Vec<_>>()})) }
        ("POST", ["api", "maintenance", "restore"]) => {
            if !admin { return need_admin(); }
            let name = ctx.body_or_param("name").unwrap_or_default();
            match crate::maintenance::restore(app, &name) { Ok(()) => ok(json!({"ok": true})), Err(e) => fail(400, &e) }
        }
        ("GET", ["api", "optimized"]) => { if !admin { return need_admin(); } ok(crate::actions::optimized_list(app)) }
        ("DELETE", ["api", "optimized"]) => { if !admin { return need_admin(); } ok(json!({"ok": true, "removed": crate::actions::remove_all_optimized(app)})) }
        ("POST", ["api", "maintenance", "optimize"]) => { if !admin { return need_admin(); } crate::maintenance::optimize(app) }
        ("POST", ["api", "maintenance", "cleanup"]) => { if !admin { return need_admin(); } crate::maintenance::cleanup(app) }
        ("POST", ["api", "maintenance", "backup"]) => { if !admin { return need_admin(); } match crate::maintenance::backup(app) { Ok(p) => ok(json!({"ok": true, "path": p})), Err(e) => fail(500, &e) } }
        ("GET", ["api", "maintenance", "database"]) => { if !admin { return need_admin(); } crate::maintenance::database(app) }
        ("GET", ["api", "maintenance", "report"]) => { if !admin { return need_admin(); } crate::maintenance::report(app) }
        ("GET", ["api", "dashboard"]) => {
            if !user.admin { return fail(403, "Only an admin can do this"); }
            crate::dashboard::dashboard(app)
        }
        ("GET", ["api", "dashboard", "stats"]) => {
            if !user.admin { return fail(403, "Only an admin can do this"); }
            crate::dashboard::stats_range(app, ctx.param("range").unwrap_or("realtime"))
        }
        ("POST", ["api", "dashboard", "stop"]) => {
            if !admin { return need_admin(); }
            let (Some(uid), Some(iid)) = (ctx.body["user"].as_u64(), ctx.body["item"].as_u64()) else { return fail(400, "user and item are needed") };
            let message: String = ctx.body["message"].as_str().unwrap_or("").chars().take(500).collect();
            let n = app.stop_playback(uid, iid, &message);
            ok(json!({"ok": true, "stopped": n}))
        }
        ("GET", ["api", "dashboard", "history"]) => {
            if !user.admin { return fail(403, "Only an admin can do this"); }
            // Plays still going count too (with the time so far).
            let open: Vec<crate::db::PlaySession> = app.playing.lock().unwrap().values().filter(|p| p.session_at > 0).map(|p| p.session()).collect();
            let db = app.db.lock().unwrap();
            crate::dashboard::history(&db, &open, ctx.num("days").unwrap_or(30), ctx.num("user"))
        }

        // ---------------- hubs and search
        ("GET", ["hubs"]) | ("GET", ["hubs", "home"]) => {
            let db = app.db.lock().unwrap();
            let mut hubs = vec![hub(&db, &user, "continueWatching", "Continue Watching", continue_watching(&db, &user))];
            let mut wl = crate::dashboard::watchlist_ids(&db, &user, "", None);
            wl.truncate(20);
            hubs.push(hub(&db, &user, "watchlist", "Watchlist", wl));
            // Rows for the time of year (Halloween, Christmas ...); ?date=MM-DD tries another day.
            let day = ctx.param("date").and_then(related::month_day).or_else(|| related::month_day(&db::today())).unwrap_or((1, 1));
            for o in related::occasion_rows(&db, &user, day, 20) {
                hubs.push(hub(&db, &user, &o.identifier, &o.title, o.ids));
            }
            let episodes = if db.settings.group_recent_episodes {
                let eps = recently_added(&db, &user, Some(ItemKind::Episode), 200);
                shows_of(&db, &eps, 20)
            } else {
                recently_added(&db, &user, Some(ItemKind::Episode), 20)
            };
            hubs.extend([
                hub(&db, &user, "recentlyAddedMovies", "Recently Added Movies", recently_added(&db, &user, Some(ItemKind::Movie), 20)),
                hub(&db, &user, "recentlyAddedEpisodes", "Recently Added TV", episodes),
            ]);
            for p in db.playlists.iter().filter(|p| p.pinned && visible_playlist(p, &user)) {
                if matches!(p.default_key.as_deref(), Some("continue" | "recent_movies" | "recent_episodes")) { continue; }
                let mut ids = query::playlist_items(&db, &user, p);
                ids.truncate(20);
                hubs.push(hub(&db, &user, &format!("playlist.{}", p.id), &p.title, ids));
            }
            hubs.retain(|h| h["size"].as_u64().unwrap_or(0) > 0);
            ok(json!({"MediaContainer": {"size": hubs.len(), "Hub": hubs}}))
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
                    || (q.len() >= 3 && i.kind != ItemKind::Episode && ["director", "actor", "writer", "producer"].iter().any(|f| match query::field(&db, user.id, i, f) {
                        query::FieldValue::Text(v) => v.iter().any(|n| n.to_lowercase().contains(&q)),
                        _ => false,
                    }))
            };
            let mut ids: Vec<u64> = db.items.values()
                .filter(|i| db.can_see_item(&user, i) && i.kind != ItemKind::Season && !i.is_extra())
                .filter(|i| by_title(i) || by_other(i))
                .map(|i| i.id)
                .collect();
            ids.sort_by_key(|i| {
                let it = &db.items[i];
                (!it.title.to_lowercase().starts_with(&q), !by_title(it), it.kind == ItemKind::Episode, it.sort_title.clone())
            });
            ids.truncate(ctx.num("limit").unwrap_or(60) as usize);
            list_resp(&db, &user, &ids, "search")
        }

        // ---------------- playlists
        ("GET", ["playlists"]) | ("GET", ["playlists", "all"]) => {
            let db = app.db.lock().unwrap();
            let mut rows: Vec<(Playlist, usize, u64)> = vec![];
            let mut jsons: HashMap<u64, Value> = HashMap::new();
            for p in db.playlists.iter().filter(|p| visible_playlist(p, &user)) {
                let (v, n, d) = playlist_json(&db, &user, p);
                // ?section=<id>: only playlists with something from that library (its Playlists page).
                let in_section = ctx.param("section").and_then(|s| s.parse::<u64>().ok()).is_none_or(|sid| {
                    query::playlist_items(&db, &user, p).iter().any(|i| db.items.get(i).is_some_and(|x| x.section_id == sid))
                });
                if in_section && query::playlist_matches(p, &user, &ctx.params, n) {
                    jsons.insert(p.id, v);
                    rows.push((p.clone(), n, d));
                }
            }
            query::sort_playlists(&mut rows, ctx.param("sort").unwrap_or(""));
            let all: Vec<Value> = rows.iter().filter_map(|r| jsons.remove(&r.0.id)).collect();
            let (pg, off) = page(ctx, &all);
            ok(container(pg, all.len(), off, json!({})))
        }
        ("POST", ["playlists"]) => create_playlist(app, ctx, &user),
        ("POST", ["playlists", "preview"]) => {
            let q: SmartQuery = match serde_json::from_value(ctx.body.get("query").cloned().unwrap_or(ctx.body.clone())) {
                Ok(q) => q,
                Err(e) => return fail(400, &format!("The rules could not be read: {e}")),
            };
            let db = app.db.lock().unwrap();
            let ids = query::run_smart(&db, &user, &q);
            let list: Vec<Value> = ids.iter().take(200).filter_map(|i| db.items.get(i)).map(|i| item_json(&db, &user, i, false)).collect();
            ok(container(list, ids.len(), 0, json!({})))
        }
        ("GET", ["playlists", pid]) => {
            let db = app.db.lock().unwrap();
            match id(pid).and_then(|p| db.playlists.iter().find(|x| x.id == p)).filter(|p| visible_playlist(p, &user)) {
                Some(p) => ok(container(vec![playlist_json(&db, &user, p).0], 1, 0, json!({}))),
                None => fail(404, "No such playlist"),
            }
        }
        ("PUT", ["playlists", pid]) | ("PATCH", ["playlists", pid]) => update_playlist(app, ctx, &user, pid),
        ("DELETE", ["playlists", pid]) => {
            let Some(pid) = id(pid) else { return fail(404, "No such playlist") };
            let mut db = app.db.lock().unwrap();
            let Some(p) = db.playlists.iter().find(|p| p.id == pid) else { return fail(404, "No such playlist") };
            if p.is_default() { return fail(400, "Default playlists can't be deleted; unpin them instead (PUT pinned=false)"); }
            if p.owner_id != user.id && !admin { return fail(403, "This playlist belongs to someone else"); }
            db.playlists.retain(|p| p.id != pid);
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["playlists", pid, "items"]) => {
            let db = app.db.lock().unwrap();
            let Some(p) = id(pid).and_then(|p| db.playlists.iter().find(|x| x.id == p)).filter(|p| visible_playlist(p, &user)) else { return fail(404, "No such playlist") };
            let mut ids = query::playlist_items(&db, &user, p);
            let rules = query::rules_from_params(&ctx.params);
            let any = ctx.param("match") == Some("any");
            let text = ctx.param("q").or(ctx.param("query")).map(|s| s.to_lowercase());
            ids.retain(|i| db.items.get(i).is_some_and(|it| query::matches_all(&db, user.id, it, &rules, !any) && text.as_ref().is_none_or(|t| it.title.to_lowercase().contains(t))));
            if let Some(s) = ctx.param("sort").filter(|s| *s != "playlist" && !s.is_empty()) {
                query::sort_items(&db, user.id, &mut ids, s);
            }
            let (pg, off) = page(ctx, &ids);
            let list: Vec<Value> = pg.iter().filter_map(|i| db.items.get(i)).map(|i| {
                let mut v = item_json(&db, &user, i, false);
                v["playlistItemID"] = json!(i);
                v
            }).collect();
            ok(container(list, ids.len(), off, json!({"playlistID": p.id, "title": p.title, "smart": p.kind == PlaylistKind::Smart})))
        }
        ("POST", ["playlists", pid, "items"]) | ("PUT", ["playlists", pid, "items"]) => {
            let ids = ids_from(ctx);
            let mut db = app.db.lock().unwrap();
            let leaves: Vec<u64> = ids.iter().flat_map(|i| db.leaves(*i)).filter(|i| db.items.get(i).is_some_and(|it| db.can_see_item(&user, it))).collect();
            let Some(p) = id(pid).and_then(|p| db.playlists.iter_mut().find(|x| x.id == p)) else { return fail(404, "No such playlist") };
            if p.kind == PlaylistKind::Smart { return fail(400, "Smart playlists fill themselves; change their rules instead"); }
            if p.owner_id != user.id && !admin { return fail(403, "This playlist belongs to someone else"); }
            let before = p.items.len();
            for l in leaves {
                if !p.items.contains(&l) { p.items.push(l); }
            }
            p.updated_at = now();
            let added = p.items.len() - before;
            let pid = p.id;
            drop(db);
            app.save();
            app.fire("playlist.updated", json!({"id": pid, "added": added}));
            ok(json!({"ok": true, "added": added}))
        }
        ("DELETE", ["playlists", pid, "items", ..]) | ("POST", ["playlists", pid, "items", "remove"]) => {
            // One entry (/items/{itemId}), several (?ids=1,2 or {ids: [..]}), or
            // all of them (?all=1). A show or season removes all its episodes.
            let one = segs.get(3).filter(|s| **s != "remove").and_then(|s| id(s));
            let wanted = match one { Some(i) => vec![i], None => ids_from(ctx) };
            let all = ctx.param("all") == Some("1") || ctx.body.get("all").and_then(Value::as_bool) == Some(true);
            if segs.len() > 4 || (one.is_none() && segs.len() == 4 && segs[3] != "remove") {
                return fail(404, "Not found");
            }
            if wanted.is_empty() && !all {
                return fail(400, "Say which entries to remove: /playlists/{id}/items/{itemId}, ?ids=1,2, {ids: [..]} or ?all=1");
            }
            let mut db = app.db.lock().unwrap();
            let mut gone: Vec<u64> = wanted.clone();
            for w in &wanted {
                gone.extend(db.leaves(*w));
            }
            let Some(p) = id(pid).and_then(|p| db.playlists.iter_mut().find(|x| x.id == p)) else { return fail(404, "No such playlist") };
            if p.kind == PlaylistKind::Smart { return fail(400, "Smart playlists fill themselves; change their rules instead"); }
            if p.owner_id != user.id && !admin { return fail(403, "This playlist belongs to someone else"); }
            let before = p.items.len();
            if all { p.items.clear(); } else { p.items.retain(|i| !gone.contains(i)); }
            let removed = before - p.items.len();
            if removed == 0 { return fail(404, "Not in this playlist"); }
            p.updated_at = now();
            let (pid, left) = (p.id, p.items.len());
            drop(db);
            app.save();
            app.fire("playlist.updated", json!({"id": pid, "removed": removed}));
            ok(json!({"ok": true, "removed": removed, "left": left}))
        }
        ("PUT", ["playlists", pid, "items", iid, "move"]) => {
            let after = ctx.num("after").unwrap_or(0);
            let mut db = app.db.lock().unwrap();
            let Some(p) = id(pid).and_then(|p| db.playlists.iter_mut().find(|x| x.id == p)) else { return fail(404, "No such playlist") };
            if p.owner_id != user.id && !admin { return fail(403, "This playlist belongs to someone else"); }
            let Some(iid) = id(iid) else { return fail(404, "Not in this playlist") };
            let Some(pos) = p.items.iter().position(|i| *i == iid) else { return fail(404, "Not in this playlist") };
            p.items.remove(pos);
            let at = if after == 0 { 0 } else { p.items.iter().position(|i| *i == after).map(|x| x + 1).unwrap_or(p.items.len()) };
            p.items.insert(at, iid);
            p.updated_at = now();
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("POST", ["playlists", pid, "copy"]) => {
            let mut db = app.db.lock().unwrap();
            let Some(p) = id(pid).and_then(|p| db.playlists.iter().find(|x| x.id == p)).filter(|p| visible_playlist(p, &user)).cloned() else { return fail(404, "No such playlist") };
            let items = query::playlist_items(&db, &user, &p);
            let nid = db.new_id();
            let t = now();
            let title = ctx.body_or_param("title").unwrap_or_else(|| format!("{} (copy)", p.title));
            db.playlists.push(Playlist { id: nid, owner_id: user.id, title, summary: p.summary.clone(), kind: PlaylistKind::Manual, items, query: None, default_key: None, shared: false, pinned: false, created_at: t, updated_at: t });
            drop(db);
            app.save();
            js(201, json!({"ok": true, "id": nid}))
        }

        // ---------------- tokens
        ("GET", ["api", "tokens"]) => {
            let db = app.db.lock().unwrap();
            let list: Vec<Value> = db.tokens.iter().filter(|t| t.user_id == user.id).map(|t| json!({
                "id": auth::token_id(&t.token), "device": t.device, "created_at": t.created_at, "last_used": t.last_used, "last_device": t.last_device, "last_ip": t.last_ip,
                "current": ctx.token.as_deref() == Some(t.token.as_str()),
            })).collect();
            ok(json!({"tokens": list}))
        }
        ("POST", ["api", "tokens"]) => {
            // An extra token, e.g. for a script; it shows once.
            let device = ctx.body_or_param("device").filter(|d| !d.trim().is_empty()).unwrap_or_else(|| "API token".into());
            let mut db = app.db.lock().unwrap();
            let t = new_token(&mut db, user.id, &device);
            drop(db);
            app.save();
            js(201, json!({"ok": true, "token": t, "id": auth::token_id(&t), "device": device}))
        }
        ("POST", ["api", "tokens", "rotate"]) => {
            // New token for this sign-in; the old one stops working at once.
            // {all: true} also signs out every other device and script.
            let all = ctx.body.get("all").and_then(Value::as_bool).unwrap_or(false) || ctx.param("all") == Some("1");
            let mut db = app.db.lock().unwrap();
            let device = db.tokens.iter().find(|t| ctx.token.as_deref() == Some(t.token.as_str())).map(|t| t.device.clone()).unwrap_or_default();
            let before = db.tokens.len();
            db.tokens.retain(|t| t.user_id != user.id || (!all && ctx.token.as_deref() != Some(t.token.as_str())));
            let revoked = before - db.tokens.len();
            let t = new_token(&mut db, user.id, &device);
            drop(db);
            app.save();
            ok(json!({"ok": true, "token": t, "id": auth::token_id(&t), "revoked": revoked}))
        }
        ("DELETE", ["api", "tokens", tid]) => {
            let mut db = app.db.lock().unwrap();
            let before = db.tokens.len();
            db.tokens.retain(|t| t.user_id != user.id || auth::token_id(&t.token) != *tid);
            if db.tokens.len() == before { return fail(404, "No such token"); }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("DELETE", ["api", "users", uid, "tokens"]) => {
            if !admin { return need_admin(); }
            let Some(uid) = id(uid) else { return fail(404, "No such user") };
            let mut db = app.db.lock().unwrap();
            let before = db.tokens.len();
            db.tokens.retain(|t| t.user_id != uid);
            let revoked = before - db.tokens.len();
            drop(db);
            app.save();
            ok(json!({"ok": true, "revoked": revoked}))
        }

        // ---------------- users and invites
        ("GET", ["api", "users"]) => {
            if !admin { return need_admin(); }
            let db = app.db.lock().unwrap();
            ok(json!({"users": db.users.iter().map(user_json).collect::<Vec<_>>()}))
        }
        ("POST", ["api", "users"]) => {
            // A local account made by an admin: no invite link, they sign in with what the admin set.
            if !admin { return need_admin(); }
            let username = ctx.body_or_param("username").unwrap_or_default().trim().to_string();
            let password = ctx.body_or_param("password").unwrap_or_default();
            let home = ctx.body.get("home").and_then(|v| v.as_bool()).unwrap_or(false);
            let pin = ctx.body_or_param("pin").unwrap_or_default();
            let as_admin = ctx.body.get("admin").and_then(|v| v.as_bool()).unwrap_or(false);
            if !auth::valid_username(&username) { return fail(400, "Pick a user name of 2-40 letters or numbers"); }
            // A home profile may go without a password: it is then only reachable from the sign-in screen.
            if !(home && password.is_empty()) && password.chars().count() < 6 { return fail(400, "Use at least 6 characters for the password"); }
            if !pin.is_empty() && !auth::valid_pin(&pin) { return fail(400, "A PIN is 4 to 8 digits"); }
            if home && as_admin && pin.is_empty() { return fail(400, "An admin's home profile needs a PIN"); }
            let (h, s) = if password.is_empty() { (db::random_hex(32), db::random_hex(16)) } else { auth::hash_password(&password) };
            let sections = match ctx.body.get("allowed_sections") {
                Some(Value::Array(a)) => Some(a.iter().filter_map(|x| x.as_u64()).collect()),
                _ => None,
            };
            let mut db = app.db.lock().unwrap();
            if db.users.iter().any(|u| u.username.eq_ignore_ascii_case(&username)) { return fail(409, "That user name is taken"); }
            let id = db.new_id();
            db.users.push(db::User { id, username: username.clone(), email: ctx.body_or_param("email").unwrap_or_default(), password_hash: h, salt: s,
                admin: as_admin, created_at: now(), allowed_sections: sections, invited_by: Some(user.id),
                allow_transcode: None, allow_delete: false, allow_download: None, sidebar: Value::Null, home, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new(), full_name: String::new(), totp_secret: String::new(), totp_pending: String::new(), prefs: Default::default() });
            let _ = set_pin(db.users.last_mut().unwrap(), &pin);
            let v = user_json(db.user(id).unwrap());
            drop(db);
            app.save();
            app.fire("user.joined", json!({"username": username, "added_by": user.username}));
            js(201, v)
        }
        ("PATCH", ["api", "users", uid]) | ("PUT", ["api", "users", uid]) => {
            if !admin { return need_admin(); }
            let Some(uid) = id(uid) else { return fail(404, "No such user") };
            let mut db = app.db.lock().unwrap();
            let admins = db.users.iter().filter(|u| u.admin).count();
            let Some(u) = db.users.iter_mut().find(|u| u.id == uid) else { return fail(404, "No such user") };
            let new_pin = ctx.body.get("pin").and_then(|v| v.as_str());
            if let Some(p) = new_pin { if !p.is_empty() && !auth::valid_pin(p) { return fail(400, "A PIN is 4 to 8 digits"); } }
            let will_admin = ctx.body.get("admin").and_then(|v| v.as_bool()).unwrap_or(u.admin);
            let will_home = ctx.body.get("home").and_then(|v| v.as_bool()).unwrap_or(u.home);
            let will_pin = new_pin.map(|p| !p.is_empty()).unwrap_or(!u.pin_hash.is_empty());
            if will_home && will_admin && !will_pin { return fail(400, "An admin's home profile needs a PIN"); }
            if let Some(a) = ctx.body.get("admin").and_then(|v| v.as_bool()) {
                if !a && u.admin && admins <= 1 { return fail(400, "The last admin can't be removed"); }
                u.admin = a;
            }
            if let Some(v) = ctx.body.get("allowed_sections") {
                u.allowed_sections = if v.is_null() { None } else { v.as_array().map(|a| a.iter().filter_map(|x| x.as_u64()).collect()) };
            }
            if let Some(v) = ctx.body.get("allow_transcode").and_then(|v| v.as_bool()) { u.allow_transcode = Some(v); }
            if let Some(v) = ctx.body.get("allow_delete").and_then(|v| v.as_bool()) { u.allow_delete = v; }
            if let Some(v) = ctx.body.get("allow_download").and_then(|v| v.as_bool()) { u.allow_download = Some(v); }
            if ctx.body.get("reset_2fa").and_then(|v| v.as_bool()) == Some(true) { u.totp_secret.clear(); u.totp_pending.clear(); }
            if let Some(v) = ctx.body.get("max_age") { u.max_age = v.as_u64().map(|n| n.min(99) as u32); }
            if let Some(v) = ctx.body.get("block_unrated").and_then(|v| v.as_bool()) { u.block_unrated = v; }
            if let Some(e) = ctx.body.get("email").and_then(|v| v.as_str()) { u.email = e.into(); }
            if let Some(p) = new_pin { let _ = set_pin(u, p); }
            u.home = will_home;
            if let Some(pw) = ctx.body.get("password").and_then(|v| v.as_str()) {
                if pw.chars().count() < 6 { return fail(400, "Use at least 6 characters"); }
                let (h, s) = auth::hash_password(pw);
                u.password_hash = h;
                u.salt = s;
            }
            let v = user_json(u);
            drop(db);
            app.save();
            ok(v)
        }
        ("DELETE", ["api", "users", uid]) => {
            if !admin { return need_admin(); }
            let Some(uid) = id(uid) else { return fail(404, "No such user") };
            if uid == user.id { return fail(400, "You can't remove yourself"); }
            let mut db = app.db.lock().unwrap();
            db.users.retain(|u| u.id != uid);
            db.tokens.retain(|t| t.user_id != uid);
            db.playlists.retain(|p| p.owner_id != uid);
            let prefix = format!("{uid}:");
            db.views.retain(|k, _| !k.starts_with(&prefix));
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "invites"]) => {
            if !admin { return need_admin(); }
            let db = app.db.lock().unwrap();
            let base = base_url(&db, ctx);
            ok(json!({"invites": db.invites.iter().rev().map(|i| invite_json(&db, i, &base)).collect::<Vec<_>>()}))
        }
        ("POST", ["api", "invites"]) => {
            if !admin { return need_admin(); }
            let mut db = app.db.lock().unwrap();
            let days = ctx.body_or_param("days").and_then(|d| d.parse::<u64>().ok()).unwrap_or(db.settings.invite_default_days).clamp(1, 365);
            let sections = match ctx.body.get("allowed_sections") {
                Some(Value::Array(a)) => Some(a.iter().filter_map(|x| x.as_u64()).collect()),
                _ => None,
            };
            let inv = Invite {
                code: db::random_hex(12),
                email: ctx.body_or_param("email").unwrap_or_default(),
                note: ctx.body_or_param("note").unwrap_or_default(),
                created_by: user.id,
                created_at: now(),
                expires_at: now() + days * 86400,
                allowed_sections: sections,
                admin: ctx.body.get("admin").and_then(|v| v.as_bool()).unwrap_or(false),
                used_by: None,
                used_at: None,
            };
            let base = base_url(&db, ctx);
            let v = invite_json(&db, &inv, &base);
            db.invites.push(inv);
            drop(db);
            app.save();
            app.fire("user.invited", json!({"email": v["email"], "url": v["url"], "note": v["note"], "expires_at": v["expires_at"], "by": user.username}));
            js(201, v)
        }
        ("DELETE", ["api", "invites", code]) => {
            if !admin { return need_admin(); }
            let mut db = app.db.lock().unwrap();
            db.invites.retain(|i| i.code != *code);
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }

        // ---------------- plugins
        ("GET", ["api", "plugins"]) => {
            if !admin { return need_admin(); }
            plugin_list(app)
        }
        ("POST", ["api", "plugins", "reload"]) => {
            if !admin { return need_admin(); }
            app.plugins.lock().unwrap().reload();
            app.sync_plugin_states();
            app.save();
            let p = app.plugins.lock().unwrap();
            ok(json!({"ok": true, "count": p.list.len(), "errors": p.errors}))
        }
        ("GET", ["api", "plugins", "log"]) => {
            if !admin { return need_admin(); }
            let p = app.plugins.lock().unwrap();
            ok(json!({"log": p.log.iter().rev().cloned().collect::<Vec<_>>()}))
        }
        ("POST", ["api", "plugins", pid, onoff]) if *onoff == "enable" || *onoff == "disable" => {
            if !admin { return need_admin(); }
            if app.plugins.lock().unwrap().get(pid).is_none() { return fail(404, "No such plugin"); }
            let mut db = app.db.lock().unwrap();
            db.plugins.entry(pid.to_string()).or_default().enabled = *onoff == "enable";
            drop(db);
            app.save();
            ok(json!({"ok": true, "enabled": *onoff == "enable"}))
        }
        ("PUT", ["api", "plugins", pid, "settings"]) => {
            if !admin { return need_admin(); }
            if app.plugins.lock().unwrap().get(pid).is_none() { return fail(404, "No such plugin"); }
            let Value::Object(map) = ctx.body.clone() else { return fail(400, "Send the settings as a JSON object") };
            let mut db = app.db.lock().unwrap();
            let st = db.plugins.entry(pid.to_string()).or_default();
            for (k, v) in map { st.settings.insert(k, v); }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "plugins", "panels"]) => plugin_panels(app),
        ("POST", ["api", "plugins", pid, "call", action]) => plugin_call(app, ctx, &user, pid, action),

        // ---------------- settings
        ("GET", ["api", "settings"]) => {
            if !admin { return need_admin(); }
            let db = app.db.lock().unwrap();
            ok(json!({"settings": db.settings, "lan_url": format!("http://{}:{}", crate::lan_address(), *app.port.lock().unwrap()), "scan_intervals": db::SCAN_INTERVALS.iter().map(|s| json!({"minutes": s.0, "label": s.1})).collect::<Vec<_>>(),
                "data_dir": app.data_dir.to_string_lossy(), "transcoder_qualities": ["automatic", "speed", "quality", "max"], "hw_accel": ["auto", "none", "vaapi", "qsv", "nvenc"],
                "played_thresholds": db::PLAYED_THRESHOLDS, "h264_presets": transcode::X264_PRESETS,
                "played_at_choices": db::PLAYED_AT_CHOICES.iter().map(|c| json!({"value": c.0, "label": c.1})).collect::<Vec<_>>(),
                "credits_marker_choices": db::CREDITS_MARKER_CHOICES.iter().map(|c| json!({"value": c.0, "label": c.1})).collect::<Vec<_>>(),
                "languages": db::LANGUAGES.iter().map(|l| json!({"code": l.0, "name": l.1})).collect::<Vec<_>>(),
                "port_in_use": *app.port.lock().unwrap(),
                "your_address": ctx.remote.map(|a| a.to_string()), "your_address_trusted": ctx.remote_trusted, "you_are_local": ctx.is_local()}))
        }
        ("PUT", ["api", "settings"]) | ("PATCH", ["api", "settings"]) => {
            if !admin { return need_admin(); }
            let mut db = app.db.lock().unwrap();
            let mut cur = serde_json::to_value(&db.settings).unwrap_or(json!({}));
            if let (Value::Object(c), Value::Object(b)) = (&mut cur, &ctx.body) {
                for (k, v) in b {
                    if c.contains_key(k) { c.insert(k.clone(), v.clone()); }
                }
            }
            match serde_json::from_value::<db::Settings>(cur) {
                Ok(s) => {
                    if !["auto", "none", "vaapi", "qsv", "nvenc"].contains(&s.hw_accel.as_str()) { return fail(400, "hw_accel must be auto, none, vaapi, qsv or nvenc"); }
                    if !transcode::X264_PRESETS.contains(&s.h264_preset.as_str()) { return fail(400, "h264_preset must be one of the x264 presets"); }
                    if !(1..=51).contains(&s.h264_minimum_crf) { return fail(400, "h264_minimum_crf must be 1-51"); }
                    if s.played_at_behaviour > 3 { return fail(400, "played_at_behaviour must be 0, 1, 2 or 3"); }
                    if !db::CREDITS_MARKER_CHOICES.iter().any(|c| c.0 == s.credits_markers_behaviour) { return fail(400, "credits_markers_behaviour must be never, scheduled or asap"); }
                    if !db::CREDITS_MARKER_CHOICES.iter().any(|c| c.0 == s.intro_markers_behaviour) { return fail(400, "intro_markers_behaviour must be never, scheduled or asap"); }
                    if !["automatic", "speed", "quality", "max"].contains(&s.transcoder_quality.as_str()) { return fail(400, "transcoder_quality must be automatic, speed, quality or max"); }
                    if s.port == 0 { return fail(400, "port must be 1-65535"); }
                    if let Err(e) = crate::server::check_network_list(&s.lan_networks) { return fail(400, &e); }
                    if !s.bind_address.trim().is_empty() && s.bind_address.trim().parse::<std::net::IpAddr>().is_err() { return fail(400, "network card address must be empty or an IP address such as 192.168.1.10"); }
                    let dev = s.hw_device.trim();
                    if !(dev.is_empty() || dev.chars().all(|c| c.is_ascii_digit()) || (dev.starts_with("/dev/") && !dev.contains(' ') && !dev.contains(".."))) {
                        return fail(400, "graphics device must be empty, a card number or a path like /dev/dri/renderD128");
                    }
                    if let Err(e) = crate::server::check_open_networks(&s.no_auth_networks) { return fail(400, &e); }
                    let port_changed = s.port != db.settings.port;
                    let cache_mb = s.db_cache_mb;
                    let language_changed = s.metadata_language != db.settings.metadata_language;
                    let imdb_plots = !s.omdb_api_key.trim().is_empty() && s.omdb_api_key.trim() != db.settings.omdb_api_key.trim();
                    if !db::LANGUAGES.iter().any(|x| x.0 == s.metadata_language) { return fail(400, "Unknown language"); }
                    let articles_changed = s.sort_ignore_articles != db.settings.sort_ignore_articles || s.sort_articles != db.settings.sort_articles;
                    db.settings = s;
                    if articles_changed {
                        db::set_articles(db.settings.sort_ignore_articles, &db.settings.sort_articles);
                        let ids: Vec<u64> = db.items.keys().copied().collect();
                        for id in ids { if let Some(it) = db.items.get_mut(&id) { it.sort_title = db::sort_title(&it.title); } }
                    }
                    let mut v = json!({"ok": true, "settings": db.settings});
                    if port_changed || db.settings.port != *app.port.lock().unwrap() {
                        v["restart_needed"] = json!(format!("The server keeps using port {} until it is restarted (a --port option or LICHTSPIEL_PORT still wins over this setting).", *app.port.lock().unwrap()));
                    }
                    drop(db);
                    app.store.lock().unwrap().set_cache_mb(cache_mb);
                    app.save();
                    if language_changed {
                        // Libraries that follow the server's language get their details again.
                        v["refetching"] = json!(app.refetch_details(|sec| sec.language.trim().is_empty()));
                    } else if imdb_plots {
                        // A new OMDb key: everything gets its plot from IMDb now.
                        v["refetching"] = json!(app.refetch_details(|_| true));
                    }
                    ok(v)
                }
                Err(e) => fail(400, &format!("Those settings could not be read: {e}")),
            }
        }
        ("GET", ["api", "transcoder"]) => {
            if !admin { return need_admin(); }
            let mut v = transcode::detect(&app.settings());
            v["active_transcodes"] = json!(app.active_transcodes());
            ok(v)
        }
        ("GET", ["api", "browse"]) => {
            if !admin { return need_admin(); }
            browse(ctx.param("path").unwrap_or(""))
        }
        _ => fail(404, &format!("Unknown address {} {}. See GET /api/docs", ctx.method, ctx.path)),
    }
}

/// The items of a library page: the kind asked for, the simple filters from the address, the
/// advanced rules (`rules` = a JSON list of {field, op, value}, `rmatch=any` for "any of") and the title search.
fn section_ids(db: &db::Db, user: &db::User, sid: u64, sec: &db::Section, ctx: &Ctx) -> (ItemKind, Vec<u64>) {
    let kind = ctx.param("type").and_then(ItemKind::parse).unwrap_or(match sec.kind {
        SectionKind::Movie => ItemKind::Movie,
        SectionKind::Show => ItemKind::Show,
        SectionKind::Other => ItemKind::Video,
        SectionKind::Music => ItemKind::Artist,
    });
    let params: Vec<(String, String)> = ctx.params.iter().filter(|(k, _)| k != "rules" && k != "rmatch").cloned().collect();
    let rules: Vec<db::Rule> = query::rules_from_params(&params).into_iter().filter(|r| r.field != "kind").collect();
    let any = ctx.param("match") == Some("any");
    let advanced: Vec<db::Rule> = ctx.param("rules").and_then(|r| serde_json::from_str::<Vec<db::Rule>>(r).ok()).unwrap_or_default();
    let adv_any = ctx.param("rmatch") == Some("any");
    let text = ctx.param("q").or(ctx.param("query")).map(|s| s.to_lowercase());
    let ids = db.items.values()
        .filter(|i| i.section_id == sid && i.kind == kind)
        .filter(|i| query::matches_all(db, user.id, i, &rules, !any))
        .filter(|i| query::matches_all(db, user.id, i, &advanced, !adv_any))
        .filter(|i| text.as_ref().is_none_or(|t| i.title.to_lowercase().contains(t)))
        .map(|i| i.id)
        .collect();
    (kind, ids)
}

fn list_resp(db: &db::Db, user: &db::User, ids: &[u64], hub: &str) -> Resp {
    let list: Vec<Value> = ids.iter().filter_map(|i| db.items.get(i)).map(|i| item_json(db, user, i, false)).collect();
    let n = list.len();
    ok(container(list, n, 0, json!({"hubIdentifier": hub})))
}

#[cfg(test)]
mod tests {
    #[test]
    fn api_groups() {
        // Library addresses are never filed under "Server and sign-in".
        for (_, path, _) in super::ROUTES {
            if path.starts_with("/library") {
                assert_ne!(super::route_group(path), super::GROUPS[0], "{path}");
            }
        }
        assert_eq!(super::route_group("/library/people/{key}"), "Movies, shows and episodes");
        assert_eq!(super::route_group("/api/login"), "Server and sign-in");
    }
}
