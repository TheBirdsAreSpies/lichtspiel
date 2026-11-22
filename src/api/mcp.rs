//! An MCP server (Model Context Protocol) at POST /mcp, so an AI assistant (Claude Code, Claude
//! Desktop, any MCP client) can search the libraries, see what you are watching, mark things
//! watched, keep the watchlist and build playlists.
//!
//! It speaks MCP's "Streamable HTTP" transport without sessions: each POST carries one JSON-RPC
//! message and gets one JSON answer (no event stream). The caller signs in like any script, with a
//! token in `Authorization: Bearer …`, and every tool runs as one of the server's own API calls for
//! that person, so the same rights apply as in the web app (libraries they may see, admin only …).
//! Answers are cut down to what a model needs (no picture addresses, codecs only on a detail page).

use crate::server::*;
use serde_json::{json, Map, Value};
use std::io::Read;
use std::sync::Arc;

/// Protocol versions understood, newest first; the client's is used when it is one of them.
const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Longest answer handed to the model (characters); a longer one is cut with a note.
const MAX_TEXT: usize = 60_000;

const INSTRUCTIONS: &str = "Lichtspiel is a self-hosted media server for movies and TV shows. \
Every title has a numeric id: use search or browse_library to find it, get_item for its details. \
Shows contain seasons, seasons contain episodes (get_children). Tools act for the signed-in person: \
their libraries, watched state, ratings, watchlist and playlists. Tools marked admin need an admin account. \
For anything without a tool, api_request reaches the whole API (GET /api/docs lists every call).";


/// POST /mcp (signed in).
pub fn handle(app: &Arc<App>, ctx: &Ctx) -> Resp {
    match &ctx.body {
        Value::Object(_) => match answer(app, ctx, &ctx.body) {
            Some(v) => ok(v),
            // A notification or a reply from the client: taken, nothing to answer.
            None => bytes(202, "application/json", vec![]),
        },
        // JSON-RPC batches (protocol 2025-03-26) are answered as a list.
        Value::Array(list) if !list.is_empty() => {
            let out: Vec<Value> = list.iter().filter_map(|m| answer(app, ctx, m)).collect();
            if out.is_empty() { bytes(202, "application/json", vec![]) } else { ok(Value::Array(out)) }
        }
        _ => js(400, rpc_error(Value::Null, -32700, "Send one JSON-RPC message as the body")),
    }
}


fn rpc_error(id: Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}


/// The answer to one message, or None for a notification.
fn answer(app: &Arc<App>, ctx: &Ctx, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|m| m.as_str());
    let (Some(id), Some(method)) = (id, method) else { return None };
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOLS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOLS[0]);
            let name = display_name(&app.db.lock().unwrap().settings);
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "lichtspiel", "title": name, "version": VERSION},
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = match &params["arguments"] { Value::Object(_) => params["arguments"].clone(), _ => json!({}) };
            match call_tool(app, ctx, name, &args) {
                Ok(v) => text_result(&v, false),
                Err(ToolError::Unknown) => return Some(rpc_error(id, -32602, &format!("Unknown tool: {name}"))),
                Err(ToolError::Failed(msg)) => text_result(&Value::String(msg), true),
            }
        }
        // No resources or prompts, but clients that ask anyway get empty lists.
        "resources/list" => json!({"resources": []}),
        "resources/templates/list" => json!({"resourceTemplates": []}),
        "prompts/list" => json!({"prompts": []}),
        _ => return Some(rpc_error(id, -32601, &format!("Method not found: {method}"))),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}


fn text_result(v: &Value, is_error: bool) -> Value {
    let mut text = match v { Value::String(s) => s.clone(), _ => serde_json::to_string_pretty(v).unwrap_or_default() };
    if text.chars().count() > MAX_TEXT {
        text = text.chars().take(MAX_TEXT).collect();
        text.push_str("\n… (cut off: ask for fewer with limit/offset)");
    }
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}


enum ToolError {
    Unknown,
    Failed(String),
}


fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}


/// One tool: name, title, description, input schema, and whether it only reads.
fn tool(name: &str, title: &str, description: &str, schema: Value, read_only: bool) -> Value {
    json!({"name": name, "title": title, "description": description, "inputSchema": schema,
        "annotations": {"title": title, "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": false}})
}


fn tools() -> Vec<Value> {
    let id = json!({"type": "integer", "description": "The id of a movie, show, season or episode"});
    let ids = json!({"type": "array", "items": {"type": "integer"}, "description": "Ids of movies, shows, seasons or episodes"});
    let limit = |d: u64| json!({"type": "integer", "description": format!("How many at most (default {d})")});
    let mut list = vec![
        tool("list_libraries", "Libraries", "The libraries this person may see, with their id, kind (movie or show) and size.", obj(json!({}), &[]), true),
        tool("search", "Search", "Search every library by title, genre, year, studio or person (actor, director, writer). Returns movies, shows and episodes with their ids.",
            obj(json!({"query": {"type": "string"}, "limit": limit(25)}), &["query"]), true),
        tool("browse_library", "Browse a library",
            "List a library with filters and sorting. filters takes the API's filter keys: {\"genre\": \"Drama\"} (several: \"Drama,Comedy\"), \"year>\": \"2000\" (from), \
\"year<\": \"2010\" (up to), \"title~\": \"star\" (contains), \"actor\": \"…\", \"director\": \"…\", \"rating.gt\": \"7\", \"resolution\": \"4k\" … (get_filter_fields lists them all).",
            obj(json!({
                "library_id": {"type": "integer"},
                "type": {"type": "string", "enum": ["movie", "show", "season", "episode"], "description": "What to list (default: the library's own kind)"},
                "unwatched": {"type": "boolean", "description": "Only what this person has not watched"},
                "sort": {"type": "string", "description": "title, year:desc, added_at:desc, rating:desc, duration, random …"},
                "filters": {"type": "object", "additionalProperties": {"type": "string"}},
                "limit": limit(50),
                "offset": {"type": "integer"},
            }), &["library_id"]), true),
        tool("get_item", "Details", "Everything about one movie, show, season or episode: plot, cast, directors, ratings, length, watched state, files (resolution, audio and subtitle languages).",
            obj(json!({"id": id}), &["id"]), true),
        tool("get_children", "Seasons and episodes", "The seasons of a show or the episodes of a season. all_episodes: every episode of a show at once.",
            obj(json!({"id": id, "all_episodes": {"type": "boolean"}}), &["id"]), true),
        tool("get_related", "Related titles", "Titles related to a movie or show: other editions, similar titles, more by the director, more with the lead actors.",
            obj(json!({"id": id}), &["id"]), true),
        tool("home_row", "Home rows", "One of the home page rows: continue_watching (started, plus next episodes), on_deck (next episode of shows being watched) or recently_added.",
            obj(json!({"row": {"type": "string", "enum": ["continue_watching", "on_deck", "recently_added"]},
                "type": {"type": "string", "enum": ["movie", "episode"], "description": "recently_added only: movies or episodes"},
                "limit": limit(20)}), &["row"]), true),
        tool("set_watched", "Mark watched", "Mark movies or episodes watched or unwatched (a show or season: all its episodes).",
            obj(json!({"ids": ids, "watched": {"type": "boolean"}}), &["ids", "watched"]), false),
        tool("rate", "Rate", "This person's rating for a title, 0-10 (0 removes it).",
            obj(json!({"id": id, "rating": {"type": "number", "minimum": 0, "maximum": 10}}), &["id", "rating"]), false),
        tool("get_watchlist", "Watchlist", "This person's watchlist.",
            obj(json!({"sort": {"type": "string", "enum": ["added", "oldest", "title", "year", "rating"]},
                "type": {"type": "string", "enum": ["movie", "show", "season", "episode"]}}), &[]), true),
        tool("set_watchlist", "Change the watchlist", "Put titles on the watchlist (on: true) or take them off (on: false).",
            obj(json!({"ids": ids, "on": {"type": "boolean"}}), &["ids", "on"]), false),
        tool("list_playlists", "Playlists", "The playlists this person can see: their own, shared ones and the server's default ones.",
            obj(json!({"kind": {"type": "string", "enum": ["manual", "smart", "default"]}}), &[]), true),
        tool("get_playlist", "Playlist contents", "The titles in a playlist, in its order.",
            obj(json!({"id": {"type": "integer"}, "limit": limit(100), "offset": {"type": "integer"}}), &["id"]), true),
        tool("create_playlist", "New playlist",
            "Make a playlist. Manual: give ids (a show or season adds all its episodes). Smart: give rules instead, and it fills itself from the libraries, \
e.g. rules [{\"field\": \"genre\", \"op\": \"is\", \"value\": \"Comedy\"}, {\"field\": \"duration\", \"op\": \"lt\", \"value\": 95}] (get_filter_fields lists fields and operators).",
            obj(json!({
                "title": {"type": "string"},
                "summary": {"type": "string"},
                "ids": ids,
                "rules": {"type": "array", "items": {"type": "object", "properties": {"field": {"type": "string"}, "op": {"type": "string"}, "value": {}}, "required": ["field", "op", "value"]}},
                "match_all": {"type": "boolean", "description": "Smart: every rule must fit (default) or any one"},
                "sort": {"type": "string", "description": "Smart: e.g. year:desc, rating:desc, random"},
                "limit": {"type": "integer", "description": "Smart: at most this many titles"},
            }), &["title"]), false),
        tool("add_to_playlist", "Add to a playlist", "Add titles to a manual playlist (a show or season adds all its episodes).",
            obj(json!({"id": {"type": "integer", "description": "The playlist"}, "ids": ids}), &["id", "ids"]), false),
        tool("remove_from_playlist", "Remove from a playlist", "Take titles out of a manual playlist.",
            obj(json!({"id": {"type": "integer", "description": "The playlist"}, "ids": ids}), &["id", "ids"]), false),
        tool("get_filter_fields", "Filter fields", "Every field, operator and sort key for browse_library filters and smart playlist rules.", obj(json!({}), &[]), true),
        tool("now_playing", "Now playing", "What is playing on the server right now, and by whom.", obj(json!({}), &[]), true),
        tool("scan_library", "Scan a library (admin)", "Admin: look for new, changed and removed files in a library now.",
            obj(json!({"library_id": {"type": "integer"}}), &["library_id"]), false),
        tool("api_request", "API call",
            "Any call of the server's API, for what no other tool does (GET /api/docs lists every call with what it takes). \
Runs with this person's rights. Videos, pictures and downloads are not sent here.",
            obj(json!({
                "method": {"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"]},
                "path": {"type": "string", "description": "e.g. /library/sections/1/genres"},
                "query": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Address parameters"},
                "body": {"description": "JSON body"},
            }), &["method", "path"]), false),
    ];
    // Changes that delete or reach outside cannot be ruled out for the open API call.
    if let Some(t) = list.last_mut() { t["annotations"]["destructiveHint"] = json!(true); t["annotations"]["openWorldHint"] = json!(true); }
    list
}


/// Runs one of the server's own API calls as the signed-in person: (status, JSON answer).
fn api(app: &Arc<App>, ctx: &Ctx, method: &str, path: &str, params: Vec<(String, String)>, body: Value) -> (u16, Value) {
    let sub = Ctx {
        method: method.to_string(),
        path: path.to_string(),
        params,
        body,
        user: ctx.user.clone(),
        host: ctx.host.clone(),
        range: None,
        token: ctx.token.clone(),
        remote: ctx.remote,
        remote_trusted: ctx.remote_trusted,
        lan_extra: ctx.lan_extra,
        device: ctx.device.clone(),
    };
    let r = crate::api::routes::route(app, &sub);
    let status = r.status_code().0;
    let ctype = r.headers().iter().find(|h| h.field.equiv("Content-Type")).map(|h| h.value.as_str().to_ascii_lowercase()).unwrap_or_default();
    let textual = ctype.starts_with("application/json") || ctype.starts_with("text/") || ctype.contains("xml");
    if !textual { return (status, json!({"note": format!("not text ({ctype}), not shown")})); }
    let mut buf = Vec::new();
    let _ = r.into_reader().take(8 << 20).read_to_end(&mut buf);
    let v = serde_json::from_slice(&buf).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&buf).to_string()));
    (status, v)
}


/// The answer of a call, or the server's message when it failed.
fn checked((status, v): (u16, Value)) -> Result<Value, ToolError> {
    if status >= 400 {
        let msg = v["error"].as_str().map(String::from).unwrap_or_else(|| v.to_string());
        return Err(ToolError::Failed(format!("{status}: {msg}")));
    }
    Ok(v)
}


fn num(args: &Value, k: &str) -> Option<u64> {
    args[k].as_u64().or_else(|| args[k].as_str().and_then(|s| s.trim().parse().ok()))
}


fn need(args: &Value, k: &str) -> Result<u64, ToolError> {
    num(args, k).ok_or_else(|| ToolError::Failed(format!("{k} is needed (a number)")))
}


fn id_list(args: &Value) -> Result<Vec<u64>, ToolError> {
    let ids: Vec<u64> = match &args["ids"] {
        Value::Array(a) => a.iter().filter_map(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()))).collect(),
        Value::Number(n) => n.as_u64().into_iter().collect(),
        _ => vec![],
    };
    if ids.is_empty() { return Err(ToolError::Failed("ids is needed (a list of numbers)".into())); }
    Ok(ids)
}


fn page_params(args: &Value, default_limit: u64) -> Vec<(String, String)> {
    let mut p = vec![("limit".to_string(), num(args, "limit").unwrap_or(default_limit).min(500).to_string())];
    if let Some(o) = num(args, "offset") { p.push(("offset".into(), o.to_string())); }
    p
}


fn s(v: &str) -> String {
    v.to_string()
}


fn call_tool(app: &Arc<App>, ctx: &Ctx, name: &str, args: &Value) -> Result<Value, ToolError> {
    let none = Value::Null;
    Ok(match name {
        "list_libraries" => {
            let v = checked(api(app, ctx, "GET", "/library/sections", vec![], none))?;
            let list = v["MediaContainer"]["Directory"].as_array().cloned().unwrap_or_default();
            json!(list.iter().map(|l| json!({"id": l["id"], "title": l["title"], "type": l["type"], "titles": l["count"], "videos": l["videoCount"]})).collect::<Vec<_>>())
        }
        "search" => {
            let q = args["query"].as_str().unwrap_or("").trim().to_string();
            if q.is_empty() { return Err(ToolError::Failed("query is needed".into())); }
            let v = checked(api(app, ctx, "GET", "/hubs/search", vec![(s("query"), q)], none))?;
            let n = num(args, "limit").unwrap_or(25) as usize;
            let mut v = slim_list(&v);
            if let Some(a) = v["items"].as_array_mut() { a.truncate(n); }
            v
        }
        "browse_library" => {
            let lib = need(args, "library_id")?;
            let mut p = page_params(args, 50);
            if let Some(t) = args["type"].as_str() { p.push((s("type"), s(t))); }
            if let Some(t) = args["sort"].as_str() { p.push((s("sort"), s(t))); }
            if args["unwatched"].as_bool() == Some(true) { p.push((s("unwatched"), s("1"))); }
            if let Value::Object(f) = &args["filters"] {
                for (k, v) in f {
                    let v = match v { Value::String(x) => x.clone(), Value::Array(a) => a.iter().map(|x| x.as_str().map(String::from).unwrap_or(x.to_string())).collect::<Vec<_>>().join(","), x => x.to_string() };
                    // "year>=" and "year>" both mean "from".
                    p.push((k.trim_end_matches('=').to_string(), v));
                }
            }
            slim_list(&checked(api(app, ctx, "GET", &format!("/library/sections/{lib}/all"), p, none))?)
        }
        "get_item" => {
            let id = need(args, "id")?;
            let v = checked(api(app, ctx, "GET", &format!("/library/metadata/{id}"), vec![], none))?;
            let it = v["MediaContainer"]["Metadata"].get(0).cloned().unwrap_or(v);
            slim_item(&it, true)
        }
        "get_children" => {
            let id = need(args, "id")?;
            let tail = if args["all_episodes"].as_bool() == Some(true) { "allLeaves" } else { "children" };
            slim_list(&checked(api(app, ctx, "GET", &format!("/library/metadata/{id}/{tail}"), vec![], none))?)
        }
        "get_related" => {
            let id = need(args, "id")?;
            let v = checked(api(app, ctx, "GET", &format!("/library/metadata/{id}/related"), vec![(s("count"), s("8"))], none))?;
            let hubs = v["MediaContainer"]["Hub"].as_array().cloned().unwrap_or_default();
            json!(hubs.iter().map(|h| json!({"row": h["title"], "items": items_of(&h["Metadata"])})).collect::<Vec<_>>())
        }
        "home_row" => {
            let n = num(args, "limit").unwrap_or(20);
            let (path, mut p) = match args["row"].as_str().unwrap_or("") {
                "continue_watching" => ("/hubs/home/continueWatching", vec![]),
                "on_deck" => ("/hubs/home/onDeck", vec![]),
                "recently_added" => ("/hubs/home/recentlyAdded", vec![(s("limit"), n.to_string())]),
                other => return Err(ToolError::Failed(format!("Unknown row {other:?}: continue_watching, on_deck or recently_added"))),
            };
            if let Some(t) = args["type"].as_str() { p.push((s("type"), s(t))); }
            let mut v = slim_list(&checked(api(app, ctx, "GET", path, p, none))?);
            if let Some(a) = v["items"].as_array_mut() { a.truncate(n as usize); }
            v
        }
        "set_watched" => {
            let ids = id_list(args)?;
            let watched = args["watched"].as_bool().unwrap_or(true);
            let key = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
            checked(api(app, ctx, "POST", if watched { "/:/scrobble" } else { "/:/unscrobble" }, vec![(s("key"), key)], none))?;
            json!({"ok": true, "ids": ids, "watched": watched})
        }
        "rate" => {
            let id = need(args, "id")?;
            let rating = args["rating"].as_f64().ok_or_else(|| ToolError::Failed("rating is needed (0-10)".into()))?;
            checked(api(app, ctx, "POST", "/:/rate", vec![(s("key"), id.to_string()), (s("rating"), rating.clamp(0.0, 10.0).to_string())], none))?;
            json!({"ok": true, "id": id, "rating": rating})
        }
        "get_watchlist" => {
            let mut p = vec![];
            for k in ["sort", "type"] { if let Some(x) = args[k].as_str() { p.push((s(k), s(x))); } }
            slim_list(&checked(api(app, ctx, "GET", "/api/watchlist", p, none))?)
        }
        "set_watchlist" => {
            let ids = id_list(args)?;
            let on = args["on"].as_bool().unwrap_or(true);
            checked(api(app, ctx, "POST", "/api/watchlist", vec![], json!({"ids": ids, "watchlist": on})))?
        }
        "list_playlists" => {
            let p = args["kind"].as_str().map(|k| vec![(s("kind"), s(k))]).unwrap_or_default();
            let v = checked(api(app, ctx, "GET", "/playlists", p, none))?;
            let list = v["MediaContainer"]["Metadata"].as_array().cloned().unwrap_or_default();
            json!(list.iter().map(|p| {
                let mut o = pick(p, &["id", "title", "summary", "kind", "smart", "leafCount", "shared", "pinned", "owner"]);
                if let Some(d) = p["duration"].as_u64() { o["minutes"] = json!(d / 60_000); }
                o
            }).collect::<Vec<_>>())
        }
        "get_playlist" => {
            let id = need(args, "id")?;
            slim_list(&checked(api(app, ctx, "GET", &format!("/playlists/{id}/items"), page_params(args, 100), none))?)
        }
        "create_playlist" => {
            let title = args["title"].as_str().unwrap_or("").trim().to_string();
            if title.is_empty() { return Err(ToolError::Failed("title is needed".into())); }
            let mut body = json!({"title": title, "summary": args["summary"].as_str().unwrap_or("")});
            if let Value::Array(rules) = &args["rules"] {
                let mut q = json!({"match_all": args["match_all"].as_bool().unwrap_or(true), "rules": rules});
                if let Some(x) = args["sort"].as_str() { q["sort"] = json!(x); }
                if let Some(x) = num(args, "limit") { q["limit"] = json!(x); }
                body["kind"] = json!("smart");
                body["query"] = q;
            } else {
                body["kind"] = json!("manual");
                body["ids"] = json!(id_list(args).unwrap_or_default());
            }
            let v = checked(api(app, ctx, "POST", "/playlists", vec![], body))?;
            let p = v["MediaContainer"]["Metadata"].get(0).cloned().unwrap_or(v);
            pick(&p, &["id", "title", "kind", "smart", "leafCount"])
        }
        "add_to_playlist" => {
            let (id, ids) = (need(args, "id")?, id_list(args)?);
            checked(api(app, ctx, "POST", &format!("/playlists/{id}/items"), vec![], json!({"ids": ids})))?;
            json!({"ok": true})
        }
        "remove_from_playlist" => {
            let (id, ids) = (need(args, "id")?, id_list(args)?);
            checked(api(app, ctx, "POST", &format!("/playlists/{id}/items/remove"), vec![], json!({"ids": ids})))?;
            json!({"ok": true})
        }
        "get_filter_fields" => checked(api(app, ctx, "GET", "/api/filters", vec![], none))?,
        "now_playing" => {
            let v = checked(api(app, ctx, "GET", "/status/sessions", vec![], none))?;
            v["MediaContainer"]["Metadata"].clone()
        }
        "scan_library" => {
            let lib = need(args, "library_id")?;
            checked(api(app, ctx, "POST", &format!("/library/sections/{lib}/refresh"), vec![], none))?;
            json!({"ok": true, "note": "The scan runs in the background."})
        }
        "api_request" => {
            let method = args["method"].as_str().unwrap_or("GET").to_ascii_uppercase();
            let raw = args["path"].as_str().unwrap_or("").trim();
            let (path, q) = raw.split_once('?').unwrap_or((raw, ""));
            let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
            let mut p = parse_query(q);
            if let Value::Object(m) = &args["query"] {
                for (k, v) in m { p.push((k.clone(), v.as_str().map(String::from).unwrap_or(v.to_string()))); }
            }
            if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") { return Err(ToolError::Failed("method: GET, POST, PUT, PATCH or DELETE".into())); }
            if blocked_path(&path) {
                return Err(ToolError::Failed(format!("{path} sends videos, pictures or files (or is this MCP server itself) and is not available here")));
            }
            let body = if method == "GET" { Value::Null } else { args.get("body").cloned().unwrap_or(Value::Null) };
            let (status, v) = api(app, ctx, &method, &path, p, body);
            if status >= 400 { return checked((status, v)); }
            json!({"status": status, "answer": v})
        }
        _ => return Err(ToolError::Unknown),
    })
}


/// Addresses the open API call does not reach: video streams (they start ffmpeg), files,
/// pictures, the web app, and this endpoint itself.
fn blocked_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p == "/mcp" || p.starts_with("/mcp/") || p.starts_with("/video/") || p.starts_with("/library/parts/") || p.starts_with("/web/")
        || p == "/vue" || p.starts_with("/vue/") || p == "/" || p.contains("/file") || p.ends_with("/thumb") || p.ends_with("/art") || p.ends_with("/theme") || p.contains("/download")
        || p.ends_with(".m3u8") || p.ends_with(".mp4")
}


/// Only these keys of an object (those it has).
fn pick(v: &Value, keys: &[&str]) -> Value {
    let mut o = Map::new();
    for k in keys {
        if let Some(x) = v.get(*k).filter(|x| !x.is_null() && *x != "" && *x != &json!([])) { o.insert(k.to_string(), x.clone()); }
    }
    Value::Object(o)
}


/// Short names of a list of people ({name, role} or plain names).
fn names(v: &Value, n: usize) -> Vec<Value> {
    v.as_array().map(|a| a.iter().take(n).map(|p| match (p.get("name"), p.get("role").or(p.get("character")).and_then(|r| r.as_str()).filter(|r| !r.is_empty())) {
        (Some(name), Some(role)) => json!(format!("{} ({role})", name.as_str().unwrap_or(""))),
        (Some(name), None) => name.clone(),
        _ => p.clone(),
    }).collect()).unwrap_or_default()
}


/// A movie, show, season or episode cut down to what a model needs; `detail` adds the plot,
/// people and files.
fn slim_item(it: &Value, detail: bool) -> Value {
    let mut o = pick(it, &["id", "type", "title", "year", "grandparentTitle", "parentTitle", "parentIndex", "index", "editionTitle",
        "genres", "rating", "scores", "userRating", "contentRating", "leafCount", "viewedLeafCount", "viewCount", "librarySectionID"]);
    if let Some(d) = it["duration"].as_u64().filter(|d| *d > 0) { o["minutes"] = json!((d + 30_000) / 60_000); }
    if let Some(off) = it["viewOffset"].as_u64().filter(|x| *x > 0) { o["stoppedAtMinute"] = json!(off / 60_000); }
    for k in ["watchlist", "favorite", "missing"] { if it[k].as_bool() == Some(true) { o[k] = json!(true); } }
    if !detail { return o; }
    for k in ["summary", "tagline", "studio", "originallyAvailableAt", "addedAt", "lastViewedAt", "credits", "intro"] {
        if let Some(x) = it.get(k).filter(|x| !x.is_null() && *x != "") { o[k] = x.clone(); }
    }
    let ex = &it["extra"];
    for (k, n) in [("directors", 4), ("creators", 4), ("writers", 6), ("countries", 4), ("collections", 10), ("labels", 10)] {
        let l = names(&ex[k], n);
        if !l.is_empty() { o[k] = json!(l); }
    }
    let cast = names(&ex["cast"], 15);
    if !cast.is_empty() { o["cast"] = json!(cast); }
    if let Some(t) = ex["original_title"].as_str().filter(|t| !t.is_empty()) { o["originalTitle"] = json!(t); }
    if let Some(media) = it["Media"].as_array() {
        o["files"] = json!(media.iter().map(|m| {
            let mut f = pick(m, &["id", "container", "videoCodec", "height", "audioCodec", "audioChannels", "edition", "file"]);
            if let Some(b) = m["size"].as_u64() { f["sizeGB"] = json!((b as f64 / 1e9 * 10.0).round() / 10.0); }
            let langs = |kind: &str| -> Vec<Value> {
                let mut l: Vec<Value> = m["streams"].as_array().into_iter().flatten().filter(|s| s["kind"] == kind)
                    .map(|s| s["language"].clone()).filter(|x| x.as_str().is_some_and(|x| !x.is_empty())).collect();
                l.dedup();
                l
            };
            let (audio, subs) = (langs("audio"), langs("subtitle"));
            if !audio.is_empty() { f["audioLanguages"] = json!(audio); }
            if !subs.is_empty() { f["subtitleLanguages"] = json!(subs); }
            f
        }).collect::<Vec<_>>());
    }
    o
}


fn items_of(list: &Value) -> Vec<Value> {
    list.as_array().map(|a| a.iter().map(|i| slim_item(i, false)).collect()).unwrap_or_default()
}


/// A MediaContainer list as {total, offset, items}.
fn slim_list(v: &Value) -> Value {
    let mc = &v["MediaContainer"];
    let items = items_of(&mc["Metadata"]);
    let mut o = json!({"count": items.len(), "items": items});
    if let Some(t) = mc["totalSize"].as_u64().filter(|t| *t as usize != o["count"].as_u64().unwrap_or(0) as usize) { o["total"] = json!(t); }
    if let Some(x) = mc["offset"].as_u64().filter(|x| *x > 0) { o["offset"] = json!(x); }
    if let Some(t) = mc["title"].as_str() { o["title"] = json!(t); }
    o
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::db::Item;

    fn rpc(app: &Arc<App>, tok: &str, body: &str) -> String {
        crate::server::tests::ask(app, &format!("POST /mcp HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
    }

    fn result(got: &str) -> Value {
        serde_json::from_str(got.split("\r\n\r\n").nth(1).unwrap_or("")).unwrap_or(Value::Null)
    }

    #[test]
    fn a_client_signs_in_lists_tools_and_searches_with_the_callers_rights() {
        let (app, root) = crate::server::tests::test_app("mcp");
        let (admin, kid) = {
            let mut db = app.db.lock().unwrap();
            db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [root.join("media").to_string_lossy()], "created_at": 0})).unwrap());
            let i: Item = serde_json::from_value(json!({"id": 7, "section_id": 1, "kind": "movie", "title": "Der Himmel über Berlin", "sort_title": "Himmel", "year": 1987,
                "added_at": 0, "updated_at": 0, "genres": ["Drama"], "duration_ms": 7_680_000})).unwrap();
            db.items.insert(7, i);
            db.users[1].allowed_sections = Some(vec![]);
            (crate::api::handlers::new_token(&mut db, 1, "test"), crate::api::handlers::new_token(&mut db, 2, "test"))
        };
        // Without a token: no.
        let got = crate::server::tests::ask(&app, "POST /mcp HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
        assert!(got.starts_with("HTTP/1.1 401"), "{got}");

        let init = result(&rpc(&app, &admin, r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#));
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(init["result"]["capabilities"]["tools"].is_object());
        assert!(rpc(&app, &admin, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).starts_with("HTTP/1.1 202"));

        let list = result(&rpc(&app, &admin, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#));
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"search") && names.contains(&"create_playlist") && names.contains(&"api_request"), "{names:?}");

        let call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search","arguments":{"query":"himmel"}}}"#;
        let found = result(&rpc(&app, &admin, call));
        let text: Value = serde_json::from_str(found["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["items"][0]["id"], 7);
        assert_eq!(text["items"][0]["minutes"], 128);
        assert!(text["items"][0].get("thumb").is_none(), "picture addresses are left out");
        // The profile may not see library 1, so it finds nothing.
        let kid_found = result(&rpc(&app, &kid, call));
        let text: Value = serde_json::from_str(kid_found["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["count"], 0, "{text}");

        // Marking watched goes through the normal route.
        let mark = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"set_watched","arguments":{"ids":[7],"watched":true}}}"#;
        assert_eq!(result(&rpc(&app, &admin, mark))["result"]["isError"], false);
        assert!(app.db.lock().unwrap().view(1, 7).view_count > 0);

        // Admin only stays admin only; videos are not reachable through the open call.
        let scan = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"api_request","arguments":{"method":"GET","path":"/api/settings"}}}"#;
        assert_eq!(result(&rpc(&app, &kid, scan))["result"]["isError"], true);
        assert_eq!(result(&rpc(&app, &admin, scan))["result"]["isError"], false);
        let video = r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"api_request","arguments":{"method":"GET","path":"/video/transcode/7/stream.mp4"}}}"#;
        assert_eq!(result(&rpc(&app, &admin, video))["result"]["isError"], true);

        let unknown = result(&rpc(&app, &admin, r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"nope"}}"#));
        assert_eq!(unknown["error"]["code"], -32602);
        let _ = std::fs::remove_dir_all(&root);
    }
}
