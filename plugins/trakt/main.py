"""Trakt plugin for Lichtspiel.

Reads {"event", "data", "settings", "server"} from standard input.

Playback events -> Trakt's scrobble API: "watching now" while playing, paused
when paused, and into the history once watched (Trakt counts a stop at 80 % or
more as watched). Titles marked as watched by hand go to /sync/history.

Each Lichtspiel user connects their own Trakt account with Trakt's device
login (a code to type in at trakt.tv/activate). The tokens are kept in
tokens.json next to this file. Own API actions (POST /api/plugins/trakt/call/…):
  panel       what the Settings panel shows for the caller
  connect     starts the device login, answers the code to type in
  poll        checks whether the code was entered (the page asks every few seconds)
  disconnect  forgets (and revokes) the caller's Trakt login
  import      reads the caller's watched movies and episodes from Trakt and
              answers {"mark_watched": [...]}, which the server applies
  status      the caller's connection as plain JSON
"""
import json
import os
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone

API = "https://api.trakt.tv"
HERE = os.path.dirname(os.path.abspath(__file__))
STATE = os.path.join(HERE, "tokens.json")
LOCK = STATE + ".lock"
APP_URL = "https://trakt.tv/oauth/applications/new"
REDIRECT = "urn:ietf:wg:oauth:2.0:oob"
# A finished scrobble is not sent again by a later stop/watched of the same item.
SCROBBLED_FOR = 6 * 3600

DE = {
    "Trakt is not set up yet": "Trakt ist noch nicht eingerichtet",
    "An admin needs to create a Trakt app first and put its Client ID and Client Secret into this plugin's settings (Plugins page).":
        "Ein Admin muss zuerst eine Trakt-App anlegen und deren Client ID und Client Secret in die Einstellungen dieses Plugins eintragen (Seite Plugins).",
    "Create a Trakt app at {url} (redirect URI {redirect}), then enter its Client ID and Client Secret in the Trakt plugin's settings on the Plugins page.":
        "Lege unter {url} eine Trakt-App an (Redirect URI {redirect}) und trage ihre Client ID und ihr Client Secret in die Einstellungen des Trakt-Plugins auf der Seite Plugins ein.",
    "Not connected. Connect your Trakt account and what you watch here shows up on Trakt.":
        "Nicht verbunden. Verbinde dein Trakt-Konto, dann erscheint auf Trakt, was du hier schaust.",
    "Connected as {name}.": "Verbunden als {name}.",
    "Playing shows as “watching now” on Trakt; watched movies and episodes go into your Trakt history.":
        "Beim Abspielen zeigt Trakt „schaut gerade“; Gesehenes landet in deinem Trakt-Verlauf.",
    "Sending to Trakt is switched off by the admin.": "Das Senden an Trakt ist vom Admin ausgeschaltet.",
    "Connect Trakt": "Trakt verbinden",
    "Disconnect": "Trennen",
    "Bring in my Trakt history": "Meinen Trakt-Verlauf übernehmen",
    "Disconnect your Trakt account?": "Dein Trakt-Konto trennen?",
    "Open {url} and enter this code:": "Öffne {url} und gib diesen Code ein:",
    "The code has expired. Try again.": "Der Code ist abgelaufen. Versuch es noch einmal.",
    "Trakt was told no.": "Die Anmeldung wurde auf Trakt abgelehnt.",
    "Disconnected from Trakt.": "Von Trakt getrennt.",
    "Not connected to Trakt.": "Nicht mit Trakt verbunden.",
    "Read {movies} movies and {episodes} episodes from Trakt.": "{movies} Filme und {episodes} Folgen von Trakt gelesen.",
    "Waiting for the code…": "Warte auf den Code …",
    "Last sent: {what} ({when}).": "Zuletzt gesendet: {what} ({when}).",
    "No Trakt connection was started. Press Connect again.": "Es wurde keine Trakt-Anmeldung gestartet. Drück noch einmal auf Verbinden.",
    "Trakt answered {code}: {text}": "Trakt hat {code} geantwortet: {text}",
}
LANG = "en"


def t(msg, **kw):
    s = DE.get(msg, msg) if LANG.startswith("de") else msg
    return s.format(**kw) if kw else s


# ---------------------------------------------------------------- state file

def lock():
    """A small lock file, because several events may run at the same time."""
    for _ in range(800):  # up to 40 s
        try:
            fd = os.open(LOCK, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            os.close(fd)
            return True
        except FileExistsError:
            try:  # left behind by a plugin run that was stopped
                if time.time() - os.path.getmtime(LOCK) > 60:
                    os.remove(LOCK)
                    continue
            except OSError:
                pass
            time.sleep(0.05)
    return False


def unlock():
    try:
        os.remove(LOCK)
    except OSError:
        pass


def load():
    try:
        with open(STATE, encoding="utf-8") as f:
            st = json.load(f)
    except (OSError, ValueError):
        st = {}
    for k in ("users", "pending", "scrobbled", "last"):
        st.setdefault(k, {})
    return st


def save(st):
    tmp = STATE + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(st, f, indent=1)
    os.replace(tmp, STATE)


# ---------------------------------------------------------------- Trakt calls

class TraktError(Exception):
    def __init__(self, code, text):
        super().__init__(t("Trakt answered {code}: {text}", code=code, text=text))
        self.code = code
        self.text = text


def call(settings, method, path, body=None, token=None):
    headers = {
        "Content-Type": "application/json",
        "trakt-api-version": "2",
        "trakt-api-key": settings.get("client_id", ""),
        "User-Agent": "Lichtspiel-Trakt/1.0",
    }
    if token:
        headers["Authorization"] = "Bearer " + token
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(API + path, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            raw = r.read()
            return json.loads(raw) if raw.strip() else {}
    except urllib.error.HTTPError as e:
        text = e.read().decode("utf-8", "replace")[:300]
        raise TraktError(e.code, text or e.reason)
    except OSError as e:  # no answer at all (URLError, timeout); code 0 never counts as a disconnect
        raise TraktError(0, str(getattr(e, "reason", e)))


def configured(settings):
    return bool(settings.get("client_id", "").strip() and settings.get("client_secret", "").strip())


def access_token(settings, st, user):
    """The user's token, refreshed when it is (nearly) out of date."""
    u = st["users"].get(user)
    if not u:
        return None
    # Trakt's tokens now live only a day or a week, and each refresh replaces the refresh token, so refresh only at the end.
    if u.get("expires_at", 0) - time.time() < 600 and u.get("refresh_token"):
        try:
            tok = call(settings, "POST", "/oauth/token", {
                "refresh_token": u["refresh_token"], "client_id": settings["client_id"],
                "client_secret": settings["client_secret"], "redirect_uri": REDIRECT, "grant_type": "refresh_token"})
            keep_token(u, tok)
            save(st)
        except TraktError as e:
            # Only a refresh token Trakt no longer knows means the login is gone; a wrong Client
            # Secret (invalid_client) or a hiccup must not throw away everyone's login.
            if e.code in (400, 401) and "invalid_grant" in e.text:
                st["users"].pop(user, None)
                save(st)
                return None
            raise
    return u["access_token"]


def keep_token(u, tok):
    u["access_token"] = tok["access_token"]
    u["refresh_token"] = tok.get("refresh_token", u.get("refresh_token", ""))
    u["expires_at"] = int(tok.get("created_at", time.time())) + int(tok.get("expires_in", 7776000))


# ---------------------------------------------------------------- items

def trakt_ids(ids):
    out = {}
    for k in ("imdb", "tmdb", "tvdb", "trakt", "slug"):
        v = (ids or {}).get(k)
        if v in (None, "", 0):
            continue
        if k in ("tmdb", "tvdb", "trakt"):
            try:
                v = int(v)
            except (TypeError, ValueError):
                continue
        out[k] = v
    return out


def scrobble_body(item):
    """What Trakt's scrobble API wants for a movie or an episode (None for anything else)."""
    kind = item.get("type")
    if kind == "movie":
        m = {"title": item.get("title"), "ids": trakt_ids(item.get("ids"))}
        if item.get("year"):
            m["year"] = item["year"]
        return {"movie": m}
    if kind == "episode" and item.get("show") and item.get("season") is not None and item.get("episode") is not None:
        show = item["show"]
        s = {"title": show.get("title"), "ids": trakt_ids(show.get("ids"))}
        if show.get("year"):
            s["year"] = show["year"]
        return {"show": s, "episode": {"season": item["season"], "number": item["episode"]}}
    return None


def history_body(item, when):
    """/sync/history for a movie, episode, season or whole show marked as watched."""
    kind = item.get("type")
    at = datetime.fromtimestamp(when, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")
    if kind == "movie":
        return {"movies": [dict(scrobble_body(item)["movie"], watched_at=at)]}
    show = item.get("show") if kind in ("episode", "season") else item if kind == "show" else None
    if not show:
        return None
    s = {"title": show.get("title"), "ids": trakt_ids(show.get("ids"))}
    if show.get("year"):
        s["year"] = show["year"]
    if kind == "show":
        s["watched_at"] = at
    elif kind == "season":
        s["seasons"] = [{"number": item.get("season"), "watched_at": at}]
    else:
        s["seasons"] = [{"number": item.get("season"), "episodes": [{"number": item.get("episode"), "watched_at": at}]}]
    return {"shows": [s]}


def label(item):
    if item.get("type") == "episode" and item.get("show"):
        return "%s S%02dE%02d" % (item["show"].get("title", "?"), item.get("season") or 0, item.get("episode") or 0)
    return item.get("title", "?")


# ---------------------------------------------------------------- playback events

def on_playback(event, data, settings):
    user = data.get("user")
    item = data.get("item") or {}
    if not settings.get("scrobble", True) or not configured(settings) or not user:
        return {"skipped": "off"}
    if not lock():
        return {"skipped": "busy"}
    try:
        st = load()
        token = access_token(settings, st, user)
        if not token:
            return {"skipped": "%s is not connected" % user}
        progress = float(data.get("progress") or 0)
        key = "%s:%s" % (user, item.get("id"))
        done_at = st["scrobbled"].get(key, 0)
        recently_done = time.time() - done_at < SCROBBLED_FOR

        if event == "playback.watched" and data.get("manual"):
            if not settings.get("send_marked_watched", True):
                return {"skipped": "marked by hand"}
            body = history_body(item, time.time())
            if not body:
                return {"skipped": item.get("type")}
            r = call(settings, "POST", "/sync/history", body, token)
            note(st, user, "history", item)
            save(st)
            return {"ok": True, "added": r.get("added")}

        body = scrobble_body(item)
        if not body:
            return {"skipped": item.get("type")}
        if event == "playback.watched":
            if recently_done:
                return {"skipped": "already scrobbled"}
            # Watched here (threshold or credits) counts as watched on Trakt too.
            action, progress = "stop", max(progress, 80.0)
        elif event == "playback.stopped":
            if recently_done:
                return {"skipped": "already scrobbled"}
            action = "stop"
        elif data.get("state") == "paused":
            action = "pause"
        else:
            action = "start"
        body["progress"] = round(min(max(progress, 0.0), 100.0), 2)
        try:
            r = call(settings, "POST", "/scrobble/" + action, body, token)
        except TraktError as e:
            if e.code == 409:  # Trakt already has this scrobble
                st["scrobbled"][key] = int(time.time())
                save(st)
                return {"ok": True, "already": True}
            raise
        if action == "stop" and body["progress"] >= 80:
            st["scrobbled"][key] = int(time.time())
        elif action == "start" and recently_done and body["progress"] < 10:
            st["scrobbled"].pop(key, None)  # watching it again from the start
        # Old entries go.
        st["scrobbled"] = {k: v for k, v in st["scrobbled"].items() if time.time() - v < SCROBBLED_FOR}
        note(st, user, action, item)
        save(st)
        return {"ok": True, "action": r.get("action", action), "progress": body["progress"]}
    finally:
        unlock()


def note(st, user, what, item):
    st["last"][user] = {"what": what, "title": label(item), "at": int(time.time())}


# ---------------------------------------------------------------- the caller's own actions

def panel(settings, data, st):
    user, admin = data.get("user"), data.get("admin")
    out = {"title": "Trakt"}
    if not configured(settings):
        out["status"] = "setup"
        out["text"] = t("Trakt is not set up yet") + ". " + (
            t("Create a Trakt app at {url} (redirect URI {redirect}), then enter its Client ID and Client Secret in the Trakt plugin's settings on the Plugins page.", url=APP_URL, redirect=REDIRECT)
            if admin else t("An admin needs to create a Trakt app first and put its Client ID and Client Secret into this plugin's settings (Plugins page)."))
        if admin:
            out["link"] = APP_URL
        out["buttons"] = []
        return out
    u = st["users"].get(user)
    if not u:
        out["status"] = "off"
        out["text"] = t("Not connected. Connect your Trakt account and what you watch here shows up on Trakt.")
        out["buttons"] = [{"label": t("Connect Trakt"), "action": "connect", "primary": True}]
        return out
    out["status"] = "on"
    lines = [t("Connected as {name}.", name=u.get("name") or "?"),
             t("Playing shows as “watching now” on Trakt; watched movies and episodes go into your Trakt history.")
             if settings.get("scrobble", True) else t("Sending to Trakt is switched off by the admin.")]
    last = st["last"].get(user)
    if last:
        lines.append(t("Last sent: {what} ({when}).", what=last["title"], when=datetime.fromtimestamp(last["at"]).strftime("%Y-%m-%d %H:%M")))
    out["text"] = " ".join(lines)
    out["link"] = "https://trakt.tv/users/%s" % u.get("slug") if u.get("slug") else None
    out["buttons"] = [
        {"label": t("Bring in my Trakt history"), "action": "import"},
        {"label": t("Disconnect"), "action": "disconnect", "confirm": t("Disconnect your Trakt account?")},
    ]
    return out


def connect(settings, user, st):
    r = call(settings, "POST", "/oauth/device/code", {"client_id": settings["client_id"]})
    st["pending"][user] = {"device_code": r["device_code"], "expires_at": int(time.time()) + int(r.get("expires_in", 600)),
                           "interval": int(r.get("interval", 5))}
    save(st)
    url = r.get("verification_url", "https://trakt.tv/activate")
    return {"message": t("Open {url} and enter this code:", url=url), "code": r["user_code"], "link": url,
            "poll": {"action": "poll", "seconds": max(int(r.get("interval", 5)), 2)}}


def poll(settings, user, st):
    p = st["pending"].get(user)
    if not p:
        return {"error": t("No Trakt connection was started. Press Connect again."), "done": True}
    if time.time() > p["expires_at"]:
        st["pending"].pop(user, None)
        save(st)
        return {"error": t("The code has expired. Try again."), "done": True}
    try:
        tok = call(settings, "POST", "/oauth/device/token", {
            "code": p["device_code"], "client_id": settings["client_id"], "client_secret": settings["client_secret"]})
    except TraktError as e:
        if e.code in (400, 429):  # not entered yet / asked too fast
            return {"message": t("Waiting for the code…"), "poll": {"action": "poll", "seconds": p["interval"] + (5 if e.code == 429 else 0)}}
        st["pending"].pop(user, None)
        save(st)
        if e.code == 418:
            return {"error": t("Trakt was told no."), "done": True}
        if e.code in (404, 409, 410):
            return {"error": t("The code has expired. Try again."), "done": True}
        raise
    u = {}
    keep_token(u, tok)
    try:
        me = call(settings, "GET", "/users/settings", token=u["access_token"])
        u["name"] = me.get("user", {}).get("name") or me.get("user", {}).get("username")
        u["slug"] = me.get("user", {}).get("ids", {}).get("slug") or me.get("user", {}).get("username")
    except TraktError:
        pass
    st["users"][user] = u
    st["pending"].pop(user, None)
    save(st)
    return {"message": t("Connected as {name}.", name=u.get("name") or "?"), "done": True}


def disconnect(settings, user, st):
    u = st["users"].pop(user, None)
    save(st)
    if u:
        try:
            call(settings, "POST", "/oauth/revoke", {"token": u["access_token"], "client_id": settings["client_id"],
                                                    "client_secret": settings["client_secret"]})
        except (TraktError, OSError):
            pass
    return {"message": t("Disconnected from Trakt."), "done": True}


def unix(s):
    try:
        return int(datetime.strptime(s[:19], "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc).timestamp())
    except (TypeError, ValueError):
        return None


def import_history(settings, user, st):
    token = access_token(settings, st, user)
    if not token:
        return {"error": t("Not connected to Trakt."), "done": True}
    marks = []
    movies = call(settings, "GET", "/sync/watched/movies", token=token) or []
    for m in movies:
        marks.append({"type": "movie", "ids": m.get("movie", {}).get("ids", {}), "plays": m.get("plays", 1),
                      "watched_at": unix(m.get("last_watched_at"))})
    episodes = 0
    shows = call(settings, "GET", "/sync/watched/shows", token=token) or []
    for s in shows:
        ids = s.get("show", {}).get("ids", {})
        for season in s.get("seasons", []):
            for ep in season.get("episodes", []):
                episodes += 1
                marks.append({"type": "episode", "show": {"ids": ids}, "season": season.get("number"), "episode": ep.get("number"),
                              "plays": ep.get("plays", 1), "watched_at": unix(ep.get("last_watched_at"))})
    return {"message": t("Read {movies} movies and {episodes} episodes from Trakt.", movies=len(movies), episodes=episodes),
            "mark_watched": marks, "done": True}


def on_call(data, settings):
    action = data.get("action")
    user = data.get("user")
    body = data.get("body") or {}
    global LANG
    LANG = str(body.get("lang") or "en") if isinstance(body, dict) else "en"
    if not lock():
        return {"error": "busy"}
    try:
        st = load()
        if action == "panel":
            return panel(settings, data, st)
        if action == "status":
            u = st["users"].get(user)
            return {"configured": configured(settings), "connected": bool(u), "name": (u or {}).get("name")}
        if not configured(settings):
            return {"error": t("Trakt is not set up yet"), "done": True}
        if action == "connect":
            return connect(settings, user, st)
        if action == "poll":
            return poll(settings, user, st)
        if action == "disconnect":
            return disconnect(settings, user, st)
        if action == "import":
            return import_history(settings, user, st)
        return {"error": "unknown action %s" % action}
    except TraktError as e:
        return {"error": str(e), "done": True}
    finally:
        unlock()


def main():
    msg = json.load(sys.stdin)
    event = msg.get("event", "")
    data = msg.get("data") or {}
    settings = msg.get("settings") or {}
    if event == "api.call":
        out = on_call(data, settings)
    elif event.startswith("playback."):
        try:
            out = on_playback(event, data, settings)
        except TraktError as e:
            sys.exit(str(e))  # one line on stderr and exit 1, instead of a traceback
    else:
        out = {"skipped": event}
    print(json.dumps(out))


if __name__ == "__main__":
    main()
