//! Passwords (PBKDF2 with HMAC-SHA256) and sign-in tokens.

use sha2::{Digest, Sha256};

pub const PBKDF2_ROUNDS: u32 = 60_000;

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let h = Sha256::digest(key);
        k[..32].copy_from_slice(h.as_slice());
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let ih = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(ih.as_slice());
    let mut out = [0u8; 32];
    out.copy_from_slice(outer.finalize().as_slice());
    out
}

pub fn pbkdf2(password: &str, salt: &str, rounds: u32) -> String {
    let mut msg = salt.as_bytes().to_vec();
    msg.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac_sha256(password.as_bytes(), &msg);
    let mut t = u;
    for _ in 1..rounds {
        u = hmac_sha256(password.as_bytes(), &u);
        for i in 0..32 {
            t[i] ^= u[i];
        }
    }
    t.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hash_password(password: &str) -> (String, String) {
    let salt = crate::data::db::random_hex(16);
    (pbkdf2(password, &salt, PBKDF2_ROUNDS), salt)
}

pub fn verify(password: &str, salt: &str, hash: &str) -> bool {
    same(&pbkdf2(password, salt, PBKDF2_ROUNDS), hash)
}

/// Compared without stopping at the first difference (so the time taken tells nothing).
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The signature of a ticket.
fn sig(key: &str, msg: &str) -> String {
    hmac_sha256(key.as_bytes(), msg.as_bytes())[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// What the library keeps of a sign-in token: its SHA-256 (64 hex letters), so a copy of the file signs nobody in.
pub fn token_hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A short public name for a token (the API lists and revokes tokens by this,
/// so the token itself is never shown again): the start of its hash.
pub fn token_id(token: &str) -> String {
    token_hash(token)[..12].to_string()
}

/// The public name of a stored token (a hash, see `token_hash`).
pub fn stored_token_id(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// A short-lived ticket for media addresses, so the sign-in token never goes into a URL (proxy logs,
/// browser history, the download list). `scope` is an item id (its video, subtitles, stills and theme),
/// "p" (pictures and pre-roll videos) or the path of one file to save; see `ticket_scope`. It holds the
/// short id of the sign-in it was made with, so signing out ends it too. The end time is rounded
/// (pictures: 6 to 12 hours, the rest 5 to 6 hours), so addresses stay the same and the browser keeps the
/// pictures it has (a new ticket is a new address, which it fetches again). A picture ticket opens only
/// pictures, and signing out ends it at once.
/// → (ticket, end time in unix seconds)
pub fn ticket(key: &str, user: u64, scope: &str, token: Option<&str>, now: u64) -> (String, u64) {
    let (step, life) = if scope == "p" { (6 * 3600, 12 * 3600) } else { (3600, 6 * 3600) };
    let expires = now / step * step + life;
    let msg = format!("{user}.{expires}.{}.{scope}", token.map(token_id).unwrap_or_default());
    (format!("{msg}.{}", sig(key, &msg)), expires)
}

/// How long a "Forgot password" link works.
pub const RESET_SECS: u64 = 3600;

/// The code in a "Forgot password" link: user, end time and a signature that includes the
/// current password hash, so the link stops working once the password was changed (it works once).
pub fn reset_code(key: &str, user: u64, password_hash: &str, now: u64) -> String {
    let msg = format!("{user}.{}", now + RESET_SECS);
    format!("{msg}.{}", sig(key, &format!("reset.{msg}.{password_hash}")))
}

/// The user a reset code is for (not checked yet: see `reset_code_ok`).
pub fn reset_code_user(code: &str) -> Option<u64> {
    code.split('.').next()?.parse().ok()
}

pub fn reset_code_ok(key: &str, code: &str, password_hash: &str, now: u64) -> bool {
    let Some((msg, s)) = code.rsplit_once('.') else { return false };
    let Some(Ok(expires)) = msg.split_once('.').map(|(_, e)| e.parse::<u64>()) else { return false };
    !key.is_empty() && expires > now && same(&sig(key, &format!("reset.{msg}.{password_hash}")), s)
}

/// (user, token id, scope) of a ticket made with `key` that has not ended.
pub fn read_ticket(key: &str, t: &str, now: u64) -> Option<(u64, String, String)> {
    let (msg, sig) = t.rsplit_once('.')?;
    if key.is_empty() || !same(&self::sig(key, msg), sig) { return None; }
    let mut p = msg.splitn(4, '.');
    let (user, expires, tid, scope) = (p.next()?.parse().ok()?, p.next()?.parse::<u64>().ok()?, p.next()?, p.next()?);
    (expires > now).then(|| (user, tid.to_string(), scope.to_string()))
}

/// The ticket scope that opens `path` (GET only), or None when no ticket opens it.
pub fn ticket_scope(path: &str) -> Option<String> {
    let s: Vec<&str> = path.split('/').filter(|x| !x.is_empty()).collect();
    match s.as_slice() {
        ["library", "parts", id, ..] | ["video", "transcode", id, "stream.mp4" | "start.m3u8"] | ["library", "metadata", id, "theme"] => Some(id.to_string()),
        ["library", "metadata", _, "thumb" | "art" | "cast" | "logo" | "square", ..] | ["library", "people", _, "thumb"] | ["api", "preroll", _, "file"] => Some("p".into()),
        ["api", "downloads", id, "file", ..] => Some(format!("/api/downloads/{id}/file")),
        ["api", "log", "download"] | ["api", "maintenance", "report" | "database"] => Some(format!("/{}", s.join("/"))),
        _ => None,
    }
}

pub fn valid_username(name: &str) -> bool {
    let n = name.trim();
    (2..=40).contains(&n.chars().count())
        && n.chars().all(|c| c.is_alphanumeric() || " ._-@".contains(c))
}

/// A PIN is 4 to 8 digits.
pub fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rfc6070_style() {
        // PBKDF2-HMAC-SHA256("password", "salt", 1)
        assert_eq!(
            super::pbkdf2("password", "salt", 1),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
    }

    #[test]
    fn a_ticket_reads_back_only_unchanged_and_with_its_key() {
        let (t, _) = super::ticket("k", 3, "p", Some("tok"), 1000);
        assert_eq!(super::read_ticket("k", &t, 1000), Some((3, super::token_id("tok"), "p".into())));
        assert_eq!(super::read_ticket("other", &t, 1000), None);
        assert_eq!(super::read_ticket("", &super::ticket("", 3, "p", None, 1000).0, 1000), None);
        let mut bad = t.clone();
        let last = if bad.ends_with('0') { '1' } else { '0' };
        bad.pop();
        bad.push(last);
        assert_eq!(super::read_ticket("k", &bad, 1000), None);
        let (hash, salt) = super::hash_password("pw");
        assert!(super::verify("pw", &salt, &hash) && !super::verify("px", &salt, &hash));
        assert!(super::same("abc", "abc") && !super::same("abc", "abd") && !super::same("abc", "ab"));
    }

    #[test]
    fn reset_codes() {
        let c = super::reset_code("k", 7, "hash1", 1000);
        assert_eq!(super::reset_code_user(&c), Some(7));
        assert!(super::reset_code_ok("k", &c, "hash1", 1000 + 3599));
        assert!(!super::reset_code_ok("k", &c, "hash1", 1000 + 3600), "ended");
        assert!(!super::reset_code_ok("k", &c, "hash2", 1001), "the password was changed since");
        assert!(!super::reset_code_ok("other", &c, "hash1", 1001));
        assert!(!super::reset_code_ok("", &c, "hash1", 1001));
        let forged = c.replacen("7.", "8.", 1);
        assert!(!super::reset_code_ok("k", &forged, "hash1", 1001));
        let longer = format!("7.{}.{}", 1000 + 99999, c.rsplit('.').next().unwrap());
        assert!(!super::reset_code_ok("k", &longer, "hash1", 1001));
        for bad in ["", "7", "7.x.y", "x.1.2"] { assert!(!super::reset_code_ok("k", bad, "hash1", 0), "{bad}"); }
    }
}
