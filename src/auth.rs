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
    let salt = crate::db::random_hex(16);
    (pbkdf2(password, &salt, PBKDF2_ROUNDS), salt)
}

pub fn verify(password: &str, salt: &str, hash: &str) -> bool {
    let h = pbkdf2(password, salt, PBKDF2_ROUNDS);
    // constant-time compare
    h.len() == hash.len() && h.bytes().zip(hash.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

/// A short public name for a token (the API lists and revokes tokens by this,
/// so the token itself is never shown again).
pub fn token_id(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect()
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
}
