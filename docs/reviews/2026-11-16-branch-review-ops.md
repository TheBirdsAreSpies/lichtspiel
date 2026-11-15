# Branch review: web app, plugins and deploy, 2026-11-16

Status: all findings handled, see [the review overview](../review-2026-11-15.md).

Scope: `git diff 5b30e62 -- web plugins .gitea Dockerfile docker-compose.yml .gitignore .dockerignore`, re-read at HEAD 96ff123 on 2026-11-16.
Assumed load: one household, a few browsers and devices, a NAS deploy on every push to main.

What this change does: Picture, video and download addresses in the web app now carry a short-lived ticket from `GET /api/ticket` instead of the sign-in token. A page change cancels the old page's reads, playlists load 120 rows at a time, and a few teardown, theme-music and German-text fixes are included. The plugins get a safer Trakt token refresh, parallel webhooks with a deadline, and Kodi-style .nfo parsing. The Docker image now runs as uid 1026:100 with a new health check script, and the deploy checks that /config is writable before it replaces the container, rolls back on any failed start, and removes the old per-commit image tags.

## Must fix

### O-R1. Every pre-roll ends with a false "The video stopped before its end" message (`web/app.js` L2024-2033, `playPrerollVideo` L1673-1693)

Status: Fixed in `5357b4c`.

- **What this is:** The player's main `ended` handler decides whether a title really finished. The pre-roll video plays in the same `<video>` element while `P.it` is already the movie (`play()` sets it at L1574, before `playPreroll` at L1619).
- **Problem:** When the pre-roll ends, the main handler runs first. `reallyEnded(30 s, 7200 s)` is false, so it reports `paused` for the movie at the pre-roll's position plus a leftover `P.offset` from the last title, and shows the new toast. Before this branch, the same path reported `stopped` at the movie's full length and started the up-next countdown, so this was already wrong. The branch only changes how it goes wrong.
- **Fix:** Set `P.preroll = true` in `playPrerollVideo` and `false` in its `end()`. Start the main `ended` and `error` handlers with `if (P.preroll) return;`. That is three lines.
- **If we skip it:** Everyone who has a pre-roll set sees an error toast at the start of every movie, and the movie gets a bogus resume point.

### O-R2. Posters break once the picture ticket is renewed (`web/app.js` L66-82, every `withTok` img with `loading: 'lazy'`, e.g. L286, L881, L1008)

Status: Fixed in `5357b4c`.

- **What this is:** `withTok` puts the current picture ticket into the `src` when a card is built. The ticket lasts 60 to 90 minutes, and the app replaces it only when less than 10 minutes are left.
- **Problem:** A lazy image fetches only when it is scrolled into view. Say a library page is drawn when the ticket has 11 minutes left, and the user scrolls down 12 minutes later. The browser then asks for the poster with a dead ticket, gets a 401 and shows a broken image. A tab left open on a library or home page overnight shows nothing but broken posters below the fold. With the old token in the address this never happened.
- **Fix:** Add one capture-phase listener: `document.addEventListener('error', e => { const el = e.target, tk = TK[''] && TK[''].ticket; if (el.tagName === 'IMG' && tk && el.src.includes('ticket=') && !el.src.includes(encodeURIComponent(tk))) el.src = el.src.replace(/ticket=[^&]*/, 'ticket=' + encodeURIComponent(tk)); }, true);`. It retries once with the current ticket. Add the `visibilitychange` renewal before it so a woken tab has the new ticket.
- **If we skip it:** People who leave tabs open, which is the usual case on a TV browser or tablet, see broken posters until they reload the page.

### O-R3. The first deploy as non-root quietly turns off the graphics chip (`Dockerfile` `USER 1026:100`, `.gitea/workflows/deploy.yml` `DOCKER_ARGS` default `--device /dev/dri:/dev/dri`)

Status: Fixed in `e5b860a`.

- **What this is:** The image no longer runs as root. On DSM 7, `/dev/dri/renderD128` belongs to root and a video group (the compose comment's "937"), and the old root container could open it.
- **Problem:** The deploy passes the device but no `--group-add`. So uid 1026 can't open it, `hw_works` fails, and `effective_hw` (`src/transcode.rs` L268-297) falls back to the processor: silently on "auto", and with only a log line on "vaapi". The health check still passes, so the deploy counts as a success. On a J4125 Celeron NAS, converting 4K HEVC on the processor is not real-time.
- **Fix:** In "Replace the container", ask the device for its group through the new image and add it when `/dev/dri` is in the args: `gid=$(docker run --rm --device /dev/dri:/dev/dri --entrypoint stat ${NAME}:latest -c %g /dev/dri/renderD128 2>/dev/null || true); case "$DOCKER_ARGS" in *dev/dri*) DOCKER_ARGS="$DOCKER_ARGS ${gid:+--group-add $gid}";; esac`. This goes through the Docker daemon, so it also works when the runner itself runs in a container.
- **If we skip it:** After the push, titles that need converting stutter or fail on the TV, and nothing says why except a dashboard that no longer shows "hw".

## Should fix

### O-R4. Resuming after a long pause can fail with "could not be played" (`web/app.js` L2016-2020)

Status: Fixed in `5357b4c`.

- **What this is:** The video `error` handler restarts the stream with a fresh ticket when the ticket is (nearly) out of date. It judges that from the cache `TK['item=<id>']`, not from the ticket the video address actually uses (`P.tk`).
- **Problem:** Something else can renew the cache in between: `hoverTip` calls `ticket('item=…')` on every seek-bar hover, and `startTheme` does too. Case: pause a direct-play movie overnight, hover over the bar in the morning to find the spot (this renews `TK`), then press play. The range request with the old `P.tk` gets a 401. The handler sees a fresh `TK`, skips the reload and shows "This video could not be played. Try a lower quality."
- **Fix:** Check the address's own ticket. The end time is its second field: `if (Number(String(P.tk).split('.')[1]) - Date.now() / 1000 < 600) { load(position()); return; }`.
- **If we skip it:** People see a misleading error after a long pause and have to close and reopen the player.

### O-R5. nfo-reader no longer reads UTF-16 .nfo files (`plugins/nfo-reader/main.py` L30-34)

Status: Fixed in `9f3ed97`.

- **What this is:** The new code cuts the file after the last `>` byte, so a link after the XML is ignored.
- **Problem:** In UTF-16 the `>` is followed by a `\x00` byte. Cutting there leaves an odd number of bytes, and the parse fails. I checked this: a UTF-16 file with `<title>Ä Film</title>` gives `None` now, and the old `ET.parse` read "Ä Film". Some Windows tools write UTF-16 .nfo files.
- **Fix:** Parse the whole bytes first and cut only when that fails: `try: root = ET.fromstring(s)` / `except ET.ParseError: root = ET.fromstring(s[:s.rfind(b">") + 1])`. Add one UTF-16 case to `test_main.py`.
- **If we skip it:** Libraries with UTF-16 .nfo files lose their plot, genres and rating with no message.

### O-R6. A rollback signs out every device (`.gitea/workflows/deploy.yml` rollback branch, with `Db::hash_old_tokens` in `src/db.rs` L1021)

Status: Fixed in `e5b860a` (the rollback message explains it; the old version still can't read the new sign-ins).

- **What this is:** When the new version becomes healthy too late or not at all, the deploy starts `lichtspiel:previous`, the old image, on the same /config.
- **Problem:** The new version stores only a fingerprint of each sign-in token as soon as it has loaded and saved once. The old version compares raw tokens, so after a rollback no stored sign-in matches. Every browser, TV app and script is signed out, and a stored fingerprint now works as a token on the old version. The 10-minute wait makes this more likely: a slow but working first start, for example a big database being converted, gets killed and rolled back.
- **Fix:** The old image can't be taught to read the new format. Say it in the rollback branch (`echo "Everyone has to sign in again: the newer version had already converted the sign-ins."`) and in the commit message, so the household knows why.
- **If we skip it:** A failed deploy looks like "everything is signed out", which reads like a security incident rather than a rollback.

## Nice to have

### O-R7. "Move down" does nothing on the last loaded playlist row (`web/pages.js` L84-88)

Status: Fixed in `5357b4c`.

- **What this is:** A manual playlist now loads 120 rows at a time, and `move()` only knows the rows loaded so far.
- **Problem:** In a 200-item playlist, ↓ on row 120 hits `target >= items.length` and returns without doing anything or saying why.
- **Fix:** Pass the total into `playlistRow` and give that button `disabled: i === items.length - 1`, as the home-row editor already does (L1676). Or load the next page before moving.
- **If we skip it:** It is a small dead button that is rarely hit.

## Checked and fine (no finding)

- **Deploy failure paths:** test and build failures never touch the container. A failed writable check exits before `docker rm`, so the old server keeps running. A failed `docker run` or a health timeout rolls back to `:previous`, because errexit is off inside an `if` condition, so a failing `start` returns non-zero. With an existing root-owned /config, the first deploy stops at the writable check and prints the `chown` line, with the old server still up.
- **Rollback and later deploys:** a rollback to the root image that writes root-owned files is caught by the same check on the next deploy.
- **Missing /dev/dri:** a model without it fails `docker run` with the default args, both for the new version and for the rollback, so the server goes down. That was already true before this branch, the workflow comment documents it, and compose has the same line.
- **Health check:** `hostname -I` failures are caught, 127.0.0.1 is tried first, and the 3 s timeouts fit in 20 s for the bridge setup. The first check runs one interval (15 s) after the start.
- **Tag tidy-up:** it filters `latest`, `previous` and `<none>`, `|| true` covers an empty grep, and the `${NAME}-test` repository is not matched.
- **Shell syntax:** `bash -n` passes on all 4 extracted steps.
- **Web:** `node --check` passes on all web files. `web_csp()` allows inline styles, and the branch adds no inline handlers, no `eval` and no `javascript:` links (`h()` uses `addEventListener`). Every new `t('…')` string and every new server error string shown in the UI has a German entry. The two missing ones ("No ticket is made for this address", "No theme music") are never shown in the web app.
- **Navigation and teardown:** the AbortController only cancels GETs and only on `render()`. Player GETs are safe, because `hashchange` does not render while `P.it` is set. `LIBCLEAN`, the downloads `tick` and the import `waitDone` stop once their page is gone.
- **Sign-out and token rotation:** both clear `TK`.
- **Plugins:** all three test suites pass (6, 7 and 6 tests). The Trakt refresh runs under the lock file, and only `invalid_grant` drops a login. The webhook threads end at a 12 s deadline, under the plugin's 15 s limit. Test files reach only fresh installs (`copy_bundled_plugins` skips `test_*` on updates) and are never run there.

Verdict: fix O-R1, O-R2 and O-R3 first.
Not checked: Docker isn't installed, so I couldn't run the image, the health script or `--group-add` against a real DSM `/dev/dri`, and the group number 937 comes from the compose comment, not from a NAS. I also didn't check whether extra mounts in LICHTSPIEL_DOCKER_ARGS (a backup or downloads folder outside /config) are still writable by uid 1026. The writable check only looks at /config.
