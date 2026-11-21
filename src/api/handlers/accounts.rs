//! Accounts: setup, sign-in, home profiles with PINs, invites, password reset by e-mail and sign-in with a code.

use super::*;

pub fn new_token(db: &mut Db, user_id: u64, device: &str) -> String {
    let t = db::random_hex(20);
    db.tokens.push(db::Token { token: auth::token_hash(&t), user_id, created_at: now(), device: device.chars().take(100).collect(), last_used: 0, last_device: String::new(), last_ip: String::new() });
    t
}

pub fn setup(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let username = ctx.body_or_param("username").unwrap_or_default().trim().to_string();
    let password = ctx.body_or_param("password").unwrap_or_default();
    if !auth::valid_username(&username) { return fail(400, "Pick a user name of 2-40 letters or numbers"); }
    if password.chars().count() < 6 { return fail(400, "Use at least 6 characters for the password"); }
    let (h, s) = auth::hash_password(&password);
    let mut db = app.db.lock().unwrap();
    if !db.users.is_empty() { return fail(409, "This server is already set up; sign in instead"); }
    let id = db.new_id();
    db.users.push(User { id, username, email: ctx.body_or_param("email").unwrap_or_default(), password_hash: h, salt: s, admin: true, created_at: now(), allowed_sections: None, invited_by: None, allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new(), full_name: String::new(), totp_secret: String::new(), totp_pending: String::new(), prefs: Default::default() });
    if let Some(name) = ctx.body_or_param("server_name").filter(|n| !n.trim().is_empty()) {
        db.settings.server_name = name;
    }
    let tok = new_token(&mut db, id, "setup");
    let u = user_json(db.user(id).unwrap());
    drop(db);
    app.save();
    js(201, json!({"token": tok, "user": u}))
}

/// Wrong sign-ins per user name and per address: (count, locked until). Five wrong ones for a
/// user name (30 for an address, which may be a whole household) lock it for five minutes.
static LOGIN_FAILS: std::sync::Mutex<Option<std::collections::HashMap<String, (u32, u64)>>> = std::sync::Mutex::new(None);
/// The last code step accepted per user: a code works once.
static USED_STEPS: std::sync::Mutex<Option<std::collections::HashMap<u64, u64>>> = std::sync::Mutex::new(None);

/// Takes a code step for a user; false if it (or a later one) was used already.
fn use_step(user: u64, step: u64) -> bool {
    let mut g = USED_STEPS.lock().unwrap_or_else(|e| e.into_inner());
    let last = g.get_or_insert_with(Default::default).entry(user).or_insert(0);
    if step <= *last { return false; }
    *last = step;
    true
}

fn login_keys(ctx: &Ctx, name: &str) -> [(String, u32); 2] {
    [(format!("user:{}", name.trim().to_lowercase()), 5), (format!("ip:{}", ctx.remote.map(|a| a.to_string()).unwrap_or_default()), 30)]
}

/// Minutes left of a lock, if one of the keys is locked.
fn login_locked(keys: &[(String, u32)]) -> Option<u64> {
    let g = LOGIN_FAILS.lock().unwrap();
    let map = g.as_ref()?;
    keys.iter().filter_map(|(k, limit)| map.get(k).filter(|(n, until)| n >= limit && *until > now()).map(|(_, until)| (until - now()).div_ceil(60))).max()
}

fn login_failed(keys: &[(String, u32)]) {
    let mut g = LOGIN_FAILS.lock().unwrap();
    let map = g.get_or_insert_with(Default::default);
    if map.len() > 10_000 { map.retain(|_, (_, until)| *until > now()); }
    for (k, limit) in keys {
        let e = map.entry(k.clone()).or_insert((0, 0));
        if e.1 != 0 && e.1 <= now() { *e = (0, 0); }
        e.0 += 1;
        if e.0 >= *limit { e.1 = now() + PIN_LOCK_SECS; }
    }
}

pub fn login(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let username = ctx.body_or_param("username").unwrap_or_default();
    let password = ctx.body_or_param("password").unwrap_or_default();
    let keys = login_keys(ctx, &username);
    if let Some(min) = login_locked(&keys) {
        return fail(429, &format!("Too many wrong attempts; try again in {min} minute(s)"));
    }
    let found = {
        let db = app.db.lock().unwrap();
        db.users.iter().find(|u| u.username.eq_ignore_ascii_case(username.trim())).cloned()
    };
    match found {
        Some(u) if auth::verify(&password, &u.salt, &u.password_hash) => {
            if !u.totp_secret.is_empty() {
                let code = ctx.body_or_param("code").unwrap_or_default();
                if code.trim().is_empty() {
                    return js(401, json!({"error": "Enter the code from your authenticator app", "status": 401, "needs_code": true}));
                }
                let step = crate::accounts::totp::matching_step(&u.totp_secret, &code, now());
                if !step.is_some_and(|s| use_step(u.id, s)) {
                    std::thread::sleep(Duration::from_millis(400));
                    login_failed(&keys);
                    return js(401, json!({"error": if step.is_some() { "That code was already used; wait for the next one" } else { "That code is wrong" }, "status": 401, "needs_code": true}));
                }
            }
            if let Some(m) = LOGIN_FAILS.lock().unwrap().as_mut() { m.remove(&keys[0].0); }
            let mut db = app.db.lock().unwrap();
            let tok = new_token(&mut db, u.id, &ctx.body_or_param("device").unwrap_or_default());
            drop(db);
            app.save();
            ok(json!({"token": tok, "user": user_json(&u)}))
        }
        _ => {
            std::thread::sleep(Duration::from_millis(400));
            login_failed(&keys);
            fail(401, "User name or password is wrong")
        }
    }
}


/// A home profile may be picked on the sign-in screen: it is marked as one, and an admin
/// only with a PIN (else anyone in the house could take over the server).
fn pickable(u: &User) -> bool {
    u.home && (!u.admin || !u.pin_hash.is_empty())
}

/// Set or remove (empty) a user's PIN.
pub fn set_pin(u: &mut User, pin: &str) -> Result<(), &'static str> {
    if pin.is_empty() {
        u.pin_hash.clear();
        u.pin_salt.clear();
        return Ok(());
    }
    if !auth::valid_pin(pin) { return Err("A PIN is 4 to 8 digits"); }
    let (h, s) = auth::hash_password(pin);
    u.pin_hash = h;
    u.pin_salt = s;
    Ok(())
}

/// Wrong PINs per user: (count, locked until). Five wrong ones lock the profile for five minutes.
static PIN_FAILS: std::sync::Mutex<Option<std::collections::HashMap<u64, (u32, u64)>>> = std::sync::Mutex::new(None);
const PIN_TRIES: u32 = 5;
const PIN_LOCK_SECS: u64 = 300;

pub fn home_profiles(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let db = app.db.lock().unwrap();
    if !db.settings.home_profiles || !ctx.is_local() { return ok(json!({"local": ctx.is_local(), "profiles": []})); }
    let list: Vec<Value> = db.users.iter().filter(|u| pickable(u))
        .map(|u| json!({"id": u.id, "username": u.username, "display_name": u.display_name(), "has_pin": !u.pin_hash.is_empty()})).collect();
    ok(json!({"local": true, "profiles": list}))
}

pub fn home_login(app: &Arc<App>, ctx: &Ctx, uid: &str) -> Resp {
    if !app.db.lock().unwrap().settings.home_profiles { return fail(403, "Profiles on the sign-in screen are switched off"); }
    if !ctx.is_local() { return fail(403, "Profiles can only be picked in the local network; sign in with your password"); }
    let Some(uid) = uid.parse::<u64>().ok() else { return fail(404, "No such profile") };
    let Some(u) = app.db.lock().unwrap().user(uid).cloned().filter(pickable) else { return fail(404, "No such profile") };
    if !u.pin_hash.is_empty() {
        {
            let mut g = PIN_FAILS.lock().unwrap();
            if let Some((n, until)) = g.get_or_insert_with(Default::default).get(&uid).copied() {
                if n >= PIN_TRIES && until > now() { return fail(429, "Too many wrong PINs; try again in a few minutes"); }
            }
        }
        let pin = ctx.body_or_param("pin").unwrap_or_default();
        if !auth::verify(&pin, &u.pin_salt, &u.pin_hash) {
            std::thread::sleep(Duration::from_millis(400));
            let mut g = PIN_FAILS.lock().unwrap();
            let e = g.get_or_insert_with(Default::default).entry(uid).or_insert((0, 0));
            if e.1 != 0 && e.1 <= now() { *e = (0, 0); }
            e.0 += 1;
            if e.0 >= PIN_TRIES { e.1 = now() + PIN_LOCK_SECS; }
            return fail(401, "Wrong PIN");
        }
        PIN_FAILS.lock().unwrap().get_or_insert_with(Default::default).remove(&uid);
    }
    let mut db = app.db.lock().unwrap();
    let tok = new_token(&mut db, u.id, &ctx.body_or_param("device").unwrap_or_else(|| "profile".into()));
    drop(db);
    app.save();
    ok(json!({"token": tok, "user": user_json(&u)}))
}


/// The invite mail: who invites, their message and the link.
pub fn invite_mail(settings: &db::Settings, lang: &str, who: &str, to: &str, note: &str, url: &str, days: u64) -> Result<(), String> {
    let server = display_name(settings);
    let note = note.trim();
    let (subject, text) = if crate::accounts::mail::german(lang) {
        (format!("Einladung zu {server}"),
         format!("Hallo,\n\n{who} hat dich zu {server} eingeladen.{}\n\nÜber diesen Link wählst du einen Benutzernamen und ein Passwort:\n{url}\n\nDer Link gilt {}.\n",
            if note.is_empty() { String::new() } else { format!("\n\n{note}") }, if days == 1 { "einen Tag".to_string() } else { format!("{days} Tage") }))
    } else {
        (format!("Invitation to {server}"),
         format!("Hello,\n\n{who} invited you to {server}.{}\n\nPick a user name and password with this link:\n{url}\n\nThe link works for {}.\n",
            if note.is_empty() { String::new() } else { format!("\n\n{note}") }, if days == 1 { "one day".to_string() } else { format!("{days} days") }))
    };
    crate::accounts::mail::send(settings, to, &subject, &text)
}

/// Settings → General → "Send a test mail": to the address given, else to the admin's own.
pub fn mail_test(app: &Arc<App>, ctx: &Ctx, user: &User) -> Resp {
    let to = ctx.body_or_param("to").map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| user.email.trim().to_string());
    if to.is_empty() { return fail(400, "Enter an address for the test mail, or your own under Settings → Account"); }
    let settings = app.settings();
    let lang = ctx.body_or_param("lang").unwrap_or_else(|| settings.metadata_language.clone());
    let server = display_name(&settings);
    let (subject, text) = if crate::accounts::mail::german(&lang) {
        (format!("Test von {server}"), format!("Diese Mail kommt von {server}. Der Mailserver ist richtig eingerichtet.\n"))
    } else {
        (format!("Test from {server}"), format!("This mail comes from {server}. The mail server is set up correctly.\n"))
    };
    match crate::accounts::mail::send(&settings, &to, &subject, &text) {
        Ok(()) => ok(json!({"ok": true, "to": to})),
        Err(e) => fail(502, &e),
    }
}

/// "Forgot password" requests per address and per account (time of each, the last hour).
static FORGOT: std::sync::Mutex<Option<std::collections::HashMap<String, Vec<u64>>>> = std::sync::Mutex::new(None);

/// Counts a request; false when there were too many in the last hour (10 per address, 3 per account).
fn forgot_allowed(key: &str, limit: usize) -> bool {
    let mut g = FORGOT.lock().unwrap_or_else(|e| e.into_inner());
    let map = g.get_or_insert_with(Default::default);
    if map.len() > 10_000 { map.retain(|_, v| v.iter().any(|t| *t + 3600 > now())); }
    let v = map.entry(key.to_string()).or_default();
    v.retain(|t| *t + 3600 > now());
    if v.len() >= limit { return false; }
    v.push(now());
    true
}

/// The address in mails that go out without an admin asking (a reset link): the address set in
/// Settings → General, else the one in the home network. Never the address of the request, which
/// anyone can choose (a link to their own site with the code in it).
fn mail_base(app: &Arc<App>, settings: &db::Settings) -> String {
    let public = settings.public_url.trim().trim_end_matches('/');
    if !public.is_empty() { return public.to_string(); }
    format!("{}://{}:{}", scheme(), crate::lan_address(), *app.port.lock().unwrap())
}

/// POST /api/password/forgot {name: user name or e-mail address, lang}: a link to set a new
/// password goes to the account's e-mail address. The answer is the same whether or not the
/// account exists, and the mail is sent in the background (the time taken tells nothing either).
pub fn password_forgot(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let settings = app.settings();
    if !crate::accounts::mail::ready(&settings) { return fail(503, "This server cannot send e-mail; ask an admin to set a new password"); }
    let name = ctx.body_or_param("name").unwrap_or_default().trim().to_lowercase();
    if name.is_empty() { return fail(400, "Enter your user name or e-mail address"); }
    if !forgot_allowed(&format!("ip:{}", ctx.remote.map(|a| a.to_string()).unwrap_or_default()), 10) {
        return fail(429, "Too many requests; try again in an hour");
    }
    let lang = ctx.body_or_param("lang").unwrap_or_default();
    let base = mail_base(app, &settings);
    let found: Vec<(User, String)> = {
        let db = app.db.lock().unwrap();
        db.users.iter().filter(|u| !u.email.trim().is_empty() && (u.username.to_lowercase() == name || u.email.trim().to_lowercase() == name))
            .map(|u| (u.clone(), auth::reset_code(&db.ticket_key, u.id, &u.password_hash, now()))).collect()
    };
    for (u, code) in found {
        if !forgot_allowed(&format!("user:{}", u.id), 3) { continue; }
        let settings = settings.clone();
        let lang = u.prefs.get("lang").cloned().filter(|l| !l.is_empty()).unwrap_or_else(|| if lang.is_empty() { settings.metadata_language.clone() } else { lang.clone() });
        let url = format!("{base}/#/reset/{code}");
        std::thread::spawn(move || {
            let server = display_name(&settings);
            let name = u.display_name();
            let (subject, text) = if crate::accounts::mail::german(&lang) {
                (format!("Neues Passwort für {server}"),
                 format!("Hallo {name},\n\njemand (hoffentlich du) möchte ein neues Passwort für dein Konto „{}“ bei {server} festlegen. Das geht mit diesem Link, er gilt eine Stunde:\n{url}\n\nWenn du das nicht warst, lösch diese Mail einfach. Dein Passwort bleibt dann, wie es ist.\n", u.username))
            } else {
                (format!("New password for {server}"),
                 format!("Hello {name},\n\nsomeone (hopefully you) asked for a new password for your account “{}” on {server}. Set it with this link, it works for one hour:\n{url}\n\nIf that was not you, just delete this mail. Your password then stays as it is.\n", u.username))
            };
            if let Err(e) = crate::accounts::mail::send(&settings, &u.email, &subject, &text) {
                crate::admin::log::add(format!("Password reset mail for {}: {e}", u.username));
            }
        });
    }
    ok(json!({"ok": true}))
}

/// The account a reset code is for, while the code works.
fn reset_user(app: &Arc<App>, code: &str) -> Option<User> {
    let db = app.db.lock().unwrap();
    let u = db.user(auth::reset_code_user(code)?)?;
    auth::reset_code_ok(&db.ticket_key, code, &u.password_hash, now()).then(|| u.clone())
}

/// GET /api/password/reset/{code}: whether the link still works, and for whom.
pub fn password_reset_info(app: &Arc<App>, code: &str) -> Resp {
    match reset_user(app, code) {
        Some(u) => ok(json!({"valid": true, "username": u.username, "server": display_name(&app.settings())})),
        None => fail(410, "This link does not work any more. Ask for a new one"),
    }
}

/// POST /api/password/reset/{code} {password}: the new password; every device is signed out.
pub fn password_reset(app: &Arc<App>, ctx: &Ctx, code: &str) -> Resp {
    let password = ctx.body_or_param("password").unwrap_or_default();
    if password.chars().count() < 6 { return fail(400, "Use at least 6 characters for the password"); }
    let Some(u) = reset_user(app, code) else { return fail(410, "This link does not work any more. Ask for a new one") };
    let (h, s) = auth::hash_password(&password);
    let mut db = app.db.lock().unwrap();
    // Checked again under the lock: the link works once.
    if !db.user(u.id).is_some_and(|x| x.password_hash == u.password_hash) { return fail(410, "This link does not work any more. Ask for a new one"); }
    if let Some(x) = db.users.iter_mut().find(|x| x.id == u.id) { x.password_hash = h; x.salt = s; }
    db.tokens.retain(|t| t.user_id != u.id);
    drop(db);
    if let Some(m) = LOGIN_FAILS.lock().unwrap().as_mut() { m.remove(&format!("user:{}", u.username.to_lowercase())); }
    app.save();
    ok(json!({"ok": true, "username": u.username}))
}

pub fn invite_info(app: &Arc<App>, code: &str) -> Resp {
    let db = app.db.lock().unwrap();
    match db.invites.iter().find(|i| i.code == code) {
        Some(i) if i.used_by.is_none() && i.expires_at >= now() => ok(json!({"valid": true, "server": crate::server::display_name(&db.settings), "email": i.email, "note": i.note,
            "invited_by": db.user(i.created_by).map(|u| u.display_name()), "expires_at": i.expires_at})),
        Some(i) if i.used_by.is_some() => fail(410, "This invite was already used"),
        Some(_) => fail(410, "This invite has expired"),
        None => fail(404, "No such invite"),
    }
}

pub fn invite_accept(app: &Arc<App>, ctx: &Ctx, code: &str) -> Resp {
    let username = ctx.body_or_param("username").unwrap_or_default().trim().to_string();
    let password = ctx.body_or_param("password").unwrap_or_default();
    if !auth::valid_username(&username) { return fail(400, "Pick a user name of 2-40 letters or numbers"); }
    if password.chars().count() < 6 { return fail(400, "Use at least 6 characters for the password"); }
    let (h, s) = auth::hash_password(&password);
    let mut db = app.db.lock().unwrap();
    if db.users.iter().any(|u| u.username.eq_ignore_ascii_case(&username)) { return fail(409, "That user name is taken"); }
    let Some(inv) = db.invites.iter().find(|i| i.code == code).cloned() else { return fail(404, "No such invite") };
    if inv.used_by.is_some() { return fail(410, "This invite was already used"); }
    if inv.expires_at < now() { return fail(410, "This invite has expired"); }
    let id = db.new_id();
    let email = ctx.body_or_param("email").filter(|e| !e.is_empty()).unwrap_or(inv.email.clone());
    db.users.push(User { id, username: username.clone(), email, password_hash: h, salt: s, admin: inv.admin, created_at: now(), allowed_sections: inv.allowed_sections.clone(), invited_by: Some(inv.created_by), allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new(), full_name: String::new(), totp_secret: String::new(), totp_pending: String::new(), prefs: Default::default() });
    if let Some(i) = db.invites.iter_mut().find(|i| i.code == code) {
        i.used_by = Some(id);
        i.used_at = Some(now());
    }
    let tok = new_token(&mut db, id, "invite");
    let u = user_json(db.user(id).unwrap());
    drop(db);
    app.save();
    app.fire("user.joined", json!({"username": username, "invite": code}));
    js(201, json!({"token": tok, "user": u}))
}


#[cfg(test)]
mod step_tests {
    /// Parallel sign-ins with the same code: only one may use it.
    #[test]
    fn a_code_step_is_taken_once_even_in_parallel() {
        for user in 1_000_000..1_000_200u64 {
            let gate = std::sync::Arc::new(std::sync::Barrier::new(8));
            let won: usize = (0..8).map(|_| { let g = gate.clone(); std::thread::spawn(move || { g.wait(); super::use_step(user, 7) }) })
                .collect::<Vec<_>>().into_iter().map(|t| t.join().unwrap() as usize).sum();
            assert_eq!(won, 1, "user {user}: the same code worked {won} times");
        }
        assert!(!super::use_step(1_000_000, 6) && super::use_step(1_000_000, 8));
    }
}



/// A TV or phone asked for a code; someone signed in elsewhere may confirm it.
struct LinkRequest {
    id: String,
    code: String,
    device: String,
    expires: u64,
    /// Set by the person who entered the code.
    approved_by: Option<u64>,
}

static LINKS: std::sync::Mutex<Vec<LinkRequest>> = std::sync::Mutex::new(Vec::new());
/// Wrong codes entered per signed-in user: (count, window start).
static LINK_FAILS: std::sync::Mutex<Option<std::collections::HashMap<u64, (u32, u64)>>> = std::sync::Mutex::new(None);
const LINK_TTL_SECS: u64 = 600;
const LINK_MAX_PENDING: usize = 200;
const LINK_TRIES: u32 = 8;
const LINK_WINDOW_SECS: u64 = 300;
/// No 0/O or 1/I: the code is read from a TV and typed on another device.
const LINK_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

fn link_code() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| LINK_ALPHABET[*x as usize % LINK_ALPHABET.len()] as char).collect()
}

fn link_normalize(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase()
}

/// The device asks for a code to show on the screen (no sign-in needed).
pub fn link_start(ctx: &Ctx) -> Resp {
    let mut g = LINKS.lock().unwrap();
    g.retain(|l| l.expires > now());
    if g.len() >= LINK_MAX_PENDING { return fail(429, "Too many sign-in codes are waiting; try again in a few minutes"); }
    let mut code = link_code();
    while g.iter().any(|l| l.code == code) { code = link_code(); }
    let id = db::random_hex(16);
    let device = ctx.body_or_param("device").filter(|d| !d.trim().is_empty()).unwrap_or_else(|| ctx.device.clone());
    g.push(LinkRequest { id: id.clone(), code: code.clone(), device, expires: now() + LINK_TTL_SECS, approved_by: None });
    drop(g);
    // A browser sends its own address: a QR code that opens the sign-in box with the code already typed in.
    let qr = ctx.body_or_param("link")
        .filter(|l| (l.starts_with("http://") || l.starts_with("https://")) && l.len() <= 300)
        .map(|l| crate::accounts::totp::qr_svg(&format!("{}#/link/{}", l.split('#').next().unwrap_or(""), code)));
    ok(json!({"id": id, "code": code, "expires_in": LINK_TTL_SECS, "qr": qr}))
}

/// The device asks whether its code was entered yet. The token is handed over once.
pub fn link_poll(app: &Arc<App>, id: &str) -> Resp {
    let taken = {
        let mut g = LINKS.lock().unwrap();
        g.retain(|l| l.expires > now());
        let Some(pos) = g.iter().position(|l| l.id == id) else { return fail(410, "This code has expired; ask for a new one") };
        if g[pos].approved_by.is_none() { return ok(json!({"status": "pending"})); }
        g.remove(pos)
    };
    let uid = taken.approved_by.unwrap_or(0);
    let mut db = app.db.lock().unwrap();
    let Some(u) = db.user(uid).cloned() else { return fail(410, "That account no longer exists") };
    let tok = new_token(&mut db, uid, &taken.device);
    drop(db);
    app.save();
    ok(json!({"status": "ok", "token": tok, "user": user_json(&u)}))
}

/// A signed-in person types the code shown on the device and signs it in as themselves.
pub fn link_confirm(ctx: &Ctx, user: &User) -> Resp {
    let code = link_normalize(&ctx.body_or_param("code").unwrap_or_default());
    {
        let mut g = LINK_FAILS.lock().unwrap();
        let e = g.get_or_insert_with(Default::default).entry(user.id).or_insert((0, now()));
        if e.1 + LINK_WINDOW_SECS <= now() { *e = (0, now()); }
        if e.0 >= LINK_TRIES { return fail(429, "Too many wrong codes; try again in a few minutes"); }
    }
    let mut g = LINKS.lock().unwrap();
    g.retain(|l| l.expires > now());
    match g.iter_mut().find(|l| l.code == code && l.approved_by.is_none()) {
        Some(l) => {
            l.approved_by = Some(user.id);
            ok(json!({"ok": true, "device": l.device}))
        }
        None => {
            drop(g);
            if let Some(e) = LINK_FAILS.lock().unwrap().get_or_insert_with(Default::default).get_mut(&user.id) { e.0 += 1; }
            fail(404, "That code is wrong or has expired")
        }
    }
}
