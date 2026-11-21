//! Who is calling and from where: network lists, trusted proxies, local addresses, and which files and
//! addresses the server may touch.

use super::*;

/// `ip` is in the list of addresses and ranges ("10.0.0.2, 172.16.0.0/12 ::1").
/// Checks a list of addresses and ranges (commas or spaces between them): every entry must be well-formed.
pub fn check_network_list(list: &str) -> Result<(), String> {
    for e in list.split(|c: char| c == ',' || c.is_whitespace()).filter(|e| !e.is_empty()) {
        let (addr, bits) = match e.split_once('/') { Some((a, b)) => (a, Some(b)), None => (e, None) };
        let ip: IpAddr = addr.parse().map_err(|_| format!("'{e}' is not an address or range"))?;
        if let Some(b) = bits {
            let max = if ip.is_ipv4() { 32 } else { 128 };
            if b.parse::<u32>().map_or(true, |b| b > max) { return Err(format!("'{e}' has no valid prefix length")); }
        }
    }
    Ok(())
}


/// Checks the "no sign-in" networks: every entry must be an address or range inside a private, loopback or
/// link-local space, because people from there get admin rights without a password.
pub fn check_open_networks(list: &str) -> Result<(), String> {
    for e in list.split(|c: char| c == ',' || c.is_whitespace()).filter(|e| !e.is_empty()) {
        let (addr, bits) = match e.split_once('/') { Some((a, b)) => (a, Some(b)), None => (e, None) };
        let ip: IpAddr = addr.parse().map_err(|_| format!("'{e}' is not an address or range"))?;
        let bits = match bits { Some(b) => Some(b.parse::<u32>().map_err(|_| format!("'{e}' has no valid prefix length"))?), None => None };
        let private = match ip {
            IpAddr::V4(a) => {
                if bits.is_some_and(|b| b > 32) { return Err(format!("'{e}' has no valid prefix length")); }
                let b = bits.unwrap_or(32);
                let o = a.octets();
                (o[0] == 10 && b >= 8) || (o[0] == 172 && (16..32).contains(&o[1]) && b >= 12) || (o[0] == 192 && o[1] == 168 && b >= 16)
                    || (o[0] == 127 && b >= 8) || (o[0] == 169 && o[1] == 254 && b >= 16)
            }
            IpAddr::V6(a) => {
                if bits.is_some_and(|b| b > 128) { return Err(format!("'{e}' has no valid prefix length")); }
                let b = bits.unwrap_or(128);
                let s0 = a.segments()[0];
                (a.is_loopback() && b >= 128) || ((s0 & 0xfe00) == 0xfc00 && b >= 7) || ((s0 & 0xffc0) == 0xfe80 && b >= 10)
            }
        };
        if !private { return Err(format!("'{e}' reaches beyond your home network; only private ranges such as 192.168.0.0/16 are allowed")); }
    }
    Ok(())
}


pub fn ip_in_list(list: &str, ip: IpAddr) -> bool {
    let ip = match ip { IpAddr::V6(a) => a.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(a)), v4 => v4 };
    list.split(|c: char| c == ',' || c.is_whitespace()).filter(|e| !e.is_empty()).any(|e| {
        let (addr, bits) = match e.split_once('/') { Some((a, b)) => (a, b.parse::<u32>().ok()), None => (e, None) };
        let Ok(net) = addr.parse::<IpAddr>() else { return false };
        match (net, ip) {
            (IpAddr::V4(n), IpAddr::V4(a)) => {
                let b = bits.unwrap_or(32).min(32);
                let mask = if b == 0 { 0 } else { u32::MAX << (32 - b) };
                (u32::from(n) & mask) == (u32::from(a) & mask)
            }
            (IpAddr::V6(n), IpAddr::V6(a)) => {
                let b = bits.unwrap_or(128).min(128);
                let mask = if b == 0 { 0 } else { u128::MAX << (128 - b) };
                (u128::from(n) & mask) == (u128::from(a) & mask)
            }
            _ => false,
        }
    })
}


/// One address from a forwarding header: "1.2.3.4", "1.2.3.4:5678", "[::1]:80", "\"[::1]\"".
fn parse_fwd(v: &str) -> Option<IpAddr> {
    let v = v.trim().trim_matches('"');
    if let Ok(ip) = v.parse() { return Some(ip); }
    if let Some(rest) = v.strip_prefix('[') { return rest.split(']').next()?.parse().ok(); }
    v.rsplit_once(':').and_then(|(a, _)| a.parse().ok())
}


/// The real caller: the connecting address, or, when that is a trusted reverse proxy, the
/// address it forwarded. The flag is false when a forwarding header came from someone who is
/// not trusted, so the address can't be relied on (it then never counts as local).
pub fn client_address(peer: Option<IpAddr>, xff: &[String], real_ip: Option<&str>, forwarded: &[String], trusted: &str) -> (Option<IpAddr>, bool) {
    if xff.is_empty() && real_ip.is_none() && forwarded.is_empty() {
        // A trusted proxy that says nothing about the caller: the caller is unknown, and the
        // proxy's own (private) address must not make everyone look like they are at home.
        let from_proxy = peer.is_some_and(|p| ip_in_list(trusted, p));
        return (peer, !from_proxy);
    }
    let Some(p) = peer else { return (None, false) };
    if !ip_in_list(trusted, p) { return (peer, false); }
    // X-Forwarded-For: proxies add on the right, so the first address from the right that is
    // not one of our proxies is the caller (anything left of it may be made up).
    let chain: Vec<&str> = xff.iter().flat_map(|h| h.split(',')).collect();
    if !chain.is_empty() {
        for part in chain.iter().rev() {
            match parse_fwd(part) {
                Some(ip) if ip_in_list(trusted, ip) => continue,
                Some(ip) => return (Some(ip), true),
                None => return (peer, false),
            }
        }
        return (parse_fwd(chain[0]), true);
    }
    if let Some(ip) = real_ip.and_then(parse_fwd) { return (Some(ip), true); }
    let fors: Vec<&str> = forwarded.iter().flat_map(|h| h.split(',')).filter_map(|el| {
        el.split(';').find_map(|kv| kv.trim().strip_prefix("for=").or_else(|| kv.trim().strip_prefix("For=")))
    }).collect();
    for f in fors.iter().rev() {
        match parse_fwd(f) {
            Some(ip) if ip_in_list(trusted, ip) => continue,
            Some(ip) => return (Some(ip), true),
            None => return (peer, false),
        }
    }
    (peer, false)
}


/// The file really lies inside one of these folders, also after following links.
pub fn inside_folders<S: AsRef<Path>>(folders: &[S], p: &Path) -> bool {
    let Ok(real) = p.canonicalize() else { return false };
    folders.iter().any(|f| f.as_ref().canonicalize().is_ok_and(|f| real.starts_with(f)))
}


/// Looks up a host for a download a user asked for (a picture's address) and refuses this computer,
/// the home network, link-local (cloud metadata) and other special addresses. ureq asks this again
/// for every redirect, so a redirect can't lead there either.
pub fn public_only(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    let all: Vec<std::net::SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(netloc)?.collect();
    let special = |ip: IpAddr| {
        let ip = match ip { IpAddr::V6(a) => a.to_ipv4_mapped().map_or(ip, IpAddr::V4), _ => ip };
        ip_is_local(ip) || ip.is_unspecified() || ip.is_multicast()
            || matches!(ip, IpAddr::V4(a) if a.is_broadcast() || a.octets()[0] == 0 || (a.octets()[0] == 100 && a.octets()[1] & 0xc0 == 64))
    };
    if all.is_empty() || all.iter().any(|a| special(a.ip())) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "addresses on this computer or the home network can't be used"));
    }
    Ok(all)
}


pub fn ip_is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => a.is_loopback() || a.is_private() || a.is_link_local(),
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4_mapped() { return ip_is_local(IpAddr::V4(v4)); }
            let first = a.segments()[0];
            a.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    fn ip(s: &str) -> Option<IpAddr> { s.parse().ok() }
    fn v(s: &[&str]) -> Vec<String> { s.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn network_lists_must_be_well_formed() {
        assert!(check_network_list("").is_ok());
        assert!(check_network_list("100.64.0.0/10, 203.0.113.7 2001:db8::/32").is_ok());
        assert!(check_network_list("10.0.0.0/33").is_err());
        assert!(check_network_list("not-an-ip").is_err());
    }

    #[test]
    fn open_networks_must_be_private() {
        assert!(check_open_networks("").is_ok());
        // HTTPS settings: nothing = plain HTTP, half or wrong files = a clear error.
        assert_eq!(read_tls("", " "), Ok(None));
        assert!(read_tls("a.pem", "").unwrap_err().contains("both"));
        assert!(read_tls("/no/such/cert.pem", "/no/such/key.pem").unwrap_err().contains("could not read"));
        let dir = std::env::temp_dir().join(format!("lichtspiel-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (c, k) = (dir.join("c.pem"), dir.join("k.pem"));
        std::fs::write(&c, "hello").unwrap();
        std::fs::write(&k, "hello").unwrap();
        assert!(read_tls(&c.to_string_lossy(), &k.to_string_lossy()).unwrap_err().contains("not a PEM certificate"));
        std::fs::write(&c, "-----BEGIN CERTIFICATE-----
AAAA
-----END CERTIFICATE-----
").unwrap();
        assert!(read_tls(&c.to_string_lossy(), &k.to_string_lossy()).unwrap_err().contains("not a PEM private key"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(check_open_networks("192.168.1.0/24, 10.0.0.5 fd00::/8").is_ok());
        assert!(check_open_networks("0.0.0.0/0").is_err());
        assert!(check_open_networks("8.8.8.8").is_err());
        assert!(check_open_networks("192.168.0.0/8").is_err());
        assert!(check_open_networks("172.15.0.1").is_err());
        assert!(check_open_networks("nonsense").is_err());
    }

    #[test]
    fn lists() {
        assert!(ip_in_list("172.16.0.0/12", "172.18.0.3".parse().unwrap()));
        assert!(!ip_in_list("172.16.0.0/12", "172.32.0.1".parse().unwrap()));
        assert!(ip_in_list("10.0.0.1, ::1", "::1".parse().unwrap()));
        assert!(ip_in_list("127.0.0.1", "::ffff:127.0.0.1".parse().unwrap()));
        assert!(!ip_in_list("", "127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn forwarding() {
        let proxy = ip("172.18.0.2");
        // No proxy in between: the connecting address.
        assert_eq!(client_address(ip("192.168.1.20"), &[], None, &[], ""), (ip("192.168.1.20"), true));
        // A forwarding header from a machine that isn't trusted can't be believed.
        assert_eq!(client_address(proxy, &v(&["192.168.1.20"]), None, &[], "").1, false);
        // Trusted proxy: the rightmost address that isn't a proxy; a made-up one on the left is ignored.
        assert_eq!(client_address(proxy, &v(&["192.168.1.20, 203.0.113.9"]), None, &[], "172.16.0.0/12"), (ip("203.0.113.9"), true));
        assert_eq!(client_address(proxy, &v(&["192.168.1.20"]), None, &[], "172.16.0.0/12"), (ip("192.168.1.20"), true));
        assert_eq!(client_address(proxy, &[], Some("203.0.113.9"), &[], "172.18.0.2"), (ip("203.0.113.9"), true));
        assert_eq!(client_address(proxy, &[], None, &v(&["for=\"[2001:db8::1]:4711\";proto=https"]), "172.18.0.2"), (ip("2001:db8::1"), true));
        assert_eq!(client_address(proxy, &v(&["garbage"]), None, &[], "172.18.0.2").1, false);
        // The proxy forgot the header: not trusted, so never "home network".
        assert_eq!(client_address(proxy, &[], None, &[], "172.16.0.0/12").1, false);
    }
}
