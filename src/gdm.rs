//! "G'Day Mate" discovery: apps on the home network find the server without typing an address.
//! The server answers search packets on UDP 32414 and announces itself once on start (multicast 239.0.0.250).

use crate::server::{App, VERSION};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

const GROUP: Ipv4Addr = Ipv4Addr::new(239, 0, 0, 250);
const SEARCH_PORT: u16 = 32414;
const HELLO_PORT: u16 = 32413;

/// The headers that describe this server.
fn description(app: &App) -> String {
    let (name, id) = { let db = app.db.lock().unwrap(); (crate::server::display_name(&db.settings), db.machine_id.clone()) };
    let port = *app.port.lock().unwrap();
    format!("Content-Type: plex/media-server\r\nName: {name}\r\nPort: {port}\r\nResource-Identifier: {id}\r\nUpdated-At: {}\r\nVersion: {VERSION}\r\n", crate::db::now())
}

/// Is this a search packet from a client?
fn is_search(packet: &str) -> bool {
    packet.starts_with("M-SEARCH * HTTP/1.")
}

/// Is the sender on the home network (a private address or one of the "home network" settings)?
/// Others are ignored, so a forged sender can't make the server send packets out to the internet.
fn answers(from: SocketAddr, lan_networks: &str) -> bool {
    crate::server::ip_is_local(from.ip()) || crate::server::ip_in_list(lan_networks, from.ip())
}

/// Answers per sender address and second. A few, not one: several apps on one computer may search
/// at the same moment. More are dropped, so a flood of searches can't keep the server busy.
const ANSWERS_PER_SECOND: u32 = 5;

/// Counts answers per sender address within the current second.
#[derive(Default)]
struct Limit(HashMap<IpAddr, (Instant, u32)>);

impl Limit {
    fn allow(&mut self, ip: IpAddr, now: Instant) -> bool {
        let second = Duration::from_secs(1);
        // Forget old senders, so many addresses can't grow the list without end.
        if self.0.len() >= 1024 { self.0.retain(|_, (t, _)| now.duration_since(*t) < second); }
        if self.0.len() >= 1024 { return false; }
        let e = self.0.entry(ip).or_insert((now, 0));
        if now.duration_since(e.0) >= second { *e = (now, 0); }
        e.1 += 1;
        e.1 <= ANSWERS_PER_SECOND
    }
}

/// Starts listening (in the background). Quietly gives up when the port is taken, e.g. by another media server.
pub fn start(app: &Arc<App>) {
    let sock = match UdpSocket::bind(("0.0.0.0", SEARCH_PORT)) {
        Ok(s) => s,
        Err(e) => { elog!("network discovery is off: UDP port {SEARCH_PORT} is not free ({e})"); return; }
    };
    let _ = sock.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED);
    let a = app.clone();
    std::thread::spawn(move || {
        // Say hello once, so apps that are already open notice the server.
        if a.settings().gdm_enabled {
            let hello = format!("HELLO * HTTP/1.0\r\n{}", description(&a));
            let _ = sock.send_to(hello.as_bytes(), SocketAddr::from((GROUP, HELLO_PORT)));
        }
        let mut buf = [0u8; 1024];
        let mut limit = Limit::default();
        while let Ok((n, from)) = sock.recv_from(&mut buf) {
            let st = a.settings();
            if !st.gdm_enabled { continue; }
            if is_search(&String::from_utf8_lossy(&buf[..n])) && answers(from, &st.lan_networks) && limit.allow(from.ip(), Instant::now()) {
                let reply = format!("HTTP/1.0 200 OK\r\n{}", description(&a));
                let _ = sock.send_to(reply.as_bytes(), from);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_searches_are_answered() {
        assert!(is_search("M-SEARCH * HTTP/1.1\r\nHost: 239.0.0.250\r\n\r\n"));
        assert!(!is_search("HELLO * HTTP/1.0\r\n"));
        assert!(!is_search("GET / HTTP/1.1"));
    }
    #[test]
    fn a_sender_gets_a_few_answers_a_second_and_no_more() {
        let mut limit = Limit::default();
        let (a, b): (IpAddr, IpAddr) = ("192.168.1.20".parse().unwrap(), "192.168.1.21".parse().unwrap());
        let t = Instant::now();
        let first: Vec<bool> = (0..8).map(|_| limit.allow(a, t)).collect();
        assert_eq!(first.iter().filter(|x| **x).count(), ANSWERS_PER_SECOND as usize);
        assert!(limit.allow(b, t), "another sender is not held back");
        assert!(limit.allow(a, t + Duration::from_millis(1001)), "the next second answers again");
        // Many senders: old ones are forgotten, new ones still answered.
        for i in 0..2000u32 { limit.allow(IpAddr::from(Ipv4Addr::from(0x0a00_0000 + i)), t); }
        assert!(limit.allow("192.168.1.99".parse().unwrap(), t + Duration::from_secs(2)));
    }
    #[test]
    fn only_the_home_network_is_answered() {
        let at = |ip: &str| SocketAddr::new(ip.parse().unwrap(), 5000);
        for ip in ["192.168.1.20", "10.0.0.3", "127.0.0.1", "fe80::1", "100.64.0.7"] { assert!(answers(at(ip), "100.64.0.0/10"), "{ip}"); }
        for ip in ["8.8.8.8", "2001:db8::1", "100.64.0.7"] { assert!(!answers(at(ip), ""), "{ip}"); }
    }
}
