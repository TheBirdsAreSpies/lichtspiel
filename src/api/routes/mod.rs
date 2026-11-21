//! Every API address. GET /api/docs returns the ROUTES table.

use crate::accounts::auth;
use crate::data::db::{self, now, Invite, ItemKind, Playlist, PlaylistKind, Section, SectionKind, SmartQuery};
use crate::api::handlers::*;
use crate::admin::plugins;
use crate::library::query;
use crate::api::related;
use crate::server::*;
use crate::playback::transcode::{self, Method};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod docs;
mod public;
mod account;
mod libraries;
mod connect;
mod browse;
mod items;
mod playback;
mod watch;
mod hubs;
mod playlists;
mod users;
mod plugin_api;
mod settings;

pub use docs::*;

pub fn route(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let segs: Vec<&str> = ctx.path.split('/').filter(|s| !s.is_empty()).collect();
    let m = ctx.method.as_str();

    // Open to everyone.
    if let Some(r) = public::routes(app, ctx, m, &segs) { return r; }

    let Some(user) = ctx.user.clone() else {
        return fail(401, "Please sign in. Send your token as the X-Token header");
    };

    // Each group in turn, in this order; the first one with a matching address answers.
    let groups: [Group; 12] = [account::routes, libraries::routes, connect::routes, browse::routes, items::routes, playback::routes, watch::routes, hubs::routes, playlists::routes, users::routes, plugin_api::routes, settings::routes];
    for group in groups {
        if let Some(r) = group(app, ctx, &user, m, &segs) { return r; }
    }
    fail(404, &format!("Unknown address {} {}. See GET /api/docs", ctx.method, ctx.path))
}

/// One group of addresses (one file in this folder): `Some` when one of its addresses matched.
type Group = fn(&Arc<App>, &Ctx, &db::User, &str, &[&str]) -> Option<Resp>;

/// Runs one group's `match`. Its last arm clears `hit` when none of the group's addresses matched;
/// an arm's `return` still answers the request, as it did when all were one `match`.
fn handled(f: impl FnOnce(&mut bool) -> Resp) -> Option<Resp> {
    let mut hit = true;
    let r = f(&mut hit);
    hit.then_some(r)
}

fn need_admin() -> Resp {
    fail(403, "Only an admin can do this")
}

fn id(s: &str) -> Option<u64> {
    s.parse::<u64>().ok()
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
    let ids = db.items.values().filter(|i| i.section_id == sid && i.kind == kind && i.version_of().is_none()).map(|i| i.id).collect();
    (kind, narrow(db, user, ids, ctx))
}

/// Keeps the items that match the filters in the address: the simple ones (`genre=…`, `year>=…`), the
/// advanced rules and the title search. Library pages and the watchlist use the same.
fn narrow(db: &db::Db, user: &db::User, ids: Vec<u64>, ctx: &Ctx) -> Vec<u64> {
    let params: Vec<(String, String)> = ctx.params.iter().filter(|(k, _)| k != "rules" && k != "rmatch" && k != "type").cloned().collect();
    let rules: Vec<db::Rule> = query::rules_from_params(&params).into_iter().filter(|r| r.field != "kind").collect();
    let any = ctx.param("match") == Some("any");
    let advanced: Vec<db::Rule> = ctx.param("rules").and_then(|r| serde_json::from_str::<Vec<db::Rule>>(r).ok()).unwrap_or_default();
    let adv_any = ctx.param("rmatch") == Some("any");
    let text = ctx.param("q").or(ctx.param("query")).map(|s| s.to_lowercase());
    let ix = query::index_for(db, &rules).or_else(|| query::index_for(db, &advanced));
    ids.into_iter()
        .filter(|id| db.items.get(id).is_some_and(|i| query::matches_all(db, ix.as_ref(), user.id, i, &rules, !any)
            && query::matches_all(db, ix.as_ref(), user.id, i, &advanced, !adv_any)
            && text.as_ref().is_none_or(|t| i.title.to_lowercase().contains(t))))
        .collect()
}

fn list_resp(db: &db::Db, user: &db::User, ids: &[u64], hub: &str) -> Resp {
    let ix = Index::build(db);
    let list: Vec<Value> = ids.iter().filter_map(|i| db.items.get(i)).map(|i| item_json_ix(db, user, i, false, Some(&ix))).collect();
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
