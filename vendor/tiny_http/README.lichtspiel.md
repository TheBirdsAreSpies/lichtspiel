# tiny_http 0.12.0, Lichtspiel copy

This is tiny_http 0.12.0 from crates.io (MIT or Apache-2.0, see LICENSE-MIT and LICENSE-APACHE),
used through `[patch.crates-io]` in the top-level Cargo.toml. Every change is marked `// lichtspiel:`.

What was changed, and why:

- `src/client.rs`: a request line or header line may be at most 8 KiB, and a request may have at
  most 100 headers. Beyond that the server answers `431 Request Header Fields Too Large` and closes
  the connection. Before, one header without a line end was read into memory without limit (a test
  grew the server from about 24 MB to 332 MB).
- `src/client.rs`: on the plain HTTP port, a connection whose first byte is 0x16 (the start of a
  TLS handshake, for example a browser opening https:// on the http port) is closed at once.
  Before, the server waited for a line end that never came and the browser hung. The HTTPS port is
  not affected.
- `src/client.rs`, `src/lib.rs`, `src/connection.rs`, `src/util/refined_tcp_stream.rs`: a client has
  30 s for a request's header, counted from its first byte (and a new connection 30 s for its first
  one, with HTTPS including the handshake). Before, a client that sent its header a byte at a time,
  or a connection that never sent anything, held a thread for ever. Past the deadline the server
  answers `408` and closes, or closes without an answer if nothing of a request came. To keep
  the clock, every accepted socket gets a 30 s read timeout; a read that times out between
  requests just waits on, so kept-open connections still have no idle limit. A request body that
  stops coming for 30 s ends with an error. The read half of a connection is now the original
  socket and the write half the clone, because on Windows a clone keeps a read timeout of its own.
  `tiny_http::set_header_deadline` changes the 30 s (the tests use it).
- `tests/lichtspiel_limits.rs`: tests for the header limits and the TLS check;
  `tests/lichtspiel_deadline.rs`: tests for the deadline (`cargo test -p tiny_http --tests`).
- Left out: `benches/`, `examples/` and the dev-dependencies they needed (fdlimit, rustc-serialize,
  sha1), so the tests run although this crate is not a workspace member.

Known upstream failures, unchanged by this copy: `tests/promptness.rs` `content_length_http10` and
`expect_continue` fail on macOS with or without these changes; on Windows all of `tests/promptness.rs`
fails (it connects to 0.0.0.0).

To update tiny_http: copy the new version here, apply the `// lichtspiel:` changes again, run the
tests above, and check that Cargo.lock lists tiny_http without a `source` line.
