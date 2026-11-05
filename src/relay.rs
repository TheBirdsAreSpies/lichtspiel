//! Relay: reaching the server from outside when the router can't forward a port.
//!
//! Two parts of the same program:
//!  * `lichtspiel --relay` runs on a computer that can be reached from the internet. It only passes
//!    TCP connections on and reads none of the traffic (with HTTPS the connection is private from
//!    the caller all the way to the home server).
//!  * The home server (Settings -> Network -> Relay) calls that computer, keeps a few idle
//!    connections open and, when a caller arrives, hands the connection to its own web server.
//!
//! The shared secret is never sent: the relay sends a random number and the home server answers
//! with a hash of the number and the secret.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::server::App;

/// Idle connections the home server keeps open to the relay.
const WORKERS: usize = 4;
/// An idle connection is replaced after this long (so a dead one is not kept for ever).
const IDLE_LIFE: Duration = Duration::from_secs(90);
/// The relay waits this long for a free connection of the home server.
const WAIT_FOR_HOME: Duration = Duration::from_secs(4);

/// Calls that came through the relay: our own end of the connection to the web server -> the caller.
/// Nobody outside this program can put an entry here, so the address can be believed.
static RELAYED: Mutex<Option<HashMap<SocketAddr, IpAddr>>> = Mutex::new(None);

/// The caller behind a connection that the relay passed on (None = an ordinary connection).
pub fn real_client(peer: SocketAddr) -> Option<IpAddr> {
    RELAYED.lock().unwrap().as_ref()?.get(&peer).copied()
}

fn proof(secret: &str, nonce: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"lichtspiel-relay\0");
    h.update(secret.as_bytes());
    h.update(b"\0");
    h.update(nonce.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// One line, byte by byte (a buffered reader would swallow what follows the line).
fn read_line(s: &mut TcpStream) -> Option<String> {
    let mut out = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match s.read(&mut b) {
            Ok(1) => {
                if b[0] == b'\n' { return Some(String::from_utf8_lossy(&out).trim_end_matches('\r').to_string()); }
                out.push(b[0]);
                if out.len() > 300 { return None; }
            }
            _ => return None,
        }
    }
}

/// Passes bytes both ways until one side ends.
fn splice(a: TcpStream, b: TcpStream) {
    let _ = a.set_read_timeout(None);
    let _ = b.set_read_timeout(None);
    let _ = a.set_nodelay(true);
    let _ = b.set_nodelay(true);
    let (Ok(mut a_in), Ok(mut b_in)) = (a.try_clone(), b.try_clone()) else { return };
    let (mut a_out, mut b_out) = (a, b);
    let t = std::thread::spawn(move || {
        let _ = std::io::copy(&mut a_in, &mut b_out);
        let _ = b_out.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut b_in, &mut a_out);
    let _ = a_out.shutdown(Shutdown::Write);
    let _ = t.join();
}

// ---------------------------------------------------------------- the relay

/// Runs the relay for ever: callers come in on `public`, the home server on `tunnel`.
pub fn run_relay(public: &str, tunnel: &str, secret: &str) -> Result<(), String> {
    if secret.len() < 8 { return Err("the relay secret must have at least 8 characters".into()); }
    let pub_l = TcpListener::bind(public).map_err(|e| format!("could not listen on {public}: {e}"))?;
    let tun_l = TcpListener::bind(tunnel).map_err(|e| format!("could not listen on {tunnel}: {e}"))?;
    let idle: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
    let secret = secret.to_string();
    {
        let idle = idle.clone();
        std::thread::spawn(move || {
            for s in tun_l.incoming().flatten() {
                let (idle, secret) = (idle.clone(), secret.clone());
                std::thread::spawn(move || accept_home(s, &secret, &idle));
            }
        });
    }
    println!("Relay is running: callers on {public}, the home server on {tunnel}.");
    for s in pub_l.incoming().flatten() {
        let idle = idle.clone();
        std::thread::spawn(move || pass_on(s, &idle));
    }
    Ok(())
}

fn accept_home(mut s: TcpStream, secret: &str, idle: &Mutex<Vec<TcpStream>>) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let nonce = crate::db::random_hex(16);
    if writeln!(s, "NONCE {nonce}").is_err() { return; }
    let Some(line) = read_line(&mut s) else { return };
    let want = proof(secret, &nonce);
    let got = line.strip_prefix("AUTH ").unwrap_or("");
    // Compared without stopping at the first difference.
    let same = got.len() == want.len() && got.bytes().zip(want.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
    if !same { let _ = writeln!(s, "NO"); return; }
    if writeln!(s, "OK").is_err() { return; }
    let mut idle = idle.lock().unwrap();
    if idle.len() < 32 { idle.push(s); }
}

fn pass_on(caller: TcpStream, idle: &Mutex<Vec<TcpStream>>) {
    let Ok(peer) = caller.peer_addr() else { return };
    let started = Instant::now();
    loop {
        let home = idle.lock().unwrap().pop();
        match home {
            Some(mut h) => {
                // The home server answers READY when it has the connection to its web server.
                let _ = h.set_write_timeout(Some(Duration::from_secs(3)));
                let _ = h.set_read_timeout(Some(Duration::from_secs(3)));
                if writeln!(h, "PROXY {}", peer.ip()).is_ok() && read_line(&mut h).as_deref() == Some("READY") {
                    splice(caller, h);
                    return;
                }
                // a dead connection: try the next one
            }
            None => {
                if started.elapsed() > WAIT_FOR_HOME { return; }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

// ---------------------------------------------------------------- the home server

/// Calls the relay for ever (while it is set in the settings) and passes callers on to the web server.
pub fn start_agent(app: Arc<App>) {
    for _ in 0..WORKERS {
        let app = app.clone();
        std::thread::spawn(move || worker(app));
    }
}

fn worker(app: Arc<App>) {
    let mut last_error = String::new();
    loop {
        let (server, secret) = { let st = app.settings(); (st.relay_server.trim().to_string(), st.relay_secret.clone()) };
        if server.is_empty() || secret.is_empty() {
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        match wait_for_caller(&server, &secret) {
            Ok(Some((relay, ip))) => {
                last_error.clear();
                std::thread::spawn(move || hand_over(relay, ip));
            }
            Ok(None) => {} // nobody came: a fresh connection
            Err(e) => {
                if e != last_error { crate::log::add(format!("relay {server}: {e}")); last_error = e; }
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

/// One idle connection: Ok(Some) when a caller came, Ok(None) when it timed out.
fn wait_for_caller(server: &str, secret: &str) -> Result<Option<(TcpStream, IpAddr)>, String> {
    let addr = server.to_socket_addrs().map_err(|e| format!("unknown address ({e})"))?.next().ok_or("unknown address")?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map_err(|e| format!("not reachable ({e})"))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let nonce = read_line(&mut s).and_then(|l| l.strip_prefix("NONCE ").map(str::to_string)).ok_or("no answer from the relay")?;
    writeln!(s, "AUTH {}", proof(secret, &nonce)).map_err(|e| e.to_string())?;
    if read_line(&mut s).as_deref() != Some("OK") { return Err("the relay did not accept the secret".into()); }
    let _ = s.set_read_timeout(Some(IDLE_LIFE));
    match read_line(&mut s) {
        Some(l) => {
            let ip = l.strip_prefix("PROXY ").and_then(|x| x.trim().parse::<IpAddr>().ok()).ok_or("unexpected message from the relay")?;
            Ok(Some((s, ip)))
        }
        None => Ok(None),
    }
}

/// Where this program's own web server can be reached from this computer.
fn local_target() -> Option<String> {
    let listen = crate::server::LISTEN.lock().unwrap().clone();
    let (host, port) = listen.rsplit_once(':')?;
    let host = if matches!(host, "0.0.0.0" | "[::]" | "::" | "") { "127.0.0.1" } else { host };
    Some(format!("{host}:{port}"))
}

fn hand_over(mut relay: TcpStream, ip: IpAddr) {
    let Some(target) = local_target() else { return };
    let Some(addr) = target.to_socket_addrs().ok().and_then(|mut a| a.next()) else { return };
    let Ok(local) = TcpStream::connect_timeout(&addr, Duration::from_secs(5)) else { return };
    let Ok(mine) = local.local_addr() else { return };
    RELAYED.lock().unwrap().get_or_insert_with(HashMap::new).insert(mine, ip);
    if writeln!(relay, "READY").is_ok() { splice(relay, local); }
    if let Some(m) = RELAYED.lock().unwrap().as_mut() { m.remove(&mine); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_depends_on_secret_and_number() {
        assert_eq!(proof("abc", "1"), proof("abc", "1"));
        assert_ne!(proof("abc", "1"), proof("abd", "1"));
        assert_ne!(proof("abc", "1"), proof("abc", "2"));
        assert_eq!(proof("abc", "1").len(), 64);
    }

    #[test]
    fn a_caller_is_passed_through_to_a_server_behind_the_relay() {
        // "web server": answers every connection with its caller's view of the world.
        let web = TcpListener::bind("127.0.0.1:0").unwrap();
        let web_addr = web.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut s in web.incoming().flatten() {
                let peer = s.peer_addr().unwrap();
                let who = real_client(peer).map(|i| i.to_string()).unwrap_or("none".into());
                let mut buf = [0u8; 16];
                let n = s.read(&mut buf).unwrap_or(0);
                let _ = write!(s, "{who}:{}", String::from_utf8_lossy(&buf[..n]));
            }
        });
        *crate::server::LISTEN.lock().unwrap() = web_addr.to_string();
        let free = |_: ()| { let l = TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap().to_string() };
        let (public, tunnel) = (free(()), free(()));
        let (p, t) = (public.clone(), tunnel.clone());
        std::thread::spawn(move || { let _ = run_relay(&p, &t, "geheimes-passwort"); });
        // Wait until the relay really listens (a fixed pause is too short on a busy machine, like a NAS).
        let started = Instant::now();
        while TcpStream::connect(&tunnel).is_err() {
            assert!(started.elapsed() < Duration::from_secs(20), "the relay did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        // A home server with the wrong secret is turned away, the right one is accepted.
        assert!(wait_for_caller(&tunnel, "falsch-falsch").is_err());
        let t2 = tunnel.clone();
        let home = std::thread::spawn(move || wait_for_caller(&t2, "geheimes-passwort"));
        std::thread::sleep(Duration::from_millis(300));
        let mut caller = TcpStream::connect(&public).unwrap();
        let (relay_end, ip) = home.join().unwrap().unwrap().expect("a caller");
        assert_eq!(ip.to_string(), "127.0.0.1");
        std::thread::spawn(move || hand_over(relay_end, ip));
        caller.write_all(b"hello").unwrap();
        caller.shutdown(Shutdown::Write).unwrap();
        let mut got = String::new();
        caller.read_to_string(&mut got).unwrap();
        assert_eq!(got, "127.0.0.1:hello");
    }
}
