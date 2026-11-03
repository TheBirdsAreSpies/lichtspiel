//! Two-step sign-in with an authenticator app (TOTP, RFC 6238: SHA-1, 6 digits, 30 seconds).

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h = [h[0].wrapping_add(a), h[1].wrapping_add(b), h[2].wrapping_add(c), h[3].wrapping_add(d), h[4].wrapping_add(e)];
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn hmac_sha1(key: &[u8], msg: &[u8]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..20].copy_from_slice(&sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(msg);
    let ih = sha1(&inner);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&ih);
    sha1(&outer)
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0);
    for &b in data {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(B32[((buf >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn unbase32(s: &str) -> Option<Vec<u8>> {
    let (mut buf, mut bits) = (0u32, 0);
    let mut out = vec![];
    for c in s.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let v = B32.iter().position(|b| *b as char == c.to_ascii_uppercase())? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            out.push((buf >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    Some(out)
}

pub fn new_secret() -> String {
    let raw = crate::db::random_hex(10); // 20 hex digits → 10 bytes
    let bytes: Vec<u8> = (0..10).map(|i| u8::from_str_radix(&raw[2 * i..2 * i + 2], 16).unwrap_or(0)).collect();
    base32(&bytes)
}

fn code_at(secret: &[u8], step: u64) -> u32 {
    let h = hmac_sha1(secret, &step.to_be_bytes());
    let o = (h[19] & 15) as usize;
    let n = u32::from_be_bytes([h[o] & 0x7f, h[o + 1], h[o + 2], h[o + 3]]);
    n % 1_000_000
}

/// Does `code` match now (or one step before or after, for clocks that differ a little)?
pub fn verify(secret: &str, code: &str, now_unix: u64) -> bool {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        return false;
    }
    let Some(key) = unbase32(secret) else { return false };
    let step = now_unix / 30;
    (step.saturating_sub(1)..=step + 1).any(|s| format!("{:06}", code_at(&key, s)) == code)
}

pub fn uri(issuer: &str, user: &str, secret: &str) -> String {
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    format!("otpauth://totp/{}:{}?secret={secret}&issuer={}&algorithm=SHA1&digits=6&period=30", enc(issuer), enc(user), enc(issuer))
}

#[cfg(test)]
mod tests {
    #[test]
    fn rfc_vectors() {
        // RFC 3174: SHA-1("abc")
        assert_eq!(super::sha1(b"abc").iter().map(|b| format!("{b:02x}")).collect::<String>(), "a9993e364706816aba3e25717850c26c9cd0d89d");
        // RFC 6238 appendix B (secret "12345678901234567890", 8 digits; here the last 6): T=59 → 94287082
        let secret = super::base32(b"12345678901234567890");
        assert!(super::verify(&secret, "287082", 59));
        assert!(super::verify(&secret, "081804", 1111111109));
        assert!(!super::verify(&secret, "123456", 59));
    }
}
