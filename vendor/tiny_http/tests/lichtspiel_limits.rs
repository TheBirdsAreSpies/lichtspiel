// lichtspiel: tests for the header limits and the quick close of TLS sent to the plain HTTP port.
// Run with `cargo test -p tiny_http --test lichtspiel_limits`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

/// A server on a free port that answers every request with 200 "ok", and a client connected to it.
fn server_and_client() -> TcpStream {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    thread::spawn(move || {
        for rq in server.incoming_requests() {
            let _ = rq.respond(tiny_http::Response::from_string("ok"));
        }
    });
    let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    client
}

/// Sends `data` from another thread (the server may close before it is all sent) and returns what
/// the server answered before closing, or an error message if it neither answered nor closed within 1 s.
fn send_and_wait_for_close(mut client: TcpStream, data: Vec<u8>) -> Result<String, String> {
    let mut writer = client.try_clone().unwrap();
    thread::spawn(move || {
        let _ = writer.write_all(&data);
        let _ = writer.flush();
    });
    let start = Instant::now();
    let mut answer = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match client.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => answer.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return Err(format!("connection still open after {:?}, answer so far: {:?}", start.elapsed(), String::from_utf8_lossy(&answer)));
            }
            Err(_) => break, // reset by the server: closed
        }
    }
    let took = start.elapsed();
    if took > Duration::from_millis(1500) {
        return Err(format!("closed only after {took:?}"));
    }
    Ok(String::from_utf8_lossy(&answer).into_owned())
}

fn assert_refused(answer: &str) {
    assert!(answer.is_empty() || answer.starts_with("HTTP/1.1 431"), "expected 431 or a plain close, got {:?}", answer);
}

#[test]
fn one_huge_header_line_is_refused_quickly() {
    let mut data = b"GET / HTTP/1.1\r\nHost: x\r\nX-Big: ".to_vec();
    data.extend(std::iter::repeat_n(b'a', 1024 * 1024)); // 1 MB, no line end
    let answer = send_and_wait_for_close(server_and_client(), data).unwrap();
    assert_refused(&answer);
}

#[test]
fn a_thousand_headers_are_refused() {
    let mut data = b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n".to_vec();
    for i in 0..1000 {
        data.extend(format!("X-H{i}: v\r\n").bytes());
    }
    data.extend(b"\r\n");
    let answer = send_and_wait_for_close(server_and_client(), data).unwrap();
    assert_refused(&answer);
}

#[test]
fn tls_hello_on_the_plain_port_is_closed_quickly() {
    // The start of a TLS ClientHello: handshake record (0x16), TLS 1.0 record version, a length, ClientHello (0x01).
    let data = vec![0x16, 0x03, 0x01, 0x00, 0xc8, 0x01, 0x00, 0x00, 0xc4, 0x03, 0x03];
    let answer = send_and_wait_for_close(server_and_client(), data).unwrap();
    assert_eq!(answer, "", "nothing should be answered to TLS");
}

#[test]
fn normal_requests_still_work() {
    // A 7 KiB header and 90 headers stay under the limits; two requests on one connection.
    let mut data = format!("GET / HTTP/1.1\r\nHost: x\r\nX-Long: {}\r\n", "b".repeat(7 * 1024)).into_bytes();
    for i in 0..90 {
        data.extend(format!("X-H{i}: v\r\n").bytes());
    }
    data.extend(b"\r\nGET /two HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let answer = send_and_wait_for_close(server_and_client(), data).unwrap();
    assert_eq!(answer.matches("HTTP/1.1 200").count(), 2, "{:?}", answer);
}
