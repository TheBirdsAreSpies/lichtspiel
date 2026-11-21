//! E-mail through the SMTP server set under Settings → General: invites, "Forgot password" and
//! plugins that send mail (they get the same server with `sends_mail` in their plugin.json).
//! Plain text in UTF-8; STARTTLS, SSL/TLS from the start, or no encryption.

use crate::db::Settings;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);

/// A server and a sender are set, so mail can go out.
pub fn ready(s: &Settings) -> bool {
    !s.smtp_host.trim().is_empty() && !sender(s).is_empty()
}

/// The sender's address: `smtp_from` ("name@example.com" or "Name <name@example.com>"), else the user name.
fn sender(s: &Settings) -> String {
    let from = if s.smtp_from.trim().is_empty() { s.smtp_user.trim() } else { s.smtp_from.trim() };
    match (from.find('<'), from.rfind('>')) {
        (Some(a), Some(b)) if a < b => from[a + 1..b].trim().to_string(),
        _ => from.to_string(),
    }
}

/// Checks the SMTP settings when they are saved.
pub fn check_settings(s: &Settings) -> Result<(), String> {
    if !["starttls", "ssl", "none"].contains(&s.smtp_security.as_str()) { return Err("smtp_security must be starttls, ssl or none".into()); }
    if s.smtp_host.trim().is_empty() { return Ok(()); }
    if s.smtp_host.trim().contains(|c: char| c.is_whitespace() || c == '/' || c == ':') { return Err("The mail server is a name like smtp.example.com, without port or http://".into()); }
    if s.smtp_port == 0 { return Err("smtp_port must be 1-65535".into()); }
    let from = sender(s);
    if from.is_empty() { return Err("Enter the sender address for e-mail".into()); }
    if !valid_address(&from) { return Err("The sender address for e-mail is not an e-mail address".into()); }
    if [&s.smtp_from, &s.smtp_user].iter().any(|x| x.contains(['\r', '\n'])) { return Err("The sender address for e-mail is not an e-mail address".into()); }
    Ok(())
}

/// name@example.com, without spaces, line breaks or angle brackets.
pub fn valid_address(a: &str) -> bool {
    let a = a.trim();
    let Some((user, host)) = a.rsplit_once('@') else { return false };
    !user.is_empty() && host.contains('.') && !host.starts_with('.') && !host.ends_with('.') && a.len() <= 254
        && !a.chars().any(|c| c.is_whitespace() || c.is_control() || "<>,;\"()[]\\".contains(c))
}

/// Mails written for people get German text when the server's language is German.
pub fn german(lang: &str) -> bool {
    lang.trim().to_lowercase().starts_with("de")
}

/// Sends one plain-text mail to `to`.
pub fn send(s: &Settings, to: &str, subject: &str, text: &str) -> Result<(), String> {
    if !ready(s) { return Err("No mail server is set up (Settings → General → E-mail)".into()); }
    let to = to.trim();
    if !valid_address(to) { return Err(format!("{to} is not an e-mail address")); }
    let msg = message(s, to, subject, text);
    deliver(s, to, &msg).map_err(|e| format!("The e-mail could not be sent: {e}"))
}

fn message(s: &Settings, to: &str, subject: &str, text: &str) -> String {
    let from = sender(s);
    let shown = if s.smtp_from.contains('<') { s.smtp_from.trim().to_string() } else { format!("{} <{from}>", word(&crate::server::display_name(s))) };
    let domain = from.rsplit_once('@').map(|x| x.1).unwrap_or("localhost");
    let body = crate::server::b64(text.replace("\r\n", "\n").replace('\n', "\r\n").as_bytes());
    let lines: Vec<&str> = body.as_bytes().chunks(76).map(|c| std::str::from_utf8(c).unwrap_or("")).collect();
    format!("From: {shown}\r\nTo: <{to}>\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: <{}@{domain}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n",
        word(subject), date(crate::db::now()), crate::db::random_hex(16), lines.join("\r\n"))
}

/// A header text: as it is when plain ASCII, else as an RFC 2047 word (UTF-8, base64). Never a line break.
fn word(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if s.is_ascii() && !s.contains("=?") { return s; }
    // Short pieces, so no header line gets too long.
    let mut out = Vec::new();
    let mut piece = String::new();
    for c in s.chars() {
        if piece.len() + c.len_utf8() > 45 { out.push(format!("=?UTF-8?B?{}?=", crate::server::b64(piece.as_bytes()))); piece.clear(); }
        piece.push(c);
    }
    if !piece.is_empty() { out.push(format!("=?UTF-8?B?{}?=", crate::server::b64(piece.as_bytes()))); }
    out.join("\r\n ")
}

/// "Sat, 21 Nov 2026 14:05:09 +0000"
fn date(t: u64) -> String {
    let days = (t / 86400) as i64;
    let (h, m, sec) = (t % 86400 / 3600, t % 3600 / 60, t % 60);
    // Days to a date (Howard Hinnant's civil_from_days).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if mo <= 2 { 1 } else { 0 };
    const WD: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MO: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{}, {d:02} {} {y} {h:02}:{m:02}:{sec:02} +0000", WD[days.rem_euclid(7) as usize], MO[(mo - 1) as usize])
}

// ---------------------------------------------------------------- the conversation with the server

enum Conn {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Conn {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match self { Conn::Plain(s) => s.read(b), Conn::Tls(s) => s.read(b) }
    }
}
impl Write for Conn {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        match self { Conn::Plain(s) => s.write(b), Conn::Tls(s) => s.write(b) }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self { Conn::Plain(s) => s.flush(), Conn::Tls(s) => s.flush() }
    }
}

struct Smtp {
    conn: Conn,
    /// Read but not yet used.
    buf: Vec<u8>,
}

impl Smtp {
    fn line(&mut self) -> Result<String, String> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let l: Vec<u8> = self.buf.drain(..=i).collect();
                return Ok(String::from_utf8_lossy(&l).trim_end().to_string());
            }
            if self.buf.len() > 64 * 1024 { return Err("the mail server sent a line that is too long".into()); }
            let mut b = [0u8; 4096];
            let n = self.conn.read(&mut b).map_err(io_err)?;
            if n == 0 { return Err("the mail server closed the connection".into()); }
            self.buf.extend_from_slice(&b[..n]);
        }
    }

    /// One answer (all its lines): (code, the lines' text).
    fn reply(&mut self) -> Result<(u16, Vec<String>), String> {
        let mut lines = Vec::new();
        loop {
            let l = self.line()?;
            let code = l.get(..3).and_then(|c| c.parse::<u16>().ok()).ok_or_else(|| format!("the mail server answered something unexpected: {}", short(&l)))?;
            lines.push(l.get(4..).unwrap_or("").to_string());
            if l.as_bytes().get(3) != Some(&b'-') { return Ok((code, lines)); }
        }
    }

    fn send(&mut self, cmd: &str) -> Result<(), String> {
        self.conn.write_all(cmd.as_bytes()).and_then(|_| self.conn.write_all(b"\r\n")).and_then(|_| self.conn.flush()).map_err(io_err)
    }

    /// Sends a command and wants an answer starting with `want` (2 = 2xx, 3 = 3xx). `shown`: what an error names it as.
    fn cmd(&mut self, cmd: &str, want: u16, shown: &str) -> Result<Vec<String>, String> {
        self.send(cmd)?;
        self.expect(want, shown)
    }

    fn expect(&mut self, want: u16, shown: &str) -> Result<Vec<String>, String> {
        let (code, lines) = self.reply()?;
        if code / 100 != want { return Err(format!("{shown}: {code} {}", short(&lines.join(" ")))); }
        Ok(lines)
    }
}

fn io_err(e: std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "the mail server did not answer in time".into(),
        _ => e.to_string(),
    }
}

fn short(s: &str) -> String {
    s.chars().take(200).collect()
}

fn tls(host: &str, tcp: TcpStream) -> Result<Conn, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions().map_err(|e| e.to_string())?
        .with_root_certificates(roots).with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| format!("{host} is not a server name"))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let mut s = rustls::StreamOwned::new(conn, tcp);
    // The handshake now, so a certificate that does not fit is named as such.
    while s.conn.is_handshaking() {
        s.conn.complete_io(&mut s.sock).map_err(|e| format!("encrypted connection failed ({e})"))?;
    }
    Ok(Conn::Tls(Box::new(s)))
}

fn deliver(s: &Settings, to: &str, msg: &str) -> Result<(), String> {
    let host = s.smtp_host.trim();
    let addrs: Vec<_> = (host, s.smtp_port).to_socket_addrs().map_err(|e| format!("{host} was not found ({e})"))?.collect();
    let mut last = format!("{host} was not found");
    let mut tcp = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, TIMEOUT) {
            Ok(t) => { tcp = Some(t); break; }
            Err(e) => last = format!("no connection to {host}:{} ({})", s.smtp_port, io_err(e)),
        }
    }
    let tcp = tcp.ok_or(last)?;
    tcp.set_read_timeout(Some(TIMEOUT)).ok();
    tcp.set_write_timeout(Some(TIMEOUT)).ok();
    let conn = if s.smtp_security == "ssl" { tls(host, tcp)? } else { Conn::Plain(tcp) };
    let mut c = Smtp { conn, buf: Vec::new() };
    c.expect(2, "the mail server did not greet")?;
    let mut caps = c.cmd("EHLO localhost", 2, "EHLO")?;
    if s.smtp_security == "starttls" {
        if !caps.iter().any(|l| l.eq_ignore_ascii_case("STARTTLS")) {
            return Err(format!("{host} does not offer STARTTLS on port {}; choose SSL/TLS (usually port 465) or no encryption", s.smtp_port));
        }
        c.cmd("STARTTLS", 2, "STARTTLS")?;
        // Nothing may come before the encryption starts.
        if !c.buf.is_empty() { return Err("the mail server sent data before the encryption started".into()); }
        let Conn::Plain(tcp) = c.conn else { unreachable!() };
        c.conn = tls(host, tcp)?;
        caps = c.cmd("EHLO localhost", 2, "EHLO")?;
    }
    let user = s.smtp_user.trim();
    if !user.is_empty() {
        let auth = caps.iter().find(|l| l.to_uppercase().starts_with("AUTH")).map(|l| l.to_uppercase()).unwrap_or_default();
        let b64 = crate::server::b64;
        if auth.split([' ', '=']).any(|m| m == "PLAIN") || auth.is_empty() {
            c.cmd(&format!("AUTH PLAIN {}", b64(format!("\0{user}\0{}", s.smtp_password).as_bytes())), 2, "sign-in at the mail server was refused")?;
        } else {
            c.cmd("AUTH LOGIN", 3, "AUTH LOGIN")?;
            c.cmd(&b64(user.as_bytes()), 3, "sign-in at the mail server was refused")?;
            c.cmd(&b64(s.smtp_password.as_bytes()), 2, "sign-in at the mail server was refused")?;
        }
    }
    c.cmd(&format!("MAIL FROM:<{}>", sender(s)), 2, "the sender was refused")?;
    c.cmd(&format!("RCPT TO:<{to}>"), 2, "the recipient was refused")?;
    c.cmd("DATA", 3, "DATA")?;
    // A line that starts with a dot gets a second one (it would end the mail otherwise).
    let data: String = msg.split("\r\n").map(|l| if l.starts_with('.') { format!(".{l}") } else { l.to_string() }).collect::<Vec<_>>().join("\r\n");
    c.conn.write_all(data.trim_end_matches("\r\n").as_bytes()).map_err(io_err)?;
    c.cmd("\r\n.", 2, "the mail was not accepted")?;
    let _ = c.send("QUIT");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert!(valid_address("anna@example.com"));
        assert!(valid_address(" a.b+c@mail.example.de "));
        for bad in ["", "anna", "anna@", "@example.com", "anna@localhost", "a b@example.com", "a@example.com\r\nBcc: x@y.z", "<a@example.com>", "a@example.com,b@example.com"] {
            assert!(!valid_address(bad), "{bad:?}");
        }
    }

    #[test]
    fn sender_and_check() {
        let mut s = Settings::default();
        assert!(!ready(&s));
        assert!(check_settings(&s).is_ok());
        s.smtp_host = "smtp.example.com".into();
        assert!(check_settings(&s).is_err(), "a server needs a sender");
        s.smtp_user = "kino@example.com".into();
        assert!(ready(&s));
        assert_eq!(sender(&s), "kino@example.com");
        s.smtp_from = "Kino <film@example.com>".into();
        assert_eq!(sender(&s), "film@example.com");
        assert!(check_settings(&s).is_ok());
        s.smtp_from = "film@example.com\r\nBcc: all@example.com".into();
        assert!(check_settings(&s).is_err());
        s.smtp_from = String::new();
        s.smtp_security = "tls".into();
        assert!(check_settings(&s).is_err());
        s.smtp_security = "ssl".into();
        s.smtp_host = "https://smtp.example.com".into();
        assert!(check_settings(&s).is_err());
    }

    #[test]
    fn header_words() {
        assert_eq!(word("Invitation"), "Invitation");
        assert_eq!(word("Line\r\nBcc: x"), "Line  Bcc: x");
        let w = word("Einladung zu Lichtspiel – für dich");
        assert!(w.starts_with("=?UTF-8?B?"), "{w}");
        assert!(w.split("\r\n ").all(|p| p.len() < 76), "{w}");
    }

    #[test]
    fn dates() {
        assert_eq!(date(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(date(1795269909), "Sat, 21 Nov 2026 14:05:09 +0000");
        assert_eq!(date(951782400), "Tue, 29 Feb 2000 00:00:00 +0000");
    }

    #[test]
    fn message_text() {
        let mut s = Settings::default();
        s.smtp_host = "smtp.example.com".into();
        s.smtp_from = "kino@example.com".into();
        let m = message(&s, "anna@example.com", "Hallo", "Zeile eins\n.punkt\nÄ");
        assert!(m.contains("To: <anna@example.com>\r\n"));
        assert!(m.contains("Content-Transfer-Encoding: base64\r\n\r\n"));
        assert!(m.contains("<kino@example.com>"));
        assert!(m.lines().all(|l| l.len() <= 998));
    }

    /// A small SMTP server on this computer: STARTTLS is not offered, so a "none" mail goes through
    /// and a "starttls" one stops with a clear message.
    #[test]
    fn conversation() {
        let srv = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = srv.local_addr().unwrap().port();
        let got = std::thread::spawn(move || {
            let mut out = Vec::new();
            for _ in 0..2 {
                let (mut c, _) = srv.accept().unwrap();
                let mut r = std::io::BufReader::new(c.try_clone().unwrap());
                let mut said = String::new();
                c.write_all(b"220 test ESMTP\r\n").unwrap();
                let mut data = false;
                loop {
                    let mut l = String::new();
                    if std::io::BufRead::read_line(&mut r, &mut l).unwrap() == 0 { break; }
                    said.push_str(&l);
                    if data { if l == ".\r\n" { data = false; c.write_all(b"250 queued\r\n").unwrap(); } continue; }
                    let up = l.to_uppercase();
                    let answer: &[u8] = if up.starts_with("EHLO") { b"250-test\r\n250 AUTH PLAIN LOGIN\r\n" }
                        else if up.starts_with("DATA") { data = true; b"354 go\r\n" }
                        else if up.starts_with("AUTH PLAIN") { b"235 ok\r\n" }
                        else if up.starts_with("QUIT") { c.write_all(b"221 bye\r\n").unwrap(); break; }
                        else { b"250 ok\r\n" };
                    c.write_all(answer).unwrap();
                }
                out.push(said);
            }
            out
        });
        let mut s = Settings::default();
        s.smtp_host = "127.0.0.1".into();
        s.smtp_port = port;
        s.smtp_security = "none".into();
        s.smtp_user = "kino@example.com".into();
        s.smtp_password = "geheim".into();
        send(&s, "anna@example.com", "Test", "Hallo\n.\nEnde").unwrap();
        s.smtp_security = "starttls".into();
        let e = send(&s, "anna@example.com", "Test", "x").unwrap_err();
        assert!(e.contains("does not offer STARTTLS"), "{e}");
        let said = got.join().unwrap();
        assert!(said[0].contains("AUTH PLAIN AGtpbm9AZXhhbXBsZS5jb20AZ2VoZWlt\r\n"), "{}", said[0]);
        assert!(said[0].contains("MAIL FROM:<kino@example.com>\r\nRCPT TO:<anna@example.com>\r\nDATA\r\n"), "{}", said[0]);
        assert!(said[0].ends_with("\r\n.\r\nQUIT\r\n"), "{}", said[0]);
        assert!(!said[1].contains("AUTH"), "no password before the encryption: {}", said[1]);
    }
}
