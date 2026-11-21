"""SIMKL / MyAnimeList plugin.

When someone finishes a movie or an episode (playback.watched, also when marked by hand), it goes into
their SIMKL history and, for anime, their MyAnimeList list (episodes watched, completed at the end).

Each person connects their own accounts in a panel under Settings:
- SIMKL with a code typed in at simkl.com/pin (like Trakt).
- MyAnimeList through its sign-in page; it sends the person on to the app's redirect address with a
  code, and they paste that address into the panel (MyAnimeList has no sign-in with a code).

MyAnimeList counts each season as its own anime: season 2 is found by following the first season's
"sequel" there. Titles need a MyAnimeList id (the Anime details plugin adds it).

The sign-ins are kept in state.json in the plugin's folder.
"""
import contextlib
import datetime
import json
import os
import secrets
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
STATE = os.path.join(HERE, "state.json")
LOCK = os.path.join(HERE, ".state.lock")
TIMEOUT = 15
SIMKL = "https://api.simkl.com"
MAL_AUTH = "https://myanimelist.net/v1/oauth2"
MAL_API = "https://api.myanimelist.net/v2"
SEQUEL_DAYS = 30  # how long a found sequel is remembered

WORDS = {
    "SIMKL / MyAnimeList": "SIMKL / MyAnimeList",
    "What you finish here is marked as watched on SIMKL and MyAnimeList (anime only).": "Was du hier fertig schaust, wird auf SIMKL und MyAnimeList (nur Anime) als gesehen markiert.",
    "What you finish here is marked as watched on SIMKL.": "Was du hier fertig schaust, wird auf SIMKL als gesehen markiert.",
    "What you finish here is marked as watched on MyAnimeList (anime only).": "Was du hier fertig schaust, wird auf MyAnimeList (nur Anime) als gesehen markiert.",
    "Not set up yet: an admin enters the Client ID of a SIMKL and/or MyAnimeList app in the plugin's settings.": "Noch nicht eingerichtet: Ein Admin trägt die Client-ID einer SIMKL- und/oder MyAnimeList-App in den Einstellungen des Plugins ein.",
    "connected as {name}": "verbunden als {name}",
    "connected": "verbunden",
    "not connected": "nicht verbunden",
    "last: {what}": "zuletzt: {what}",
    "Connect": "Verbinden",
    "Disconnect": "Trennen",
    "Open simkl.com/pin and enter the code.": "Öffne simkl.com/pin und gib den Code ein.",
    "SIMKL is connected.": "SIMKL ist verbunden.",
    "The code has expired. Try again.": "Der Code ist abgelaufen. Versuch es noch einmal.",
    "Connect MyAnimeList": "MyAnimeList verbinden",
    "1. Open the link and allow access. 2. MyAnimeList then opens another page (it may not load); copy its address from the address bar and paste it here.": "1. Öffne den Link und erlaube den Zugriff. 2. MyAnimeList öffnet danach eine andere Seite (sie lädt vielleicht nicht); kopier ihre Adresse aus der Adressleiste und füg sie hier ein.",
    "The address (…?code=…)": "Die Adresse (…?code=…)",
    "Back": "Zurück",
    "That address has no code. Copy the whole address after allowing access.": "In dieser Adresse ist kein Code. Kopier die ganze Adresse, nachdem du den Zugriff erlaubt hast.",
    "That code belongs to an older attempt. Start again.": "Dieser Code gehört zu einem älteren Versuch. Fang noch einmal an.",
    "MyAnimeList is connected.": "MyAnimeList ist verbunden.",
    "Disconnected.": "Getrennt.",
    "{site} could not be reached: {e}": "{site} ist nicht erreichbar: {e}",
}


class ApiError(Exception):
    def __init__(self, text, code=0):
        super().__init__(text)
        self.code = code


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def log(text):
    print(text, file=sys.stderr)


# ---------- what it keeps (several runs may happen at once, so with a lock)

@contextlib.contextmanager
def locked():
    end = time.time() + 20
    while True:
        try:
            os.mkdir(LOCK)
            break
        except FileExistsError:
            try:  # left behind by a run that was stopped
                if time.time() - os.path.getmtime(LOCK) > 90:
                    os.rmdir(LOCK)
                    continue
            except OSError:
                pass
            if time.time() > end:
                raise ApiError("busy, try again in a moment")
            time.sleep(0.05)
    try:
        yield
    finally:
        try:
            os.rmdir(LOCK)
        except OSError:
            pass


def load():
    try:
        with open(STATE, encoding="utf-8") as f:
            data = json.load(f)
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        return {}


def save(state):
    tmp = STATE + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(state, f, ensure_ascii=False, indent=1)
    os.replace(tmp, STATE)


def person(state, user):
    return state.setdefault("people", {}).setdefault(user, {})


def update_person(user, **kw):
    with locked():
        state = load()
        p = person(state, user)
        for k, v in kw.items():
            if v is None:
                p.pop(k, None)
            else:
                p[k] = v
        save(state)


# ---------- HTTP

def http(method, url, headers=None, body=None, form=None):
    headers = dict(headers or {}, Accept="application/json")
    data = None
    if form is not None:
        data = urllib.parse.urlencode(form).encode()
        headers["Content-Type"] = "application/x-www-form-urlencoded"
    elif body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data, headers=headers, method=method), timeout=TIMEOUT) as r:
            raw = r.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        try:
            msg = json.loads(e.read() or b"null")
            msg = (msg.get("message") or msg.get("error_description") or msg.get("error") or msg) if isinstance(msg, dict) else msg
        except ValueError:
            msg = e.reason
        raise ApiError("answered %d: %s" % (e.code, msg), e.code)
    except (OSError, ValueError) as e:
        raise ApiError(str(getattr(e, "reason", e)))


# ---------- SIMKL

def simkl_ready(s):
    return bool(str(s.get("simkl_client_id") or "").strip())


def simkl_headers(s, token=""):
    h = {"simkl-api-key": str(s["simkl_client_id"]).strip()}
    if token:
        h["Authorization"] = "Bearer " + token
    return h


def simkl_link(lang, s, user):
    cid = str(s["simkl_client_id"]).strip()
    r = http("GET", "%s/oauth/pin?%s" % (SIMKL, urllib.parse.urlencode({"client_id": cid, "redirect": "urn:ietf:wg:oauth:2.0:oob"})))
    code = r.get("user_code")
    if not code:
        raise ApiError("no code")
    update_person(user, simkl_pin={"code": code, "until": int(time.time()) + int(r.get("expires_in") or 900)})
    return simkl_waiting(lang, code, r.get("verification_url") or "https://simkl.com/pin/", int(r.get("interval") or 5))


def simkl_waiting(lang, code, url, seconds=5):
    return {"message": tr(lang, "Open simkl.com/pin and enter the code."), "code": code, "link": url,
            "poll": {"action": "simkl_poll", "seconds": max(3, seconds)}}


def simkl_poll(lang, s, user):
    pin = person(load(), user).get("simkl_pin") or {}
    if not pin or time.time() > pin.get("until", 0):
        update_person(user, simkl_pin=None)
        return {"error": tr(lang, "The code has expired. Try again.")}
    r = http("GET", "%s/oauth/pin/%s?client_id=%s" % (SIMKL, urllib.parse.quote(pin["code"]), urllib.parse.quote(str(s["simkl_client_id"]).strip())))
    token = r.get("access_token")
    if r.get("result") != "OK" or not token:
        return simkl_waiting(lang, pin["code"], "https://simkl.com/pin/")
    name = ""
    try:
        name = ((http("POST", SIMKL + "/users/settings", simkl_headers(s, token), body={}) or {}).get("user") or {}).get("name") or ""
    except ApiError as e:
        log("SIMKL: %s" % e)
    update_person(user, simkl_pin=None, simkl={"token": token, "name": name})
    return {"message": tr(lang, "SIMKL is connected.")}


def simkl_entry(item):
    """The body for SIMKL's /sync/history for one movie or episode (None if it cannot be matched)."""
    when = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    if item.get("type") == "movie":
        ids = {k: v for k, v in (item.get("ids") or {}).items() if k in ("imdb", "tmdb", "tvdb", "mal", "anidb", "simkl") and v}
        if not ids and not item.get("title"):
            return None
        return {"movies": [{"title": item.get("title"), "year": item.get("year"), "ids": ids, "watched_at": when}]}
    if item.get("type") == "episode":
        show = item.get("show") or {}
        ids = {k: v for k, v in (show.get("ids") or {}).items() if k in ("imdb", "tmdb", "tvdb", "mal", "anidb", "simkl") and v}
        if (not ids and not show.get("title")) or item.get("episode") is None:
            return None
        return {"shows": [{"title": show.get("title"), "year": show.get("year"), "ids": ids,
                           "seasons": [{"number": item.get("season") or 1, "episodes": [{"number": item["episode"], "watched_at": when}]}]}]}
    return None


def simkl_watched(s, token, item):
    body = simkl_entry(item)
    if not body:
        return "skipped"
    r = http("POST", SIMKL + "/sync/history", simkl_headers(s, token), body=body)
    missing = r.get("not_found") or {}
    return "not found" if any(missing.get(k) for k in ("movies", "shows", "episodes")) else "added"


# ---------- MyAnimeList

def mal_ready(s):
    return bool(str(s.get("mal_client_id") or "").strip())


def mal_link(lang, s, user):
    # PKCE "plain": MyAnimeList takes the verifier itself as the challenge.
    verifier = secrets.token_urlsafe(64)[:100]
    st = secrets.token_hex(8)
    update_person(user, mal_pending={"verifier": verifier, "state": st, "at": int(time.time())})
    q = {"response_type": "code", "client_id": str(s["mal_client_id"]).strip(), "code_challenge": verifier,
         "code_challenge_method": "plain", "state": st}
    if str(s.get("mal_redirect_url") or "").strip():
        q["redirect_uri"] = str(s["mal_redirect_url"]).strip()
    url = MAL_AUTH + "/authorize?" + urllib.parse.urlencode(q)
    return {"panel": {"title": tr(lang, "Connect MyAnimeList"),
                      "text": tr(lang, "1. Open the link and allow access. 2. MyAnimeList then opens another page (it may not load); copy its address from the address bar and paste it here."),
                      "link": url,
                      "input": {"placeholder": tr(lang, "The address (…?code=…)"), "action": "mal_code", "label": tr(lang, "Connect")},
                      "buttons": [{"label": tr(lang, "Back"), "action": "panel"}]}}


def mal_token(s, form):
    form = dict(form, client_id=str(s["mal_client_id"]).strip())
    if str(s.get("mal_client_secret") or "").strip():
        form["client_secret"] = str(s["mal_client_secret"]).strip()
    r = http("POST", MAL_AUTH + "/token", form=form)
    if not r.get("access_token"):
        raise ApiError("no token")
    return {"access": r["access_token"], "refresh": r.get("refresh_token") or "", "until": int(time.time()) + int(r.get("expires_in") or 2592000) - 300}


def mal_code(lang, s, user, text):
    text = str(text or "").strip()
    # The whole address, its "code=…&state=…" part, or the code alone.
    looks_like_address = "code=" in text or "://" in text or "/" in text
    q = urllib.parse.parse_qs(text.split("?", 1)[-1]) if looks_like_address else {"code": [text]}
    code = (q.get("code") or [""])[0]
    if not code:
        return {"error": tr(lang, "That address has no code. Copy the whole address after allowing access.")}
    pending = person(load(), user).get("mal_pending") or {}
    if not pending or (q.get("state") and q["state"][0] != pending.get("state")):
        return {"error": tr(lang, "That code belongs to an older attempt. Start again.")}
    form = {"grant_type": "authorization_code", "code": code, "code_verifier": pending["verifier"]}
    if str(s.get("mal_redirect_url") or "").strip():
        form["redirect_uri"] = str(s["mal_redirect_url"]).strip()
    tok = mal_token(s, form)
    try:
        tok["name"] = http("GET", MAL_API + "/users/@me", {"Authorization": "Bearer " + tok["access"]}).get("name") or ""
    except ApiError as e:
        log("MyAnimeList: %s" % e)
    update_person(user, mal_pending=None, mal=tok)
    return {"message": tr(lang, "MyAnimeList is connected.")}


def mal_access(s, user):
    """A working access token for this person (refreshed when it has run out), or None."""
    tok = person(load(), user).get("mal")
    if not tok:
        return None
    if time.time() < tok.get("until", 0):
        return tok["access"]
    new = mal_token(s, {"grant_type": "refresh_token", "refresh_token": tok.get("refresh") or ""})
    new["name"] = tok.get("name") or ""
    update_person(user, mal=new)
    return new["access"]


def mal_get(token, ident, fields):
    return http("GET", "%s/anime/%s?fields=%s" % (MAL_API, ident, fields), {"Authorization": "Bearer " + token})


def mal_sequel(token, ident):
    """The next TV season of this anime on MyAnimeList and its number of episodes (remembered for a while)."""
    key = str(ident)
    cache = load().get("sequels") or {}
    hit = cache.get(key)
    if hit and time.time() - hit.get("at", 0) < SEQUEL_DAYS * 86400:
        return hit.get("next"), hit.get("episodes") or 0
    info = mal_get(token, ident, "related_anime,num_episodes,media_type")
    nxt = None
    for rel in info.get("related_anime") or []:
        node = rel.get("node") or {}
        if rel.get("relation_type") == "sequel" and node.get("id"):
            nxt = node["id"]
            break
    with locked():
        state = load()
        state.setdefault("sequels", {})[key] = {"next": nxt, "episodes": info.get("num_episodes") or 0, "at": int(time.time())}
        save(state)
    return nxt, info.get("num_episodes") or 0


def mal_target(token, item):
    """(MyAnimeList id, episode number there) for a movie or episode, or (None, 0)."""
    if item.get("type") == "movie":
        mid = (item.get("ids") or {}).get("mal")
        return (mid, 1) if mid else (None, 0)
    if item.get("type") != "episode":
        return None, 0
    mid = ((item.get("show") or {}).get("ids") or {}).get("mal")
    season, ep = item.get("season") or 1, item.get("episode")
    if not mid or not ep or season < 1:
        return None, 0
    # Season n is the (n-1)th sequel; an episode counted on from the start of the show (101 …)
    # goes on through the sequels too.
    for _ in range(season - 1):
        mid, _count = mal_sequel(token, mid)
        if not mid:
            return None, 0
    for _ in range(20):
        nxt, count = mal_sequel(token, mid)
        if not count or ep <= count or not nxt:
            break
        ep, mid = ep - count, nxt
    return mid, ep


def mal_watched(s, user, item):
    token = mal_access(s, user)
    if not token:
        return "skipped"
    mid, ep = mal_target(token, item)
    if not mid:
        return "skipped"
    info = mal_get(token, mid, "my_list_status,num_episodes")
    have = ((info.get("my_list_status") or {}).get("num_episodes_watched")) or 0
    total = info.get("num_episodes") or 0
    watched = max(have, ep)
    status = "completed" if total and watched >= total else "watching"
    if (info.get("my_list_status") or {}).get("status") == "completed" and (status != "completed" or watched == have):
        return "already completed"  # a rewatch: the list stays as it is
    http("PATCH", "%s/anime/%s/my_list_status" % (MAL_API, mid), {"Authorization": "Bearer " + token},
         form={"status": status, "num_watched_episodes": watched})
    return status


# ---------- a title was finished

def what(item):
    if item.get("type") == "episode":
        return "%s S%02dE%02d" % ((item.get("show") or {}).get("title") or "?", item.get("season") or 0, item.get("episode") or 0)
    return (item.get("title") or "?") + (" (%s)" % item["year"] if item.get("year") else "")


def watched(s, data):
    user, item = data.get("user") or "", data.get("item") or {}
    p = person(load(), user)
    out = {}
    if simkl_ready(s) and (p.get("simkl") or {}).get("token"):
        try:
            out["simkl"] = simkl_watched(s, p["simkl"]["token"], item)
        except ApiError as e:
            out["simkl"] = "error"
            log("SIMKL (%s): %s" % (user, e))
            if e.code == 401:  # signed out on SIMKL: the panel says so
                update_person(user, simkl=None)
    if mal_ready(s) and p.get("mal"):
        try:
            out["mal"] = mal_watched(s, user, item)
        except ApiError as e:
            out["mal"] = "error"
            log("MyAnimeList (%s): %s" % (user, e))
    if out:
        update_person(user, last={"what": what(item), "result": out, "at": int(time.time())})
    return dict(out, ok=True)


# ---------- the panel

def panel(lang, s, user):
    out = {"title": tr(lang, "SIMKL / MyAnimeList")}
    sk, ma = simkl_ready(s), mal_ready(s)
    if not sk and not ma:
        out["text"] = tr(lang, "Not set up yet: an admin enters the Client ID of a SIMKL and/or MyAnimeList app in the plugin's settings.")
        return out
    out["text"] = tr(lang, "What you finish here is marked as watched on SIMKL and MyAnimeList (anime only)." if sk and ma else
                     "What you finish here is marked as watched on SIMKL." if sk else "What you finish here is marked as watched on MyAnimeList (anime only).")
    p = person(load(), user)
    items = []
    for key, site, ready, connect in (("simkl", "SIMKL", sk, "simkl_link"), ("mal", "MyAnimeList", ma, "mal_link")):
        if not ready:
            continue
        acc = p.get(key)
        sub = []
        last = p.get("last") or {}
        if acc and key in (last.get("result") or {}):
            sub.append(tr(lang, "last: {what}", what="%s (%s)" % (last.get("what"), last["result"][key])))
        items.append({"title": site, "subtitle": " · ".join(sub),
                      "status": (tr(lang, "connected as {name}", name=acc["name"]) if acc.get("name") else tr(lang, "connected")) if acc else tr(lang, "not connected"),
                      "status_kind": "default" if acc else "",
                      "buttons": [{"label": tr(lang, "Disconnect"), "action": "disconnect", "args": {"site": key}}] if acc else
                                 [{"label": tr(lang, "Connect"), "action": connect, "primary": True}]})
    out["items"] = items
    return out


def call(action, lang, s, user, body):
    args = body.get("args") if isinstance(body.get("args"), dict) else {}
    site = "SIMKL" if action.startswith("simkl") else "MyAnimeList"
    try:
        if action == "panel":
            return panel(lang, s, user)
        if action == "simkl_link" and simkl_ready(s):
            return simkl_link(lang, s, user)
        if action == "simkl_poll" and simkl_ready(s):
            return simkl_poll(lang, s, user)
        if action == "mal_link" and mal_ready(s):
            return mal_link(lang, s, user)
        if action == "mal_code" and mal_ready(s):
            out = mal_code(lang, s, user, body.get("query"))
            if "message" in out:
                out["panel"] = panel(lang, s, user)
            return out
        if action == "disconnect" and args.get("site") in ("simkl", "mal"):
            update_person(user, **{args["site"]: None})
            return {"message": tr(lang, "Disconnected."), "panel": panel(lang, s, user)}
    except ApiError as e:
        return {"error": tr(lang, "{site} could not be reached: {e}", site=site, e=e)}
    return {"error": "Unknown action"}


def main():
    # The server sends UTF-8, whatever the console code page is (Windows).
    msg = json.loads(getattr(sys.stdin, "buffer", sys.stdin).read())
    event = msg.get("event", "")
    data = msg.get("data") or {}
    s = msg.get("settings") or {}
    try:
        if event == "playback.watched":
            out = watched(s, data)
        elif event == "api.call":
            body = data.get("body") if isinstance(data.get("body"), dict) else {}
            if not data.get("user"):
                out = {"error": "Unknown user"}
            else:
                out = call(str(data.get("action") or ""), body.get("lang") or "en", s, data["user"], body)
        else:
            out = {"skipped": event}
    except ApiError as e:
        out = {"error": str(e)}
    # Escaped (ü), so the answer survives any console code page (Windows).
    print(json.dumps(out))


if __name__ == "__main__":
    main()
