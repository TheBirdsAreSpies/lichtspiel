"""Smart collections plugin (like Kometa's collections).

Once a day and when an admin asks, fills collections in the movie libraries from lists:

- "imdb-top-250": IMDb's Top 250 (IMDb blocks programs, so its copy on Letterboxd, kept up to date
  by Letterboxd: letterboxd.com/dave/list/imdb-top-250),
- "oscar-best-picture": the winners of the Academy Award for Best Picture (Wikidata),
- "tmdb-popular" / "tmdb-top-rated": TMDB's 100 most popular / best rated movies (needs a TMDB key
  in the settings),
- any public Letterboxd list or watchlist, by its address.

Only titles in the library go in. The server puts them in each collection and takes out titles the
plugin put there before that are no longer on the list; collections given by hand stay. A list
taken out of the settings empties its collection once. Letterboxd films are looked up once (their
TMDB and IMDb ids) and kept in cache.json; the last run is in state.json, both in the plugin's folder.
Wikidata's answer is kept there too: asked again after a week, and used as it is while Wikidata
does not answer (it may limit programs to one request a minute when it is busy).
"""
import contextlib
import html
import json
import os
import re
import ssl
import sys
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
STATE = os.path.join(HERE, "state.json")
CACHE = os.path.join(HERE, "cache.json")
LOCK = os.path.join(HERE, ".collections.lock")
UA = "Lichtspiel-smart-collections/1.0"
TIMEOUT = 30
LETTERBOXD_PAGES = 20          # at most 2,000 films a list
NEW_LOOKUPS = 400              # Letterboxd film pages looked up a run (the rest the next day)
PAUSE = 0.3
WIKIDATA_DAYS = 7               # Wikidata's answer is asked again after a week

PRESETS = {
    "imdb-top-250": ("letterboxd", "https://letterboxd.com/dave/list/imdb-top-250/"),
    "oscar-best-picture": ("wikidata", "Q102427"),
    "tmdb-popular": ("tmdb", "/movie/popular"),
    "tmdb-top-rated": ("tmdb", "/movie/top_rated"),
}

WORDS = {
    "Smart collections": "Smarte Sammlungen",
    "Only admins can see the smart collections.": "Nur Admins sehen die smarten Sammlungen.",
    "No lists set up. Add some under Plugins → Smart collections → settings.": "Keine Listen eingerichtet. Füge welche unter Plugins → Smarte Sammlungen → Einstellungen hinzu.",
    "Not updated yet.": "Noch nicht aktualisiert.",
    "Updated {when}.": "Aktualisiert am {when}.",
    "Update now": "Jetzt aktualisieren",
    "{n} of {m} in your libraries": "{n} von {m} in deinen Mediatheken",
    "Updated: {n} collections.": "Aktualisiert: {n} Sammlungen.",
    "The library could not be read: {e}": "Die Mediathek konnte nicht gelesen werden: {e}",
    "This plugin needs a server that lets plugins read the library.": "Dieses Plugin braucht einen Server, der Plugins die Mediathek lesen lässt.",
    "error": "Fehler",
    "needs a TMDB key in the settings": "braucht einen TMDB-Key in den Einstellungen",
    "unknown source": "unbekannte Quelle",
}


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def norm(text):
    text = unicodedata.normalize("NFKD", str(text or "")).encode("ascii", "ignore").decode().lower()
    text = re.sub(r"[^a-z0-9]+", " ", text).strip()
    return re.sub(r"^(the|a|an|der|die|das) ", "", text)


# ---------------------------------------------------------------- the web

def get(url, accept="text/html", headers=None):
    h = {"User-Agent": UA, "Accept": accept}
    h.update(headers or {})
    ctx = None
    host = urllib.parse.urlparse(url).hostname
    if url.startswith("https://") and host in ("127.0.0.1", "localhost", "::1"):
        # The server's own certificate may be made for its public name, not for 127.0.0.1.
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    for attempt in range(3):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=h), timeout=TIMEOUT, context=ctx) as r:
                return r.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            if e.code in (429, 503) and attempt < 2:
                time.sleep(wait_for(e, attempt))
                continue
            raise


def wait_for(e, attempt):
    """Seconds to wait before asking again: what the site asks for in Retry-After (at most 2 minutes)."""
    try:
        asked = int((e.headers or {}).get("Retry-After") or 0)
    except (TypeError, ValueError):
        asked = 0
    return min(max(asked, 5 * (attempt + 1)), 120)


def fetch_library(server):
    address = (server or {}).get("library")
    if not address:
        raise LookupError("no library address")
    return json.loads(get(address, "application/json"))


# ---------------------------------------------------------------- the lists
# Each gives a list of films: {"tmdb", "imdb", "title", "year"} (any of them may be missing).

def letterboxd_films(url, cache, budget):
    """The films of a Letterboxd list (all its pages), with ids from each film's page (cached)."""
    base = url.split("?")[0].rstrip("/")
    base = re.sub(r"/page/\d+$", "", base)
    films = []
    for page in range(1, LETTERBOXD_PAGES + 1):
        text = get(base + "/" if page == 1 else "%s/page/%d/" % (base, page))
        if not text:
            break
        items = parse_letterboxd_page(text)
        if not items:
            break
        films.extend(items)
        if 'class="next"' not in text and "/page/%d/" % (page + 1) not in text:
            break
        time.sleep(PAUSE)
    for f in films:
        known = cache.get(f["slug"])
        if known is None and budget[0] > 0:
            budget[0] -= 1
            known = letterboxd_ids(f["slug"])
            cache[f["slug"]] = known
            time.sleep(PAUSE)
        if known:
            f.update({k: v for k, v in known.items() if v})
    return films


def parse_letterboxd_page(text):
    """The films on one page of a list: [{"slug", "title", "year"}] in order."""
    out, seen = [], set()
    for m in re.finditer(r'<div[^>]*data-item-slug="([^"]+)"[^>]*>', text):
        tag = m.group(0)
        slug = m.group(1)
        if slug in seen:
            continue
        seen.add(slug)
        name = re.search(r'data-item-name="([^"]*)"', tag)
        title, year = html.unescape(name.group(1)) if name else slug, None
        y = re.match(r"^(.*) \((\d{4})\)$", title)
        if y:
            title, year = y.group(1), int(y.group(2))
        out.append({"slug": slug, "title": title, "year": year})
    return out


def letterboxd_ids(slug):
    """A Letterboxd film's TMDB and IMDb ids, from its page."""
    text = get("https://letterboxd.com/film/%s/" % urllib.parse.quote(slug)) or ""
    tmdb = re.search(r'data-tmdb-id="(\d+)"', text)
    imdb = re.search(r"imdb\.com/title/(tt\d+)", text)
    return {"tmdb": int(tmdb.group(1)) if tmdb else None, "imdb": imdb.group(1) if imdb else None}


def wikidata_award(award, cache=None):
    """Films that won an award (Wikidata item), with their IMDb and TMDB ids. The answer is kept in
    `cache` for WIKIDATA_DAYS; when Wikidata does not answer, the last one is used however old."""
    cache = {} if cache is None else cache
    key = "wikidata:" + award
    kept = cache.get(key) if isinstance(cache.get(key), dict) else None
    if kept and time.time() - kept.get("at", 0) < WIKIDATA_DAYS * 86400:
        return kept["films"]
    try:
        films = ask_wikidata(award)
    except (OSError, ValueError):
        if kept:
            return kept["films"]
        raise
    cache[key] = {"at": int(time.time()), "films": films}
    return films


def ask_wikidata(award):
    q = ("SELECT ?imdb ?tmdb WHERE { ?film wdt:P31 wd:Q11424; wdt:P166 wd:%s . "
         "OPTIONAL { ?film wdt:P345 ?imdb } OPTIONAL { ?film wdt:P4947 ?tmdb } }" % award)
    text = get("https://query.wikidata.org/sparql?" + urllib.parse.urlencode({"query": q}), "application/sparql-results+json")
    out = []
    for b in json.loads(text or "{}").get("results", {}).get("bindings", []):
        tmdb = (b.get("tmdb") or {}).get("value")
        out.append({"imdb": (b.get("imdb") or {}).get("value"), "tmdb": int(tmdb) if tmdb and tmdb.isdigit() else None})
    return out


def tmdb_list(path, key):
    out = []
    for page in range(1, 6):
        if key.startswith("eyJ"):
            text = get("https://api.themoviedb.org/3%s?page=%d" % (path, page), "application/json", {"Authorization": "Bearer " + key})
        else:
            text = get("https://api.themoviedb.org/3%s?page=%d&api_key=%s" % (path, page, urllib.parse.quote(key)), "application/json")
        for m in json.loads(text or "{}").get("results") or []:
            out.append({"tmdb": m.get("id"), "title": m.get("title"), "year": int(m["release_date"][:4]) if (m.get("release_date") or "")[:4].isdigit() else None})
    return out


def films_of(source, settings, cache, budget):
    source = str(source or "").strip()
    kind, arg = PRESETS.get(source, (None, None))
    if kind is None and re.match(r"^https://(www\.)?letterboxd\.com/[^/]+/(list/[^/]+|watchlist)/?", source):
        kind, arg = "letterboxd", source
    if kind == "letterboxd":
        return letterboxd_films(arg, cache, budget)
    if kind == "wikidata":
        return wikidata_award(arg, cache)
    if kind == "tmdb":
        key = str(settings.get("tmdb_api_key") or "").strip()
        if not key:
            raise ValueError("needs a TMDB key in the settings")
        return tmdb_list(arg, key)
    raise ValueError("unknown source")


# ---------------------------------------------------------------- matching

def library_index(listing, settings):
    """The movies of the chosen libraries, findable by TMDB id, IMDb id and title with year."""
    wanted = {norm(x) for x in (settings.get("libraries") or []) if str(x).strip()}
    sections = {s.get("id"): s for s in listing.get("sections") or []}
    ok = {sid for sid, s in sections.items() if s.get("kind") == "movie" and (not wanted or norm(s.get("name")) in wanted)}
    by_tmdb, by_imdb, by_name = {}, {}, {}
    for it in listing.get("items") or []:
        if it.get("type") != "movie" or it.get("version_of") or it.get("section_id") not in ok:
            continue
        ids = it.get("ids") or {}
        if str(ids.get("tmdb") or "").isdigit():
            by_tmdb.setdefault(int(ids["tmdb"]), []).append(it["id"])
        if ids.get("imdb"):
            by_imdb.setdefault(str(ids["imdb"]), []).append(it["id"])
        by_name.setdefault((norm(it.get("title")), it.get("year")), []).append(it["id"])
    return by_tmdb, by_imdb, by_name


def match(films, index):
    """The library ids of the films (each film may be there as several titles)."""
    by_tmdb, by_imdb, by_name = index
    out = []
    for f in films:
        ids = by_tmdb.get(f.get("tmdb")) or by_imdb.get(f.get("imdb")) or by_name.get((norm(f.get("title")), f.get("year"))) or []
        for i in ids:
            if i not in out:
                out.append(i)
    return out


def lists_of(settings):
    out = []
    for e in settings.get("lists") or []:
        if isinstance(e, dict) and str(e.get("name") or "").strip() and str(e.get("source") or "").strip():
            out.append((str(e["name"]).strip(), str(e["source"]).strip()))
    return out


def run(server, settings):
    """Looks every list up and gives the server's answer {"collections": {name: [ids]}} and the state."""
    listing = fetch_library(server)
    index = library_index(listing, settings)
    cache = read_json(CACHE, {})
    budget = [NEW_LOOKUPS]
    old = read_json(STATE, {})
    collections, report = {}, []
    for name, source in lists_of(settings):
        try:
            films = films_of(source, settings, cache, budget)
            ids = match(films, index)
            collections[name] = ids
            report.append({"name": name, "source": source, "found": len(ids), "total": len(films)})
        except Exception as e:  # noqa: BLE001 — shown in the panel, the other lists go on
            # Not reachable: the collection is left as it is.
            report.append({"name": name, "source": source, "error": str(e)})
    # Lists taken out of the settings: their collections are emptied once.
    for name in old.get("managed") or []:
        if name not in collections and not any(r["name"] == name for r in report):
            collections[name] = []
    write_json(CACHE, cache)
    managed = sorted({r["name"] for r in report})
    write_json(STATE, {"at": int(time.time()), "managed": managed, "lists": report})
    return {"collections": collections}, report


# ---------------------------------------------------------------- files

def read_json(path, default):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def write_json(path, value):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(value, f)
    os.replace(tmp, path)


@contextlib.contextmanager
def locked():
    """One run at a time (the daily one and a click must not overlap)."""
    for _ in range(600):
        try:
            os.mkdir(LOCK)
            break
        except FileExistsError:
            try:
                if time.time() - os.path.getmtime(LOCK) > 1200:
                    os.rmdir(LOCK)
                    continue
            except OSError:
                pass
            time.sleep(1)
    try:
        yield
    finally:
        with contextlib.suppress(OSError):
            os.rmdir(LOCK)


# ---------------------------------------------------------------- the panel

def when(ts, lang):
    return time.strftime("%d.%m.%Y, %H:%M" if (lang or "").startswith("de") else "%Y-%m-%d %H:%M", time.localtime(ts))


def panel(lang, settings, message=None):
    state = read_json(STATE, {})
    lists = lists_of(settings)
    buttons = [{"label": tr(lang, "Update now"), "action": "update", "primary": True}] if lists else []
    if not lists:
        return {"title": tr(lang, "Smart collections"), "text": tr(lang, "No lists set up. Add some under Plugins → Smart collections → settings."), "buttons": []}
    done = {r["name"]: r for r in state.get("lists") or []}
    items = []
    for name, source in lists:
        r = done.get(name)
        if r is None:
            status = tr(lang, "Not updated yet.")
        elif r.get("error"):
            status = tr(lang, "error") + ": " + tr(lang, r["error"])
        else:
            status = tr(lang, "{n} of {m} in your libraries", n=r["found"], m=r["total"])
        items.append({"title": name, "subtitle": source, "status": status, "status_kind": "default" if r and not r.get("error") else None})
    text = tr(lang, "Updated {when}.", when=when(state["at"], lang)) if state.get("at") else tr(lang, "Not updated yet.")
    if message:
        text = message + " " + text
    return {"title": tr(lang, "Smart collections"), "text": text, "items": items, "buttons": buttons}


def answer_call(data, settings, server):
    body = data.get("body") if isinstance(data.get("body"), dict) else {}
    lang = body.get("lang") or "en"
    if not data.get("admin"):
        return {"error": tr(lang, "Only admins can see the smart collections.")}
    action = data.get("action")
    if action == "panel":
        return panel(lang, settings)
    if action == "update":
        try:
            with locked():
                out, report = run(server, settings)
        except LookupError:
            return {"error": tr(lang, "This plugin needs a server that lets plugins read the library.")}
        except Exception as e:  # noqa: BLE001 — shown to the admin
            return {"error": tr(lang, "The library could not be read: {e}", e=e)}
        out["panel"] = panel(lang, settings, tr(lang, "Updated: {n} collections.", n=len([r for r in report if not r.get("error")])))
        return out
    return {"error": "unknown action %s" % action}


def main():
    # The server sends UTF-8, whatever the console code page is (Windows).
    msg = json.loads(getattr(sys.stdin, "buffer", sys.stdin).read())
    event = msg.get("event", "")
    data = msg.get("data") or {}
    settings = msg.get("settings") or {}
    server = msg.get("server") or {}
    if event == "api.call":
        print(json.dumps(answer_call(data, settings, server)))
        return
    if event != "schedule.daily":
        print(json.dumps({"skipped": event}))
        return
    with locked():
        out, report = run(server, settings)
    out["lists"] = [{k: r.get(k) for k in ("name", "found", "total", "error") if k in r} for r in report]
    print(json.dumps(out))


if __name__ == "__main__":
    main()
