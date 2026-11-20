use ascii::AsciiString;

use std::io::Error as IoError;
use std::io::Result as IoResult;
use std::io::{BufReader, BufWriter, ErrorKind, Read};

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::common::{HTTPVersion, Method};
use crate::util::RefinedTcpStream;
use crate::util::{SequentialReader, SequentialReaderBuilder, SequentialWriterBuilder};
use crate::Request;

/// A ClientConnection is an object that will store a socket to a client
/// and return Request objects.
pub struct ClientConnection {
    // address of the client
    remote_addr: IoResult<Option<SocketAddr>>,

    // sequence of Readers to the stream, so that the data is not read in
    //  the wrong order
    source: SequentialReaderBuilder<BufReader<RefinedTcpStream>>,

    // sequence of Writers to the stream, to avoid writing response #2 before
    //  response #1
    sink: SequentialWriterBuilder<BufWriter<RefinedTcpStream>>,

    // Reader to read the next header from
    next_header_source: SequentialReader<BufReader<RefinedTcpStream>>,

    // set to true if we know that the previous request is the last one
    no_more_requests: bool,

    // true if the connection goes through SSL
    secure: bool,

    // lichtspiel: true until the first byte has been read on a plain (non-TLS) connection
    check_for_tls: bool,

    // lichtspiel: when the header now being read must be complete (None between requests)
    deadline: Option<Instant>,

    // lichtspiel: a byte of the next request has come
    started: bool,
}

// lichtspiel: how long a client may take for a request's header (the request line and the headers),
// counted from its first byte, and how long a new connection may stay silent. A client that sends its
// header a byte at a time no longer holds a thread for ever. Between requests on a kept-open
// connection there is no limit, as before (a long video answer is still being sent then).
// The socket gets a read timeout of the same length (set where the connection is accepted), so a
// read that waits wakes up to look at the clock; one that times out between requests just waits on.
// A request body that stops coming for that long ends the body (the handler sees an error).
static HEADER_DEADLINE_MS: AtomicU64 = AtomicU64::new(30_000);

/// lichtspiel: changes how long a client may take to send a request's header (30 s by default).
pub fn set_header_deadline(d: Duration) {
    HEADER_DEADLINE_MS.store(d.as_millis().max(1) as u64, Ordering::Relaxed);
}

pub(crate) fn header_deadline() -> Duration {
    Duration::from_millis(HEADER_DEADLINE_MS.load(Ordering::Relaxed))
}

// lichtspiel: limits for the request line and headers, so that one client cannot make the server
// hold an endless header in memory. 8 KiB per line is what nginx and Apache allow by default.
const MAX_LINE: usize = 8 * 1024;
const MAX_HEADERS: usize = 100;

/// Error that can happen when reading a request.
#[derive(Debug)]
enum ReadError {
    WrongRequestLine,
    WrongHeader(HTTPVersion),
    /// the client sent an unrecognized `Expect` header
    ExpectationFailed(HTTPVersion),
    ReadIoError(IoError),
    /// lichtspiel: a line longer than MAX_LINE or more than MAX_HEADERS headers
    HeaderTooLarge,
}

impl ClientConnection {
    /// Creates a new `ClientConnection` that takes ownership of the `TcpStream`.
    pub fn new(
        write_socket: RefinedTcpStream,
        mut read_socket: RefinedTcpStream,
    ) -> ClientConnection {
        let remote_addr = read_socket.peer_addr();
        let secure = read_socket.secure();

        let mut source = SequentialReaderBuilder::new(BufReader::with_capacity(1024, read_socket));
        let first_header = source.next().unwrap();

        ClientConnection {
            source,
            sink: SequentialWriterBuilder::new(BufWriter::with_capacity(1024, write_socket)),
            remote_addr,
            next_header_source: first_header,
            no_more_requests: false,
            secure,
            check_for_tls: !secure,
            // A new connection gets the same time to send its first header (with HTTPS that
            // includes the TLS handshake, which happens on the first read).
            deadline: Some(Instant::now() + header_deadline()),
            started: false,
        }
    }

    /// lichtspiel: past the deadline for the header now being read
    fn past_deadline(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }

    /// lichtspiel: the header is read; the body and the wait for the next request have no limit.
    fn end_deadline(&mut self) {
        self.started = false;
        self.deadline = None;
    }

    /// true if the connection is HTTPS
    pub fn secure(&self) -> bool {
        self.secure
    }

    /// Reads the next line from self.next_header_source.
    ///
    /// Reads until `CRLF` is reached. The next read will start
    ///  at the first byte of the new line.
    fn read_next_line(&mut self) -> Result<AsciiString, ReadError> {
        let mut buf = Vec::new();
        let mut prev_byte_was_cr = false;

        loop {
            let byte = self.next_header_source.by_ref().bytes().next();

            let byte = match byte {
                // lichtspiel: the socket's read timeout (WouldBlock on Linux and macOS, TimedOut on
                // Windows): nothing came for a while. Past the deadline that ends the connection;
                // otherwise (also between requests, where there is no deadline) read on.
                Some(Err(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    if self.past_deadline() {
                        return Err(ReadError::ReadIoError(IoError::new(ErrorKind::TimedOut, "header not complete in time")));
                    }
                    continue;
                }
                Some(b) => b.map_err(ReadError::ReadIoError)?,
                None => return Err(ReadError::ReadIoError(IoError::new(ErrorKind::ConnectionAborted, "Unexpected EOF"))),
            };

            // lichtspiel: the clock for a request's header starts with its first byte, and a header
            // sent a byte at a time is cut off at the deadline
            if !self.started {
                self.started = true;
                self.deadline.get_or_insert(Instant::now() + header_deadline());
            }
            if self.past_deadline() {
                return Err(ReadError::ReadIoError(IoError::new(ErrorKind::TimedOut, "header not complete in time")));
            }

            // lichtspiel: a client speaking TLS to the plain HTTP port starts with a handshake
            // record (0x16) and then waits for the server's answer; close at once instead of
            // waiting for a line end that never comes.
            if self.check_for_tls {
                self.check_for_tls = false;
                if byte == 0x16 {
                    return Err(ReadError::ReadIoError(IoError::new(ErrorKind::InvalidData, "TLS sent to the plain HTTP port")));
                }
            }

            if byte == b'\n' && prev_byte_was_cr {
                buf.pop(); // removing the '\r'
                return AsciiString::from_ascii(buf)
                    .map_err(|_| ReadError::ReadIoError(IoError::new(ErrorKind::InvalidInput, "Header is not in ASCII")));
            }

            // lichtspiel: refuse a line longer than MAX_LINE (plus its CRLF)
            if buf.len() > MAX_LINE {
                return Err(ReadError::HeaderTooLarge);
            }

            prev_byte_was_cr = byte == b'\r';

            buf.push(byte);
        }
    }

    /// Reads a request from the stream.
    /// Blocks until the header has been read.
    fn read(&mut self) -> Result<Request, ReadError> {
        let (method, path, version, headers) = {
            // reading the request line
            let (method, path, version) = {
                let line = self.read_next_line()?;

                parse_request_line(
                    line.as_str().trim(), // TODO: remove this conversion
                )?
            };

            // getting all headers
            let headers = {
                let mut headers = Vec::new();
                loop {
                    let line = self.read_next_line()?;

                    if line.is_empty() {
                        break;
                    };
                    // lichtspiel: refuse more than MAX_HEADERS headers
                    if headers.len() >= MAX_HEADERS {
                        return Err(ReadError::HeaderTooLarge);
                    }
                    headers.push(match FromStr::from_str(line.as_str().trim()) {
                        // TODO: remove this conversion
                        Ok(h) => h,
                        _ => return Err(ReadError::WrongHeader(version)),
                    });
                }

                headers
            };

            (method, path, version, headers)
        };
        self.end_deadline();

        // building the writer for the request
        let writer = self.sink.next().unwrap();

        // follow-up for next potential request
        let mut data_source = self.source.next().unwrap();
        std::mem::swap(&mut self.next_header_source, &mut data_source);

        // building the next reader
        let request = crate::request::new_request(
            self.secure,
            method,
            path,
            version.clone(),
            headers,
            *self.remote_addr.as_ref().unwrap(),
            data_source,
            writer,
        )
        .map_err(|e| {
            use crate::request;
            match e {
                request::RequestCreationError::CreationIoError(e) => ReadError::ReadIoError(e),
                request::RequestCreationError::ExpectationFailed => {
                    ReadError::ExpectationFailed(version)
                }
            }
        })?;

        // return the request
        Ok(request)
    }
}

impl Iterator for ClientConnection {
    type Item = Request;

    /// Blocks until the next Request is available.
    /// Returns None when no new Requests will come from the client.
    fn next(&mut self) -> Option<Request> {
        use crate::{Response, StatusCode};

        // the client sent a "connection: close" header in this previous request
        //  or is using HTTP 1.0, meaning that no new request will come
        if self.no_more_requests {
            return None;
        }

        loop {
            let rq = match self.read() {
                Err(ReadError::WrongRequestLine) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(400));
                    response
                        .raw_print(writer, HTTPVersion(1, 1), &[], false, None)
                        .ok();
                    return None; // we don't know where the next request would start,
                                 // se we have to close
                }

                Err(ReadError::WrongHeader(ver)) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(400));
                    response.raw_print(writer, ver, &[], false, None).ok();
                    return None; // we don't know where the next request would start,
                                 // se we have to close
                }

                // lichtspiel: a connection that stayed silent is closed without an answer (a
                // browser may open one ahead and never use it)
                Err(ReadError::ReadIoError(ref err)) if err.kind() == ErrorKind::TimedOut && !self.started => return None,

                Err(ReadError::ReadIoError(ref err)) if err.kind() == ErrorKind::TimedOut => {
                    // request timeout
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(408));
                    response
                        .raw_print(writer, HTTPVersion(1, 1), &[], false, None)
                        .ok();
                    return None; // closing the connection
                }

                Err(ReadError::ExpectationFailed(ver)) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(417));
                    response.raw_print(writer, ver, &[], true, None).ok();
                    return None; // TODO: should be recoverable, but needs handling in case of body
                }

                // lichtspiel: answer 431 Request Header Fields Too Large and close (the rest of
                // the header is never read, so the next request's start is unknown)
                Err(ReadError::HeaderTooLarge) => {
                    let writer = self.sink.next().unwrap();
                    let response = Response::new_empty(StatusCode(431));
                    response
                        .raw_print(writer, HTTPVersion(1, 1), &[], false, None)
                        .ok();
                    return None;
                }

                Err(ReadError::ReadIoError(_)) => return None,

                Ok(rq) => rq,
            };

            // checking HTTP version
            if *rq.http_version() > (1, 1) {
                let writer = self.sink.next().unwrap();
                let response = Response::from_string(
                    "This server only supports HTTP versions 1.0 and 1.1".to_owned(),
                )
                .with_status_code(StatusCode(505));
                response
                    .raw_print(writer, HTTPVersion(1, 1), &[], false, None)
                    .ok();
                continue;
            }

            // updating the status of the connection
            let connection_header = rq
                .headers()
                .iter()
                .find(|h| h.field.equiv("Connection"))
                .map(|h| h.value.as_str());

            let lowercase = connection_header.map(|h| h.to_ascii_lowercase());

            match lowercase {
                Some(ref val) if val.contains("close") => self.no_more_requests = true,
                Some(ref val) if val.contains("upgrade") => self.no_more_requests = true,
                Some(ref val)
                    if !val.contains("keep-alive") && *rq.http_version() == HTTPVersion(1, 0) =>
                {
                    self.no_more_requests = true
                }
                None if *rq.http_version() == HTTPVersion(1, 0) => self.no_more_requests = true,
                _ => (),
            };

            // returning the request
            return Some(rq);
        }
    }
}

/// Parses a "HTTP/1.1" string.
fn parse_http_version(version: &str) -> Result<HTTPVersion, ReadError> {
    let (major, minor) = match version {
        "HTTP/0.9" => (0, 9),
        "HTTP/1.0" => (1, 0),
        "HTTP/1.1" => (1, 1),
        "HTTP/2.0" => (2, 0),
        "HTTP/3.0" => (3, 0),
        _ => return Err(ReadError::WrongRequestLine),
    };

    Ok(HTTPVersion(major, minor))
}

/// Parses the request line of the request.
/// eg. GET / HTTP/1.1
fn parse_request_line(line: &str) -> Result<(Method, String, HTTPVersion), ReadError> {
    let mut parts = line.split(' ');

    let method = parts.next().and_then(|w| w.parse().ok());
    let path = parts.next().map(ToOwned::to_owned);
    let version = parts.next().and_then(|w| parse_http_version(w).ok());

    method
        .and_then(|method| Some((method, path?, version?)))
        .ok_or(ReadError::WrongRequestLine)
}

#[cfg(test)]
mod test {
    #[test]
    fn test_parse_request_line() {
        let (method, path, ver) = super::parse_request_line("GET /hello HTTP/1.1").unwrap();

        assert!(method == crate::Method::Get);
        assert!(path == "/hello");
        assert!(ver == crate::common::HTTPVersion(1, 1));

        assert!(super::parse_request_line("GET /hello").is_err());
        assert!(super::parse_request_line("qsd qsd qsd").is_err());
    }
}
