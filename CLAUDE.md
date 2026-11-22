# Lichtspiel

A media server for movies and TV shows (Rust server, plain-JS web app, Flutter app, Python plugins).

- Read [docs/development.md](docs/development.md) first: layout, build and test commands, conventions,
  commit style. What is still open is in [docs/todo.md](docs/todo.md); take an entry off when it is done.
- Before committing, run what fits the change: `cargo test --release --no-default-features`,
  `python3 -I -m unittest discover -s plugins/<name>`, `npm run check` (lint and type check of `web/`).
- Line endings are mixed: a few files are CRLF (`web/lang/de.js`, `web/app.js`, all of `src/api/routes/` …;
  `git ls-files --eol`). Keep each file as it is: no `sed -i` on CRLF files, Python with `newline=''`,
  and check `git diff --stat` for whole-file changes.
- New or changed UI text in `web/` needs its German text in `web/lang/de.js`; in the app
  (`clients/lichtspiel`) it goes through `tr()` with its German text in `lib/lang/de.dart`.
- New feature or behaviour: update the matching guide in `docs/` (and `README.md` if it is a headline feature).
- Don't run `cargo fmt` over the tree.
- **Pushing to `main` deploys to the NAS.** Push only when asked.
