// lichtspiel: tests for the deadline on a request's header. A file of its own: the deadline is
// shortened for the whole test program. Run with `cargo test -p tiny_http --test lichtspiel_deadline`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_millis(600);

fn server() -> u16 {
    tiny_http::set_header_deadline(DEADLINE);
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    thread::spawn(move || {
        for rq in server.incoming_requests() {
            let _ = rq.respond(tiny_http::Response::from_string("ok"));
        }
    });
    port
}

fn connect(port: u16) -> TcpStream {
    let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    c
}

/// Reads until the server closes; returns what came and when it closed.
fn read_to_close(c: &mut TcpStream) -> (String, Duration) {
    let start = Instant::now();
    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        match c.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => panic!("still open after {:?}", start.elapsed()),
            Err(_) => break,
        }
    }
    (String::from_utf8_lossy(&out).into_owned(), start.elapsed())
}

#[test]
fn a_header_sent_a_byte_at_a_time_is_cut_off_at_the_deadline() {
    let mut c = connect(server());
    let mut w = c.try_clone().unwrap();
    let start = Instant::now();
    thread::spawn(move || {
        // One byte every 100 ms: each read is quick, the whole header never is.
        for b in b"GET / HTTP/1.1\r\nHost: x\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n\r\n" {
            if w.write_all(&[*b]).is_err() { return; }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let (answer, _) = read_to_close(&mut c);
    let took = start.elapsed();
    assert!(answer.starts_with("HTTP/1.1 408") || answer.is_empty(), "got {:?}", answer);
    assert!(took < DEADLINE * 3, "closed only after {:?}", took);
}

#[test]
fn a_connection_that_never_says_anything_is_closed_without_an_answer() {
    let mut c = connect(server());
    let (answer, took) = read_to_close(&mut c);
    assert_eq!(answer, "");
    assert!(took < DEADLINE * 3, "closed only after {:?}", took);
}

#[test]
fn a_kept_open_connection_may_wait_between_requests_longer_than_the_deadline() {
    let mut c = connect(server());
    let ask = |c: &mut TcpStream| {
        c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 1024];
        while !String::from_utf8_lossy(&got).ends_with("ok") {
            let n = c.read(&mut buf).unwrap();
            assert!(n > 0, "closed");
            got.extend_from_slice(&buf[..n]);
        }
    };
    ask(&mut c);
    thread::sleep(DEADLINE * 2);
    ask(&mut c);
}

#[test]
fn a_header_sent_in_one_piece_is_answered() {
    let mut c = connect(server());
    c.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let (answer, _) = read_to_close(&mut c);
    assert!(answer.starts_with("HTTP/1.1 200"), "got {:?}", answer);
}
