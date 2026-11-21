"""Subtitles plugin.

When a movie or episode arrives (library.item.added), looks for subtitles in the languages from
settings["languages"] on OpenSubtitles and saves the best one as "<video name>.<language>.srt" in
the server's subtitle folder (<data>/subtitles, below it the video's folder), so the media folder
may stay read-only; with settings["next_to_video"] it goes next to the video instead. The answer
asks the server to look for the item's subtitles again. A language the file already has (inside
the video or as a subtitle file) is skipped.

Its own API action: POST /api/plugins/subtitles/call/test checks the key (admins only).
Needs a free API key from https://www.opensubtitles.com/consumers.
"""
import datetime
import json
import os
import struct
import sys
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.opensubtitles.com/api/v1"
AGENT = "Lichtspiel subtitles v1.0"
TIMEOUT = 20
HERE = os.path.dirname(os.path.abspath(__file__))
# Kept in the plugin's folder: the sign-in of today, and the day the download limit was reached.
TOKEN_FILE = os.path.join(HERE, ".token.json")
LIMIT_FILE = os.path.join(HERE, ".limit-reached")

# OpenSubtitles uses two letters; ffprobe reports the three letters of ISO 639-2 (both forms).
THREE = {
    "de": ["ger", "deu"], "en": ["eng"], "fr": ["fre", "fra"], "es": ["spa"], "it": ["ita"],
    "nl": ["dut", "nld"], "pl": ["pol"], "pt": ["por"], "pt-br": ["por"], "pt-pt": ["por"],
    "ru": ["rus"], "tr": ["tur"], "sv": ["swe"], "da": ["dan"], "no": ["nor", "nob", "nno"],
    "fi": ["fin"], "cs": ["cze", "ces"], "hu": ["hun"], "el": ["gre", "ell"], "ja": ["jpn"],
    "ko": ["kor"], "zh-cn": ["chi", "zho"], "zh-tw": ["chi", "zho"], "ar": ["ara"], "he": ["heb"],
    "uk": ["ukr"], "ro": ["rum", "ron"], "hr": ["hrv"], "sr": ["srp"], "bg": ["bul"],
}


class Stop(Exception):
    """Something that ends this run (no key, daily limit reached)."""


class Limit(Stop):
    """The daily download limit: no more tries until tomorrow."""


def today():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")


def read(path):
    try:
        with open(path, encoding="utf-8") as f:
            return f.read()
    except OSError:
        return ""


def write_private(path, text):
    try:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(text)
    except OSError as e:
        log("could not write %s: %s" % (path, e))


def log(text):
    print(text, file=sys.stderr)


def has_language(files, lang):
    """The video already has subtitles in lang: inside the file or as a file next to it."""
    lang = lang.lower()
    codes = set([lang, lang.split("-")[0]] + THREE.get(lang, []))
    for f in files:
        for s in f.get("streams") or []:
            if s.get("kind") == "subtitle" and not s.get("forced") and (s.get("language") or "").lower() in codes:
                return True
        for sub in f.get("external_subtitles") or []:
            parts = os.path.basename(sub).lower().split(".")
            # "Film.de.srt", "Film.ger.forced.srt", "Film.de.sdh.srt": any part between name and extension
            if any(p in codes for p in parts[1:-1]) and "forced" not in parts:
                return True
    return False


def movie_hash(path):
    """OpenSubtitles' hash: the size plus the 64-bit sum of the first and last 64 KB. None if unreadable."""
    try:
        size = os.path.getsize(path)
        if size < 131072:
            return None
        h = size
        with open(path, "rb") as f:
            for pos in (0, size - 65536):
                f.seek(pos)
                for (n,) in struct.iter_unpack("<Q", f.read(65536)):
                    h = (h + n) & 0xFFFFFFFFFFFFFFFF
        return "%016x" % h
    except OSError:
        return None


def number(imdb):
    """"tt0133093" -> "133093" (OpenSubtitles wants the number)."""
    digits = "".join(c for c in str(imdb or "") if c.isdigit())
    return str(int(digits)) if digits else ""


def search_params(item, lang, mhash):
    p = {"languages": lang}
    ids = item.get("ids") or {}
    if item.get("type") == "episode":
        show = (item.get("show") or {}).get("ids") or {}
        if item.get("season") is None or item.get("episode") is None:
            return None
        p["season_number"] = item["season"]
        p["episode_number"] = item["episode"]
        if number(show.get("imdb")):
            p["parent_imdb_id"] = number(show["imdb"])
        elif show.get("tmdb"):
            p["parent_tmdb_id"] = show["tmdb"]
        elif (item.get("show") or {}).get("title"):
            p["query"] = item["show"]["title"]
        else:
            return None
    else:
        if number(ids.get("imdb")):
            p["imdb_id"] = number(ids["imdb"])
        elif ids.get("tmdb"):
            p["tmdb_id"] = ids["tmdb"]
        elif item.get("title"):
            p["query"] = item["title"]
            if item.get("year"):
                p["year"] = item["year"]
        else:
            return None
    if mhash:
        p["moviehash"] = mhash
    return p


def request(method, path, key, token=None, params=None, body=None):
    url = API + path
    if params:
        # Sorted and lower case, as OpenSubtitles asks (otherwise it answers with a redirect).
        url += "?" + urllib.parse.urlencode(sorted((k, str(v).lower()) for k, v in params.items()))
    headers = {"Api-Key": key, "User-Agent": AGENT, "Accept": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
            return json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        try:
            msg = json.loads(e.read() or b"{}")
            msg = msg.get("message") or msg.get("errors") or msg
        except Exception:
            msg = e.reason
        if e.code == 401 or e.code == 403:
            raise Stop("OpenSubtitles refused the key or the sign-in: %s" % msg)
        if e.code == 406 or e.code == 429:
            raise Limit("OpenSubtitles: daily download limit reached or too many requests (%s)" % msg)
        raise RuntimeError("OpenSubtitles answered %d: %s" % (e.code, msg))


def best(results, settings):
    """The subtitle to take: matching file hash first, then not machine-made, then the most downloaded."""
    want_sdh = bool(settings.get("hearing_impaired"))
    cands = []
    for r in results:
        a = r.get("attributes") or {}
        files = a.get("files") or []
        if not files or a.get("foreign_parts_only"):
            continue
        made = bool(a.get("machine_translated") or a.get("ai_translated"))
        if made and not settings.get("allow_machine_translated"):
            continue
        score = (bool(a.get("moviehash_match")), not made, bool(a.get("hearing_impaired")) == want_sdh, a.get("download_count") or 0)
        cands.append((score, files[0]["file_id"]))
    if not cands:
        return None
    return max(cands, key=lambda c: c[0])[1]


def sign_in(settings):
    key = settings.get("api_key") or ""
    if not key:
        raise Stop("No OpenSubtitles API key: put it in the settings as \"api_key\"")
    token = None
    user = settings.get("username") or ""
    if user and settings.get("password"):
        # A sign-in lasts a day; OpenSubtitles allows only a few a minute, so reuse it.
        try:
            saved = json.loads(read(TOKEN_FILE) or "{}")
        except ValueError:
            saved = {}
        if saved.get("date") == today() and saved.get("user") == user and saved.get("token"):
            return key, saved["token"]
        token = request("POST", "/login", key, body={"username": user, "password": settings["password"]}).get("token")
        if token:
            write_private(TOKEN_FILE, json.dumps({"date": today(), "user": user, "token": token}))
    return key, token


def store_dir(store, folder):
    """A video folder's place in the subtitle store, as the server works it out:
    "/media/Movies/X" -> "<store>/media/Movies/X", "C:\\Movies\\X" -> "<store>/C/Movies/X"."""
    drive, rest = os.path.splitdrive(folder)
    parts = []
    d = "".join(c for c in drive if c not in "\\/?.:")
    if d:
        parts.append(d)
    parts += [p for p in rest.replace("\\", "/").split("/") if p not in ("", ".", "..")]
    return os.path.join(store, *parts)


def fetch(item, settings, data_dir=""):
    """Finds and saves the missing subtitles of one item. Returns the files written."""
    files = item.get("files") or []
    if item.get("type") not in ("movie", "episode") or not files:
        return []
    video = files[0]["path"]
    langs = settings.get("languages") or ["de", "en"]
    if isinstance(langs, str):
        langs = [l.strip() for l in langs.split(",") if l.strip()]
    folder, name = os.path.split(video)
    name = os.path.splitext(name)[0]
    beside = os.path.join(folder, name)
    if settings.get("next_to_video") or not data_dir:
        stem = beside
    else:
        stem = os.path.join(store_dir(os.path.join(data_dir, "subtitles"), folder), name)
    missing = [l.lower() for l in langs if not has_language(files, l)
               and not any(os.path.exists(s + ".%s.srt" % l.lower()) for s in (stem, beside))]
    if not missing:
        return []
    if read(LIMIT_FILE).strip() == today():
        raise Limit("Daily download limit of OpenSubtitles reached; %s waits until tomorrow" % item.get("title"))
    key, token = sign_in(settings)
    mhash = movie_hash(video)
    written = []
    for lang in missing:
        params = search_params(item, lang, mhash)
        if params is None:
            log("%s: not enough to search with (no ids, title or episode numbers)" % item.get("title"))
            return written
        found = request("GET", "/subtitles", key, token, params=params).get("data") or []
        file_id = best(found, settings)
        if not file_id:
            log("%s: no %s subtitles found" % (item.get("title"), lang))
            continue
        link = request("POST", "/download", key, token, body={"file_id": file_id, "sub_format": "srt"}).get("link")
        if not link or urllib.parse.urlsplit(link).scheme not in ("http", "https"):
            log("%s: OpenSubtitles gave no download link" % item.get("title"))
            continue
        with urllib.request.urlopen(urllib.request.Request(link, headers={"User-Agent": AGENT}), timeout=TIMEOUT) as r:
            text = r.read(10 * 1024 * 1024)
        target = stem + ".%s.srt" % lang
        try:
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with open(target, "xb") as f:
                f.write(text)
        except OSError as e:
            hint = " Is the media folder mounted writable? Without \"next_to_video\" the subtitles go to the server's data folder." if stem == beside and data_dir else ""
            raise Stop("Could not save %s (%s).%s" % (target, e.strerror or e, hint))
        written.append(target)
    return written


def main():
    msg = json.load(sys.stdin)
    event = msg.get("event", "")
    data = msg.get("data") or {}
    settings = msg.get("settings") or {}
    try:
        if event == "api.call":
            if not data.get("admin"):
                print(json.dumps({"error": "Only an admin can do this"}))
                return
            if data.get("action") == "test":
                key, token = sign_in(settings)
                info = request("GET", "/infos/user", key, token) if token else {}
                left = ((info.get("data") or {}).get("remaining_downloads"))
                print(json.dumps({"message": "The key works" + ("" if left is None else "; %s downloads left today" % left)}))
                return
            print(json.dumps({"error": "Unknown action"}))
            return
    except (Stop, RuntimeError, OSError) as e:
        print(json.dumps({"error": str(e)}))
        return
    if event != "library.item.added":
        print(json.dumps({"skipped": event}))
        return
    try:
        written = fetch(data.get("item") or {}, settings, (msg.get("server") or {}).get("data_dir") or "")
    except (Stop, RuntimeError, OSError) as e:
        if isinstance(e, Limit):
            write_private(LIMIT_FILE, today())
        # Shown on the Plugins page as an error of this run.
        log(str(e))
        sys.exit(1)
    print(json.dumps({"ok": True, "saved": written, "refresh_subtitles": bool(written)}))


if __name__ == "__main__":
    main()
