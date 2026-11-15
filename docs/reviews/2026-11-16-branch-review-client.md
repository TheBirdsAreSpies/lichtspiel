# Branch review: client, 2026-11-16

Status: all findings handled, see [the review overview](../review-2026-11-15.md).

Scope: `git diff 5b30e62 -- clients/lichtspiel`.

Load assumed: one household, a few devices per person (Mac, iPhone, iPad, Android, Windows), used at home on the LAN and away over the internet. The server sits on a home line, so its upload speed is the bottleneck when you are away. On German DSL that upload is often 1 to 10 Mbit/s.

What this change does: it adds the macOS app and finds servers on the home network (Plex-style GDM search, a UDP broadcast on port 32414). The sign-in token moves into the system keychain, with the plain settings as a fallback. Requests now have timeouts, German error texts and an https→http fallback at sign-in. The token has left every address: pictures, the player and downloads send it as an `X-Token` header, and links for the browser carry a short-lived ticket. It also has several rounds of fixes: 401 takes you back to sign-in, downloads are written as `.part` with a length check, back-10s stops at 0, quality names match the server, there are phone layouts, and failed buttons now say why.

Checked and fine (no finding):
- The token never goes into addresses or log lines.
- `headersFor` compares scheme, host and port before it adds the header.
- External posters in `person.dart` get no header.
- HLS segments are covered: the server lets them in by session id, and the playlist address carries a ticket.
- The ticket scopes for downloads and logs match `auth::ticket_scope`.
- A wrong password, PIN or 2FA code never reaches `onUnauthorized`. The sign-in screen's `Api` has no token, and the account screens get 403 or 400, not 401.
- Keychain fallback on each platform. If the keychain fails, the token goes to the plain settings. If a read fails, you sign in again. The macOS legacy keychain choice is explained in the code.
- Entitlements: network client and server, user-selected read, Downloads read-write.
- iOS `Info.plist`: local network, photo library, file sharing.
- Android: `INTERNET` permission.
- `flutter test` passes (29 tests). `flutter analyze` finds 90 info-level items and no errors. The `use_build_context_synchronously` ones in the changed code are guarded by the State's own `mounted`.
- On iOS the search finds nothing without Apple's multicast entitlement. The README says so, so it is a decision, not a finding.

## Must fix

### C-R1. An attacker can silently switch an https sign-in to plain http (`lib/api.dart:139-155` `reach`, `lib/api.dart:104-114` `describe`, `lib/screens/login.dart:106-114`)

Status: Fixed in `ba75f0d`.

- **What this is:** At sign-in, if an `https://` address fails with a timeout or a TLS handshake error, the app quietly tries `http://` on the same host and port. If that answers, it signs in there.
- **Problem:** Anyone on the path (hotel Wi-Fi, a tampered router) only has to drop or reset the TLS handshake. The app then sends the user name and password in clear text to whatever answers on http. It also saves `http://` as the server address, so every later token goes in clear text too. The warning snackbar only appears after the password has already been sent. Separately, a server that is simply switched off also gets "Server spricht kein HTTPS – http:// verwenden" (`describe` treats any https timeout that way), which pushes the user toward http.
- **Fix:** Delete the automatic fallback in `reach` (about 15 lines). Keep the error text, but give it only for a real `HandshakeException`/`TlsException`, not for a timeout. Then the user decides to type `http://`. If you want to keep the convenience, allow it only for home-network hosts (private IPv4/IPv6, names without a dot, `*.local`), the same rule as the iOS ATS comment.
- **If we skip it:** A remote user who chose https can lose their password and token to anyone on the same network, and nothing warns them before it happens.

### C-R2. The 10-second limit for GETs also covers downloading the answer, so big lists and online lookups fail away from home (`lib/api.dart:168-187` `_send`)

Status: Fixed in `ba75f0d`.

- **What this is:** Every GET gets a timer of `Api.timeout` (10 s). When it runs out, the request is aborted. In `package:http` 1.6 the abort also cuts off a body that is still arriving (`io_client.dart:135`, "If the user aborts whilst streaming the response").
- **Problem:** The 10 s counts the whole transfer, not only the wait for an answer. The server does not compress JSON. Example: a long show with 700 episodes, at a few KB per episode (an estimate, not measured), is several MB of JSON. Over a 1–2 Mbit/s DSL upload, `allLeaves` takes well over 10 s, so "play all" and "Herunterladen…" for a show fail with "Server antwortet nicht". The player's next episode (`player.dart:515`) silently disappears. A large smart playlist (`playlistItems`) fails the same way. Two GETs also wait for the server's own online lookups before any byte is sent: `/library/metadata/{id}/matches` (TMDB/OMDb search) and `/library/people/{key}/details` (TMDB, 20 s per call on the server). Both can pass 10 s even at home.
- **Fix:** Use the timer only until the response headers arrive: `final s = await client.send(req); timer.cancel(); r = await http.Response.fromStream(s);`. Give `matches` and `personDetails` the `slowTimeout`, for example with an optional `slow` flag on `get`, or by sending them through `request` with a longer timer.
- **If we skip it:** Away from home, the biggest shows and playlists cannot be opened or downloaded from the app, and the "Match" and actor pages fail intermittently.

## Should fix

### C-R3. Closing the download dialog another way closes the screen behind it and keeps the download running (`lib/screens/downloads.dart:57-130` `_download`)

Status: Fixed in `ba75f0d`.

- **What this is:** On iPhone, iPad and Mac the app downloads the file itself under a modal "Wird geladen…" dialog. Only its "Abbrechen" button sets `cancelled`. At the end, `finally` pops the top route of the root navigator.
- **Problem:** On a Mac, the mouse's back button calls `navKey.currentState?.maybePop()` (`main.dart` `Listener`), which removes the dialog without setting `cancelled`. The download keeps going with no progress and no way to stop it. When it ends, `Navigator.of(context, rootNavigator: true).pop()` pops the screen underneath (Downloads or Wartung).
- **Fix:** Make any closing of the dialog count as a cancel, and pop only the dialog itself. Keep the dialog's own context (`d`) and call `if (d.mounted) Navigator.pop(d)` instead of the root pop. Add `dialog.whenComplete(() { if (!finished) { cancelled = true; client.close(); } })`. That replaces the special code in the button.
- **If we skip it:** Mac users with a 5-button mouse get downloads they cannot cancel, and the screen jumps back by itself.

## Nice to have

### C-R4. Signing out with a token that is already gone throws a null error (`lib/main.dart:53-58` `signOut`)

Status: Fixed in `ba75f0d`.

- **What this is:** "Abmelden" posts `/api/logout`, then calls `api!.forget()`.
- **Problem:** If the device was already removed elsewhere, the server answers 401. `onUnauthorized` then runs inside `_decode` and sets `api = null` before the error comes back. The next line, `api!.forget()`, throws "Null check operator used on a null value" as an uncaught error. The user still ends up at sign-in, so only the log shows it.
- **Fix:** At the start, take `final a = api; if (a == null) return;` and use `a` for both calls.
- **If we skip it:** A stray uncaught exception in logs and crash reports.

### C-R5. Every request opens a new connection (`lib/api.dart:160-165` `_client`, used per call in `_send`)

Status: Fixed in `ba75f0d`.

- **What this is:** `_send` creates an `HttpClient` for each request and closes it right after.
- **Problem:** There is no keep-alive, so every API call does a new TCP connect, plus a full TLS handshake on https. Away from home at 50 ms round trip, that adds about 100–200 ms per call. The NAS also does one TLS handshake per call.
- **Fix:** Keep one static `IOClient(HttpClient()..connectionTimeout = timeout)` and stop closing it per request. `AbortableRequest` still aborts a single request. Keep the `runWithClient` test path as it is.
- **If we skip it:** The app feels slower than the web app over the internet.

### C-R6. iPad users are told the file is "Auf meinem iPhone" (`lib/screens/downloads.dart:117`)

Status: Fixed in `ba75f0d`.

- **What this is:** The snackbar after a download says where the Files app shows the file.
- **Problem:** On an iPad the Files app calls the place "Auf meinem iPad", so the text points to a place that does not exist there.
- **Fix:** Use "Auf diesem Gerät", or pick iPhone or iPad from the screen size (`MediaQuery.sizeOf(context).shortestSide >= 600`).
- **If we skip it:** iPad users look for a folder that isn't there.

### C-R7. An older subtitle size shows as "Normal" in Settings while the player keeps the old size (`lib/screens/settings.dart:346-350`, `lib/screens/player.dart:435` `loadPrefs`)

Status: Fixed in `ba75f0d`.

- **What this is:** The Settings sizes changed from 75/100/125/150/200 to the player's 75/100/130/170.
- **Problem:** A device that saved 125, 150 or 200 shows "Normal" in Settings. The player still reads the raw value and draws 125/150/200 %. So what you see in Settings does not match what plays.
- **Fix:** Snap the stored value to the nearest key in `playerSubSizes` in `loadPrefs`, and in the Settings migration next to the `qualityKey` fix (one line each).
- **If we skip it:** Users with older settings get a size that Settings doesn't show, until they pick one again.

Verdict: fix C-R1 and C-R2 first.
Lean: -15 lines possible (deleting the automatic https→http fallback in C-R1).
Not checked: I did not build or run the app on a real device, and I did not measure real JSON sizes (the episode size in C-R2 is an estimate). The Android `allowBackup` restore of `flutter_secure_storage` data on a new phone was not tested. My reading is that the read fails and the user simply signs in again.
