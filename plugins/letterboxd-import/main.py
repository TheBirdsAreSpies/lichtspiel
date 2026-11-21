"""Letterboxd import plugin.

Letterboxd has no open API, but anyone can download their data (letterboxd.com → Settings → Import
& Export → Export your data: a ZIP of CSV files). Each person uploads theirs in their panel; the
plugin reads what they watched (with the dates from the diary), their ratings, their watchlist and
their lists, and brings them in on a click:

- watched → marked as watched with the date from Letterboxd,
- ratings → their rating (Letterboxd's ½–5 stars become 1–10), where they have none yet,
- the watchlist → onto their watchlist, or as a playlist,
- each list → as a playlist (filled anew when imported again), or onto their watchlist.

Only titles in the libraries count. A film is found by its TMDB or IMDb id, which the plugin looks up
once on its Letterboxd page (the export has only titles and short links) and keeps in cache.json;
a film found by its title and year needs no lookup. What each person uploaded is kept in
users/<hash>.json in the plugin's folder until they press "Forget".
"""
import base64
import calendar
import concurrent.futures
import csv
import hashlib
import io
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
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
USERS = os.path.join(HERE, "users")
CACHE = os.path.join(HERE, "cache.json")
UA = "Lichtspiel-letterboxd-import/1.0"
TIMEOUT = 20
WORK_SECONDS = 20          # lookups a call (the page asks again until all are done)
WORKERS = 4
PAUSE = 0.25
MAX_FILE = 20 * 1024 * 1024
ALLOWED = re.compile(r"^https://(boxd\.it|letterboxd\.com)/")

WORDS = {
    "Letterboxd import": "Letterboxd-Import",
    "Bring in what you watched, your ratings, your watchlist and your lists from Letterboxd. Download your data on letterboxd.com under Settings → Import & Export → Export your data and choose the ZIP file here.":
        "Übernimm, was du gesehen hast, deine Bewertungen, deine Watchlist und deine Listen von Letterboxd. Lade deine Daten auf letterboxd.com unter Settings → Import & Export → Export your data herunter und wähle die ZIP-Datei hier aus.",
    "Upload": "Hochladen",
    "Uploaded {when}: {n} films.": "Hochgeladen am {when}: {n} Filme.",
    "{n} films still to look up on Letterboxd.": "{n} Filme sind noch auf Letterboxd nachzuschlagen.",
    "Look up the rest": "Rest nachschlagen",
    "Watched and ratings": "Gesehen und Bewertungen übernehmen",
    "Forget the upload": "Upload vergessen",
    "Forget what you uploaded? What was brought in stays.": "Den Upload vergessen? Was schon übernommen ist, bleibt.",
    "Watched": "Gesehen",
    "Ratings": "Bewertungen",
    "Watchlist": "Watchlist",
    "List": "Liste",
    "Bring in": "Übernehmen",
    "Onto my watchlist": "Auf meine Watchlist",
    "As a playlist": "Als Playlist",
    "{n} of {m} in the libraries": "{n} von {m} in den Mediatheken",
    "brought in {when}": "übernommen am {when}",
    "This is not a Letterboxd export (a ZIP or CSV file from Letterboxd).": "Das ist kein Letterboxd-Export (eine ZIP- oder CSV-Datei von Letterboxd).",
    "Read: {n} films.": "Gelesen: {n} Filme.",
    "Looking the films up on Letterboxd: {n} of {m} …": "Schlage die Filme auf Letterboxd nach: {n} von {m} …",
    "All films looked up.": "Alle Filme nachgeschlagen.",
    "Letterboxd could not be reached; {n} films are still to look up.": "Letterboxd war nicht erreichbar; {n} Filme sind noch nachzuschlagen.",
    "Nothing uploaded yet.": "Noch nichts hochgeladen.",
    "Forgotten.": "Vergessen.",
    "The library could not be read: {e}": "Die Mediathek konnte nicht gelesen werden: {e}",
    "This plugin needs a server that lets plugins read the library.": "Dieses Plugin braucht einen Server, der Plugins die Mediathek lesen lässt.",
    "From Letterboxd": "Von Letterboxd",
    "Letterboxd: Watchlist": "Letterboxd: Watchlist",
}


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def norm(text):
    text = unicodedata.normalize("NFKD", str(text or "")).encode("ascii", "ignore").decode().lower()
    text = re.sub(r"[^a-z0-9]+", " ", text).strip()
    return re.sub(r"^(the|a|an|der|die|das) ", "", text)


# ---------------------------------------------------------------- reading the export

def year_of(text):
    text = str(text or "").strip()
    return int(text) if text.isdigit() else None


def day(text):
    """"2024-03-09" → unix seconds (noon UTC that day), else None."""
    try:
        return calendar.timegm(time.strptime(str(text).strip()[:10], "%Y-%m-%d")) + 12 * 3600
    except ValueError:
        return None


def rows(text):
    return list(csv.reader(io.StringIO(text)))


def table(text):
    """A simple export file (watched, ratings, diary, watchlist) as dicts."""
    r = rows(text)
    if not r:
        return []
    head = [h.strip() for h in r[0]]
    return [dict(zip(head, x)) for x in r[1:] if any(c.strip() for c in x)]


def parse_list(text, fallback):
    """A list's export: a block about the list, then its films. Gives (name, [film rows])."""
    r = rows(text)
    name, films = fallback, []
    for i, row in enumerate(r):
        head = [c.strip() for c in row]
        if head[:2] == ["Date", "Name"] and i + 1 < len(r) and len(r[i + 1]) > 1 and r[i + 1][1].strip():
            name = r[i + 1][1].strip()
        if head and head[0] == "Position":
            films = [dict(zip(head, x)) for x in r[i + 1:] if any(c.strip() for c in x)]
            break
    return name, films


def parse_export(blob, filename=""):
    """The upload (a ZIP of Letterboxd's export, or one of its CSV files) → the person's data:
    {"films": {key: {"title", "year", "uri"}}, "watched": {key: {"at", "plays"}}, "ratings": {key: stars},
    "watchlist": [keys], "lists": [{"name", "keys"}]}. Raises ValueError for anything else."""
    files = {}
    if blob[:2] == b"PK":
        try:
            z = zipfile.ZipFile(io.BytesIO(blob))
        except zipfile.BadZipFile as e:
            raise ValueError("bad zip") from e
        for info in z.infolist():
            low = info.filename.replace("\\", "/").lower()
            if info.is_dir() or not low.endswith(".csv") or info.file_size > MAX_FILE or low.startswith(("deleted/", "orphaned/")) or "/deleted/" in low:
                continue
            files[low] = z.read(info).decode("utf-8-sig", "replace")
    else:
        try:
            text = blob.decode("utf-8-sig")
        except UnicodeDecodeError as e:
            raise ValueError("not text") from e
        low = os.path.basename(filename or "upload.csv").lower()
        if text.startswith("Letterboxd list export"):
            low = "lists/" + low
        files[low] = text
    data = {"films": {}, "watched": {}, "ratings": {}, "watchlist": [], "lists": []}

    def film(row, own_uri=True):
        title = (row.get("Name") or "").strip()
        if not title:
            return None
        year = year_of(row.get("Year"))
        key = "%s\t%s" % (title, year or "")
        uri = (row.get("Letterboxd URI") or row.get("URL") or "").strip()
        f = data["films"].setdefault(key, {"title": title, "year": year})
        if own_uri and ALLOWED.match(uri) and "uri" not in f:
            f["uri"] = uri
        return key

    known = False
    # The films' own short links first; a diary's link is to the entry, not to the film.
    order = {"watched.csv": 0, "ratings.csv": 1, "watchlist.csv": 2}
    for name, text in sorted(files.items(), key=lambda f: (order.get(f[0].rsplit("/", 1)[-1], 3), f[0])):
        base = name.rsplit("/", 1)[-1]
        if name.startswith("lists/") or "/lists/" in name:
            list_name, list_rows = parse_list(text, base[:-4].replace("-", " "))
            keys = [k for k in (film(x) for x in list_rows) if k]
            data["lists"].append({"name": list_name, "keys": list(dict.fromkeys(keys))})
            known = True
            continue
        if base == "watched.csv":
            for x in table(text):
                k = film(x)
                if k:
                    data["watched"].setdefault(k, {"at": day(x.get("Date")), "plays": 1})
            known = True
        elif base == "diary.csv":
            for x in table(text):
                k = film(x, own_uri=False)
                if not k:
                    continue
                at = day(x.get("Watched Date")) or day(x.get("Date"))
                w = data["watched"].setdefault(k, {"at": at, "plays": 0, "diary": True})
                if not w.get("diary"):
                    w.update({"at": at or w["at"], "plays": 0, "diary": True})
                w["plays"] += 1
                if at and (not w["at"] or at < w["at"]):
                    w["at"] = at
            known = True
        elif base == "ratings.csv":
            for x in table(text):
                k = film(x)
                try:
                    stars = float(x.get("Rating") or "")
                except ValueError:
                    continue
                if k and 0 < stars <= 5:
                    data["ratings"][k] = stars
            known = True
        elif base == "watchlist.csv":
            data["watchlist"] = list(dict.fromkeys(k for k in (film(x) for x in table(text)) if k))
            known = True
    if not known:
        raise ValueError("not a Letterboxd export")
    for w in data["watched"].values():
        w.pop("diary", None)
        w["plays"] = max(1, w["plays"])
    return data


# ---------------------------------------------------------------- the web

def get(url, accept="text/html"):
    ctx = None
    host = urllib.parse.urlparse(url).hostname
    if url.startswith("https://") and host in ("127.0.0.1", "localhost", "::1"):
        # The server's own certificate may be made for its public name, not for 127.0.0.1.
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": accept})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=TIMEOUT, context=ctx) as r:
                return r.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            if e.code in (429, 503) and attempt < 2:
                time.sleep(5 * (attempt + 1))
                continue
            raise


def fetch_library(server):
    address = (server or {}).get("library")
    if not address:
        raise LookupError("no library address")
    return json.loads(get(address, "application/json"))


def lookup(uri):
    """A Letterboxd film's TMDB and IMDb ids, from its page ({} when it has none)."""
    text = get(uri) or ""
    tmdb = re.search(r'data-tmdb-id="(\d+)"', text)
    imdb = re.search(r"imdb\.com/title/(tt\d+)", text)
    out = {}
    if tmdb:
        out["tmdb"] = int(tmdb.group(1))
    if imdb:
        out["imdb"] = imdb.group(1)
    time.sleep(PAUSE)
    return out


# ---------------------------------------------------------------- matching

def library_index(listing):
    """Every movie, findable by TMDB id, IMDb id and title with year."""
    movie_sections = {s.get("id") for s in listing.get("sections") or [] if s.get("kind") == "movie"}
    by_tmdb, by_imdb, by_name = {}, {}, {}
    for it in listing.get("items") or []:
        if it.get("type") != "movie" or it.get("version_of") or (movie_sections and it.get("section_id") not in movie_sections):
            continue
        ids = it.get("ids") or {}
        if str(ids.get("tmdb") or "").isdigit():
            by_tmdb.setdefault(int(ids["tmdb"]), []).append(it["id"])
        if ids.get("imdb"):
            by_imdb.setdefault(str(ids["imdb"]), []).append(it["id"])
        by_name.setdefault((norm(it.get("title")), it.get("year")), []).append(it["id"])
    return by_tmdb, by_imdb, by_name


def ids_of(film, cache, index):
    """The library ids of one film of the export ([] when not in the libraries)."""
    by_tmdb, by_imdb, by_name = index
    known = cache.get(film.get("uri") or "") or {}
    return by_tmdb.get(known.get("tmdb")) or by_imdb.get(known.get("imdb")) or by_name.get((norm(film["title"]), film.get("year"))) or []


def to_look_up(data, cache, index):
    """Films with a Letterboxd address, not looked up yet and not found by title and year."""
    by_name = index[2]
    return [f["uri"] for f in data["films"].values()
            if f.get("uri") and f["uri"] not in cache and (norm(f["title"]), f.get("year")) not in by_name]


def look_up_some(uris, seconds=WORK_SECONDS):
    """Looks films up for about `seconds`; gives ({uri: ids}, failures)."""
    found, failed = {}, 0
    start = time.time()
    with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
        pending = list(uris)
        while pending and time.time() - start < seconds and failed < 5:
            batch, pending = pending[:WORKERS], pending[WORKERS:]
            for uri, fut in [(u, pool.submit(lookup, u)) for u in batch]:
                try:
                    found[uri] = fut.result()
                except Exception:  # noqa: BLE001 — tried again on the next call
                    failed += 1
    return found, failed


# ---------------------------------------------------------------- files

def read_json(path, default):
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def write_json(path, value):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".%d.tmp" % os.getpid()
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(value, f)
    os.replace(tmp, path)


def user_file(user):
    return os.path.join(USERS, hashlib.sha256(str(user).encode("utf-8")).hexdigest()[:24] + ".json")


def add_to_cache(found):
    """Merges new lookups into the cache (another person's lookups may have been added meanwhile)."""
    cache = read_json(CACHE, {})
    cache.update(found)
    write_json(CACHE, cache)
    return cache


# ---------------------------------------------------------------- the panel

def when(ts, lang):
    return time.strftime("%d.%m.%Y" if (lang or "").startswith("de") else "%Y-%m-%d", time.localtime(ts))


def status(lang, found, total, done):
    s = tr(lang, "{n} of {m} in the libraries", n=found, m=total)
    return s + " · " + tr(lang, "brought in {when}", when=when(done, lang)) if done else s


def panel(lang, data, cache, index):
    upload = {"label": tr(lang, "Upload"), "action": "upload", "accept": ".zip,.csv"}
    title = tr(lang, "Letterboxd import")
    intro = tr(lang, "Bring in what you watched, your ratings, your watchlist and your lists from Letterboxd. Download your data on letterboxd.com under Settings → Import & Export → Export your data and choose the ZIP file here.")
    if not data:
        return {"title": title, "text": intro, "upload": upload, "buttons": []}
    done = data.get("done") or {}
    films = data["films"]
    count = lambda keys: sum(1 for k in keys if k in films and ids_of(films[k], cache, index))
    text = intro + " " + tr(lang, "Uploaded {when}: {n} films.", when=when(data.get("uploaded_at") or time.time(), lang), n=len(films))
    pending = len(to_look_up(data, cache, index))
    buttons = []
    if pending:
        text += " " + tr(lang, "{n} films still to look up on Letterboxd.", n=pending)
        buttons.append({"label": tr(lang, "Look up the rest"), "action": "lookup"})
    items = []
    if data["watched"]:
        items.append({"title": tr(lang, "Watched"), "status": status(lang, count(data["watched"]), len(data["watched"]), done.get("watched")),
                      "status_kind": "default" if done.get("watched") else None,
                      "buttons": [{"label": tr(lang, "Bring in"), "action": "bring", "args": {"what": "watched"}}]})
    if data["ratings"]:
        items.append({"title": tr(lang, "Ratings"), "status": status(lang, count(data["ratings"]), len(data["ratings"]), done.get("ratings")),
                      "status_kind": "default" if done.get("ratings") else None,
                      "buttons": [{"label": tr(lang, "Bring in"), "action": "bring", "args": {"what": "ratings"}}]})
    lists = [("watchlist", tr(lang, "Watchlist"), data["watchlist"])] if data["watchlist"] else []
    lists += [("list:%d" % i, l["name"], l["keys"]) for i, l in enumerate(data["lists"])]
    for what, name, keys in lists:
        items.append({"title": name, "subtitle": tr(lang, "List") if what.startswith("list:") else None,
                      "status": status(lang, count(keys), len(keys), done.get(what)), "status_kind": "default" if done.get(what) else None,
                      "buttons": [{"label": tr(lang, "As a playlist"), "action": "bring", "args": {"what": what, "as": "playlist"}},
                                  {"label": tr(lang, "Onto my watchlist"), "action": "bring", "args": {"what": what, "as": "watchlist"}}]})
    for it in items:
        if it.get("subtitle") is None:
            it.pop("subtitle", None)
        if it.get("status_kind") is None:
            it.pop("status_kind", None)
    if data["watched"] or data["ratings"]:
        buttons.insert(0, {"label": tr(lang, "Watched and ratings"), "action": "bring", "args": {"what": "watched+ratings"}, "primary": True})
    buttons.append({"label": tr(lang, "Forget the upload"), "action": "forget", "confirm": tr(lang, "Forget what you uploaded? What was brought in stays.")})
    return {"title": title, "text": text, "upload": upload, "items": items, "buttons": buttons}


def bring(lang, data, cache, index, what, as_):
    """The server's answer that brings one part in for the caller."""
    films = data["films"]
    first = lambda keys: list(dict.fromkeys(ids_of(films[k], cache, index)[0] for k in keys if k in films and ids_of(films[k], cache, index)))
    out, parts = {}, what.split("+")
    if "watched" in parts:
        out["mark_watched"] = [{"id": i, "watched_at": w.get("at") or 0, "plays": w.get("plays") or 1}
                               for k, w in data["watched"].items() if k in films for i in ids_of(films[k], cache, index)]
    if "ratings" in parts:
        out["set_ratings"] = [{"id": i, "rating": round(stars * 2, 1)} for k, stars in data["ratings"].items() if k in films for i in ids_of(films[k], cache, index)]
    if what == "watchlist" or what.startswith("list:"):
        if what == "watchlist":
            name, keys = tr(lang, "Letterboxd: Watchlist"), data["watchlist"]
        else:
            i = int(what[5:]) if what[5:].isdigit() else -1
            if not 0 <= i < len(data["lists"]):
                return None
            name, keys = "Letterboxd: " + data["lists"][i]["name"], data["lists"][i]["keys"]
        ids = first(keys)
        if as_ == "watchlist":
            out["add_to_watchlist"] = ids
        else:
            out["save_playlist"] = {"title": name, "summary": tr(lang, "From Letterboxd"), "ids": ids}
    return out or None


def answer_call(data, server):
    body = data.get("body") if isinstance(data.get("body"), dict) else {}
    lang = body.get("lang") or "en"
    action = data.get("action")
    path = user_file(data.get("user") or "")
    mine = read_json(path, None)
    if action == "forget":
        if os.path.exists(path):
            os.remove(path)
        return {"message": tr(lang, "Forgotten."), "panel": panel(lang, None, {}, ({}, {}, {}))}
    upload = None
    if action == "upload":
        try:
            blob = base64.b64decode(str(body.get("file") or ""), validate=False)
            upload = parse_export(blob, str(body.get("name") or ""))
        except (ValueError, TypeError):
            return {"error": tr(lang, "This is not a Letterboxd export (a ZIP or CSV file from Letterboxd).")}
        upload["uploaded_at"] = int(time.time())
        if mine and len(upload["films"]) and not upload["watched"] and not upload["ratings"] and not upload["watchlist"]:
            # One list on its own: added to what is there.
            names = {l["name"] for l in upload["lists"]}
            mine["films"].update({k: v for k, v in upload["films"].items() if k not in mine["films"]})
            mine["lists"] = [l for l in mine["lists"] if l["name"] not in names] + upload["lists"]
            mine["uploaded_at"] = upload["uploaded_at"]
        else:
            mine = upload
        write_json(path, mine)
    try:
        index = library_index(fetch_library(server))
    except LookupError:
        return {"error": tr(lang, "This plugin needs a server that lets plugins read the library.")}
    except Exception as e:  # noqa: BLE001 — shown to the person
        return {"error": tr(lang, "The library could not be read: {e}", e=e)}
    cache = read_json(CACHE, {})
    if action == "panel":
        return panel(lang, mine, cache, index)
    if not mine:
        return {"error": tr(lang, "Nothing uploaded yet.")}
    if action in ("upload", "lookup"):
        todo = to_look_up(mine, cache, index)
        total = len([f for f in mine["films"].values() if f.get("uri")])
        if action == "lookup" and todo:
            found, failed = look_up_some(todo)
            cache = add_to_cache(found)
            todo = to_look_up(mine, cache, index)
            if failed and not found:
                return {"error": tr(lang, "Letterboxd could not be reached; {n} films are still to look up.", n=len(todo))}
        if todo:
            return {"message": tr(lang, "Read: {n} films.", n=len(mine["films"])),
                    "wait": tr(lang, "Looking the films up on Letterboxd: {n} of {m} …", n=total - len(todo), m=total),
                    "poll": {"action": "lookup", "seconds": 2}}
        msg = tr(lang, "All films looked up.") if action == "lookup" else tr(lang, "Read: {n} films.", n=len(mine["films"]))
        return {"message": msg, "panel": panel(lang, mine, cache, index)}
    if action == "bring":
        args = body.get("args") if isinstance(body.get("args"), dict) else {}
        what, as_ = str(args.get("what") or ""), str(args.get("as") or "playlist")
        out = bring(lang, mine, cache, index, what, as_)
        if out is None:
            return {"error": "unknown part %s" % what}
        done = mine.setdefault("done", {})
        for part in what.split("+"):
            done[part] = int(time.time())
        write_json(path, mine)
        out["panel"] = panel(lang, mine, cache, index)
        return out
    return {"error": "unknown action %s" % action}


def main():
    # The server sends UTF-8, whatever the console code page is (Windows).
    msg = json.loads(getattr(sys.stdin, "buffer", sys.stdin).read())
    if msg.get("event") != "api.call":
        print(json.dumps({"skipped": msg.get("event")}))
        return
    print(json.dumps(answer_call(msg.get("data") or {}, msg.get("server") or {})))


if __name__ == "__main__":
    main()
