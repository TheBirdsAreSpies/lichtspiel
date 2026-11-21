"""Library doctor plugin.

Looks through the whole library after each scan that changed something, once a day and when an
admin asks, and lists what may need a hand:

- the same movie or episode twice (not counting versions merged on purpose),
- videos below a resolution (default: narrower than 1280 pixels, i.e. below HD),
- videos without an audio track in the chosen languages (with a note if subtitles in them exist),
  and videos whose audio tracks carry no language at all,
- files whose streams could not be read,
- names the scanner could not read well: movies without a year, episodes without a season or
  episode number, and titles for which no details were found online.

The report is an admin panel under Settings. Entries can be ignored; they stay hidden until the
ignored list is cleared. The report is kept in report.json, the ignored entries in ignored.json,
both in the plugin's folder.
"""
import contextlib
import json
import os
import re
import ssl
import sys
import time
import unicodedata
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPORT = os.path.join(HERE, "report.json")
IGNORED = os.path.join(HERE, "ignored.json")
LOCK = os.path.join(HERE, ".doctor.lock")
SHOW_AT_MOST = 100

# The order of the report.
CHECKS = ["duplicate", "low_resolution", "no_language", "untagged_audio", "unreadable", "no_year", "no_numbers", "no_details"]

WORDS = {
    "Library doctor": "Bibliotheks-Doktor",
    "Only admins can see the library report.": "Nur Admins sehen den Bericht über die Mediathek.",
    "The library has not been checked yet.": "Die Mediathek wurde noch nicht geprüft.",
    "Checked {when}: {n} titles, nothing to report.": "Geprüft am {when}: {n} Titel, alles in Ordnung.",
    "Checked {when}: {n} titles.": "Geprüft am {when}: {n} Titel.",
    "Check now": "Jetzt prüfen",
    "Checking the library…": "Prüfe die Bibliothek…",
    "Checked {n} titles: {m} to look at.": "{n} Titel geprüft: {m} zum Ansehen.",
    "Checked {n} titles: nothing to report.": "{n} Titel geprüft: nichts zu melden.",
    "Show": "Anzeigen",
    "Back": "Zurück",
    "Ignore": "Ignorieren",
    "Show ignored again ({n})": "Ignorierte wieder zeigen ({n})",
    "Show all ignored entries again?": "Alle ignorierten Einträge wieder zeigen?",
    "Ignored.": "Ignoriert.",
    "Checked: {n} entries.": "Geprüft: {n} Einträge.",
    "{n} found": "{n} gefunden",
    "and {n} more": "und {n} weitere",
    "Nothing left here.": "Hier ist nichts mehr.",
    "The library could not be read: {e}": "Die Mediathek konnte nicht gelesen werden: {e}",
    "This plugin needs a server that lets plugins read the library.": "Dieses Plugin braucht einen Server, der Plugins die Mediathek lesen lässt.",
    "No file has stream details. Is ffprobe set up (Settings → Transcoder)?": "Keine Datei hat Angaben zu ihren Spuren. Ist ffprobe eingerichtet (Einstellungen → Transcoder)?",
    # checks
    "Twice in the library": "Doppelt in der Mediathek",
    "Low resolution": "Niedrige Auflösung",
    "No audio in {langs}": "Kein Ton auf {langs}",
    "Audio language not marked": "Tonsprache nicht angegeben",
    "Streams could not be read": "Spuren nicht lesbar",
    "Movie without a year": "Film ohne Jahr",
    "Episode without season or number": "Folge ohne Staffel oder Nummer",
    "No details found online": "Keine Details online gefunden",
    # details
    "{w}×{h}": "{w}×{h}",
    "subtitles in {langs}": "Untertitel auf {langs}",
    "audio: {langs}": "Ton: {langs}",
    "also: {where}": "auch: {where}",
    "{n} episodes": "{n} Folgen",
}

LANG_NAMES = {
    "de": ("German", "Deutsch"), "en": ("English", "Englisch"), "fr": ("French", "Französisch"),
    "es": ("Spanish", "Spanisch"), "it": ("Italian", "Italienisch"), "ja": ("Japanese", "Japanisch"),
    "nl": ("Dutch", "Niederländisch"), "pl": ("Polish", "Polnisch"), "ru": ("Russian", "Russisch"),
    "pt": ("Portuguese", "Portugiesisch"), "sv": ("Swedish", "Schwedisch"), "da": ("Danish", "Dänisch"),
    "no": ("Norwegian", "Norwegisch"), "fi": ("Finnish", "Finnisch"), "tr": ("Turkish", "Türkisch"),
    "ko": ("Korean", "Koreanisch"), "zh": ("Chinese", "Chinesisch"), "cs": ("Czech", "Tschechisch"),
    "hu": ("Hungarian", "Ungarisch"), "el": ("Greek", "Griechisch"), "hi": ("Hindi", "Hindi"),
}

# Two- and three-letter codes and names, all to the two-letter code.
LANG_CODES = {
    "ger": "de", "deu": "de", "german": "de", "deutsch": "de",
    "eng": "en", "english": "en", "fre": "fr", "fra": "fr", "french": "fr",
    "spa": "es", "spanish": "es", "ita": "it", "italian": "it", "jpn": "ja", "japanese": "ja",
    "dut": "nl", "nld": "nl", "dutch": "nl", "pol": "pl", "polish": "pl", "rus": "ru", "russian": "ru",
    "por": "pt", "portuguese": "pt", "swe": "sv", "swedish": "sv", "dan": "da", "danish": "da",
    "nor": "no", "nob": "no", "nno": "no", "nb": "no", "nn": "no", "norwegian": "no",
    "fin": "fi", "finnish": "fi", "tur": "tr", "turkish": "tr", "kor": "ko", "korean": "ko",
    "chi": "zh", "zho": "zh", "chinese": "zh", "cze": "cs", "ces": "cs", "czech": "cs",
    "hun": "hu", "hungarian": "hu", "gre": "el", "ell": "el", "greek": "el", "hin": "hi", "hindi": "hi",
}
UNKNOWN = {"", "und", "unk", "unknown", "mis", "zxx", "mul"}


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def lang_code(value):
    """A language as its two-letter code ("ger" → "de"), "" if unknown."""
    v = str(value or "").strip().lower()
    if v in UNKNOWN:
        return ""
    v = v.split("-")[0].split("_")[0]
    return LANG_CODES.get(v, v)


def lang_name(code, lang):
    names = LANG_NAMES.get(code)
    if not names:
        return code
    return names[1] if (lang or "").startswith("de") else names[0]


def lang_list(codes, lang):
    names = [lang_name(c, lang) for c in codes]
    if len(names) < 2:
        return "".join(names)
    joiner = " oder " if (lang or "").startswith("de") else " or "
    return ", ".join(names[:-1]) + joiner + names[-1]


def subtitle_languages(path, video_path):
    """The languages a subtitle file's name gives ("Heat.de.forced.srt" → {"de"})."""
    name = os.path.basename(path.replace("\\", "/"))
    stem = os.path.splitext(os.path.basename(video_path.replace("\\", "/")))[0]
    rest = name[len(stem):] if name.lower().startswith(stem.lower()) else name
    found = set()
    for part in re.split(r"[.\s_\-\[\]()]+", rest.lower()):
        code = lang_code(part)
        if part and (part in LANG_CODES or part in LANG_NAMES):
            found.add(code)
    return found


def settings_languages(settings):
    raw = settings.get("languages", ["de"])
    if isinstance(raw, str):
        raw = re.split(r"[,\s]+", raw)
    return [c for c in (lang_code(x) for x in raw or []) if c]


def norm(text):
    text = unicodedata.normalize("NFKD", str(text or "")).encode("ascii", "ignore").decode().lower()
    return re.sub(r"[^a-z0-9]+", " ", text).strip()


# ---------------------------------------------------------------- reading the library

def fetch_library(server):
    """The library listing the server offers this plugin (server.library)."""
    address = (server or {}).get("library")
    if not address:
        raise LookupError("no library address")
    ctx = None
    host = urllib.parse.urlparse(address).hostname
    if address.startswith("https://") and host in ("127.0.0.1", "localhost", "::1"):
        # The server's own certificate may be made for its public name, not for 127.0.0.1.
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    with urllib.request.urlopen(urllib.request.Request(address, headers={"Accept": "application/json"}), timeout=90, context=ctx) as r:
        return json.loads(r.read().decode("utf-8"))


# ---------------------------------------------------------------- the checks

def label(it):
    """How an entry is named: "Heat (1995)" or "Lost – S01E02 · Pilot"."""
    if it.get("type") == "episode":
        show = (it.get("show") or {}).get("title") or "?"
        s, e = it.get("season"), it.get("episode")
        num = ("S%02d" % s if isinstance(s, int) else "S?") + ("E%02d" % e if isinstance(e, int) else "E?")
        title = it.get("title") or ""
        return "%s – %s" % (show, num) + (" · " + title if title and norm(title) != norm(show) else "")
    year = it.get("year")
    return (it.get("title") or "?") + (" (%s)" % year if year else "")


def first_path(it):
    files = it.get("files") or []
    return files[0].get("path", "") if files else ""


def has_ids(ids):
    return any(v not in (None, "", 0, "0") for v in (ids or {}).values())


def check(listing, settings):
    """{check: [entry]} for the library listing. An entry is {"key", "id", "title", "detail", "path"}."""
    langs = settings_languages(settings)
    min_width = int(settings.get("min_width") or 0)
    skip = {norm(x) for x in (settings.get("skip_libraries") or []) if str(x).strip()}
    sections = {s.get("id"): s for s in listing.get("sections") or []}
    skipped = {sid for sid, s in sections.items() if norm(s.get("name")) in skip}
    items = [it for it in listing.get("items") or [] if it.get("section_id") not in skipped]

    # An item and the versions merged into it are one title.
    groups = {}
    for it in items:
        groups.setdefault(it.get("version_of") or it.get("id"), []).append(it)
    found = {c: [] for c in CHECKS}

    def add(c, it, detail="", key=None):
        found[c].append({"key": "%s:%s" % (c, key if key is not None else it.get("id")), "id": it.get("id"),
                         "title": label(it), "detail": detail, "path": first_path(it)})

    streams_known = 0
    mains = [g for gid, g in groups.items() if any(i.get("id") == gid for i in g)]
    for group in mains:
        main = next(i for i in group if not i.get("version_of"))
        files = [f for i in group for f in i.get("files") or []]
        streams = [s for f in files for s in f.get("streams") or []]
        if not files:
            continue
        if not streams:
            add("unreadable", main)
            continue
        streams_known += 1
        video = [s for s in streams if s.get("kind") == "video" and (s.get("width") or 0) > 0]
        if min_width and video:
            best = max(video, key=lambda s: s.get("width") or 0)
            if (best.get("width") or 0) < min_width:
                add("low_resolution", main, "%s×%s" % (best.get("width"), best.get("height") or "?"))
        audio = [s for s in streams if s.get("kind") == "audio"]
        if langs and audio:
            audio_langs = {lang_code(s.get("language")) for s in audio} - {""}
            if not audio_langs & set(langs):
                subs = {lang_code(s.get("language")) for s in streams if s.get("kind") == "subtitle"}
                for f in files:
                    for p in f.get("external_subtitles") or []:
                        subs |= subtitle_languages(p, f.get("path") or "")
                if not audio_langs:
                    add("untagged_audio", main)
                else:
                    add("no_language", main, json.dumps({"audio": sorted(audio_langs), "subs": sorted(subs & set(langs))}))

    # Names the scanner could not read well; a show without details is one entry, not one per episode.
    shows_without = {}
    for group in mains:
        it = next(i for i in group if not i.get("version_of"))
        if it.get("type") == "movie":
            if not it.get("year"):
                add("no_year", it)
            if not has_ids(it.get("ids")):
                add("no_details", it)
        elif it.get("type") == "episode":
            if not isinstance(it.get("season"), int) or not isinstance(it.get("episode"), int):
                add("no_numbers", it)
            show = it.get("show") or {}
            if show.get("id") and not has_ids(show.get("ids")):
                shows_without.setdefault(show["id"], []).append(it)
    for sid, eps in shows_without.items():
        show = eps[0].get("show") or {}
        found["no_details"].append({"key": "no_details:show:%s" % sid, "id": sid,
                                    "title": (show.get("title") or "?") + (" (%s)" % show["year"] if show.get("year") else ""),
                                    "detail": json.dumps({"episodes": len(eps)}), "path": os.path.dirname(first_path(eps[0]))})

    # The same title twice: by an id the details gave, else by title and year (movies),
    # or by show, season and episode (episodes). Versions do not count.
    seen = {}
    for group in mains:
        it = next(i for i in group if not i.get("version_of"))
        keys = []
        if it.get("type") == "movie":
            for k, v in sorted((it.get("ids") or {}).items()):
                if v not in (None, "", 0, "0"):
                    keys.append("id:%s:%s" % (k, str(v).lower()))
            if not keys and it.get("year"):
                keys.append("name:%s:%s" % (norm(it.get("title")), it.get("year")))
        elif it.get("type") == "episode" and isinstance(it.get("season"), int) and isinstance(it.get("episode"), int):
            sid = (it.get("show") or {}).get("id")
            if sid:
                keys.append("ep:%s:%s:%s" % (sid, it["season"], it["episode"]))
        for k in keys:
            seen.setdefault(k, [])
            if it not in seen[k]:
                seen[k].append(it)
    reported = set()
    for k, same in seen.items():
        if len(same) < 2:
            continue
        ids = tuple(sorted(i.get("id") for i in same))
        if ids in reported:
            continue
        reported.add(ids)
        first = same[0]
        others = [first_path(i) for i in same[1:]]
        add("duplicate", first, json.dumps({"also": others}), key="-".join(str(i) for i in ids))

    # Without any details at all, the online lookup is switched off: not worth a list.
    if found["no_details"] and not any(has_ids(i.get("ids")) or has_ids((i.get("show") or {}).get("ids")) for i in items):
        found["no_details"] = []
    hint = bool(mains) and streams_known == 0 and len(found["unreadable"]) > 0
    if hint:
        found["unreadable"] = []
    return found, len(mains), hint


def run_check(server, settings):
    listing = fetch_library(server)
    found, total, hint = check(listing, settings)
    report = {"at": int(time.time()), "total": total, "hint": hint, "found": found}
    write_json(REPORT, report)
    return report


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
    """One change to the ignored list at a time (two clicks at once must not lose one)."""
    for _ in range(100):
        try:
            os.mkdir(LOCK)
            break
        except FileExistsError:
            try:
                if time.time() - os.path.getmtime(LOCK) > 60:
                    os.rmdir(LOCK)
                    continue
            except OSError:
                pass
            time.sleep(0.1)
    try:
        yield
    finally:
        with contextlib.suppress(OSError):
            os.rmdir(LOCK)


# ---------------------------------------------------------------- the panel

def when(ts, lang):
    t = time.localtime(ts)
    return time.strftime("%d.%m.%Y, %H:%M" if (lang or "").startswith("de") else "%Y-%m-%d %H:%M", t)


def title_of(c, lang, settings):
    langs = lang_list(settings_languages(settings), lang)
    return {
        "duplicate": tr(lang, "Twice in the library"),
        "low_resolution": tr(lang, "Low resolution"),
        "no_language": tr(lang, "No audio in {langs}", langs=langs),
        "untagged_audio": tr(lang, "Audio language not marked"),
        "unreadable": tr(lang, "Streams could not be read"),
        "no_year": tr(lang, "Movie without a year"),
        "no_numbers": tr(lang, "Episode without season or number"),
        "no_details": tr(lang, "No details found online"),
    }[c]


def visible(report, ignored):
    return {c: [e for e in (report.get("found") or {}).get(c) or [] if e.get("key") not in ignored] for c in CHECKS}


def overview(lang, settings, message=None):
    report = read_json(REPORT, None)
    ignored = set(read_json(IGNORED, []))
    buttons = [{"label": tr(lang, "Check now"), "action": "check", "primary": True, "working": tr(lang, "Checking the library…")}]
    if ignored:
        buttons.append({"label": tr(lang, "Show ignored again ({n})", n=len(ignored)), "action": "unignore",
                        "confirm": tr(lang, "Show all ignored entries again?")})
    if not report:
        return {"title": tr(lang, "Library doctor"), "text": tr(lang, "The library has not been checked yet."), "buttons": buttons}
    shown = visible(report, ignored)
    items = [{"title": title_of(c, lang, settings), "status": tr(lang, "{n} found", n=len(shown[c])),
              "buttons": [{"label": tr(lang, "Show"), "action": "show", "args": {"check": c}}]}
             for c in CHECKS if shown[c]]
    n = report.get("total", 0)
    text = tr(lang, "Checked {when}: {n} titles.", when=when(report.get("at", 0), lang), n=n) if items or report.get("hint") else \
        tr(lang, "Checked {when}: {n} titles, nothing to report.", when=when(report.get("at", 0), lang), n=n)
    if report.get("hint"):
        text += " " + tr(lang, "No file has stream details. Is ffprobe set up (Settings → Transcoder)?")
    if message:
        text = message + " " + text
    return {"title": tr(lang, "Library doctor"), "text": text, "items": items, "buttons": buttons}


def describe(c, e, lang):
    """The second line of an entry: what was found, then where the file is."""
    detail = e.get("detail") or ""
    parts = []
    try:
        d = json.loads(detail) if detail.startswith("{") else None
    except ValueError:
        d = None
    if c == "no_language" and d:
        parts.append(tr(lang, "audio: {langs}", langs=", ".join(lang_name(x, lang) for x in d.get("audio") or [])))
        if d.get("subs"):
            parts.append(tr(lang, "subtitles in {langs}", langs=lang_list(d["subs"], lang)))
    elif c == "duplicate" and d:
        parts.append(tr(lang, "also: {where}", where=" · ".join(d.get("also") or [])))
    elif c == "no_details" and d and d.get("episodes"):
        parts.append(tr(lang, "{n} episodes", n=d["episodes"]))
    elif detail and not d:
        parts.append(detail)
    if e.get("path"):
        parts.append(e["path"])
    return " · ".join(parts)


def show_check(lang, settings, c):
    report = read_json(REPORT, None) or {}
    if c not in CHECKS:
        return overview(lang, settings)
    entries = visible(report, set(read_json(IGNORED, [])))[c]
    if not entries:
        return overview(lang, settings, tr(lang, "Nothing left here."))
    items = []
    for e in entries[:SHOW_AT_MOST]:
        # A show's page for a show, else the title's page.
        href = "#/item/%s" % e["id"] if e.get("id") else None
        items.append({"title": e.get("title") or "?", "subtitle": describe(c, e, lang), "href": href,
                      "buttons": [{"label": tr(lang, "Ignore"), "action": "ignore", "args": {"key": e["key"], "check": c}}]})
    text = tr(lang, "{n} found", n=len(entries))
    if len(entries) > SHOW_AT_MOST:
        text += " · " + tr(lang, "and {n} more", n=len(entries) - SHOW_AT_MOST)
    return {"title": title_of(c, lang, settings), "text": text, "items": items,
            "buttons": [{"label": tr(lang, "Back"), "action": "panel"}]}


def answer_call(data, settings, server):
    body = data.get("body") if isinstance(data.get("body"), dict) else {}
    lang = body.get("lang") or "en"
    args = body.get("args") if isinstance(body.get("args"), dict) else {}
    if not data.get("admin"):
        return {"error": tr(lang, "Only admins can see the library report.")}
    action = data.get("action")
    if action == "panel":
        return overview(lang, settings)
    if action == "check":
        try:
            report = run_check(server, settings)
        except LookupError:
            return {"error": tr(lang, "This plugin needs a server that lets plugins read the library.")}
        except Exception as e:  # noqa: BLE001 — shown to the admin
            return {"error": tr(lang, "The library could not be read: {e}", e=e)}
        n = sum(len(v) for v in visible(report, set(read_json(IGNORED, []))).values())
        done = tr(lang, "Checked {n} titles: {m} to look at.", n=report.get("total", 0), m=n) if n else             tr(lang, "Checked {n} titles: nothing to report.", n=report.get("total", 0))
        return {"message": done, "panel": overview(lang, settings)}
    if action == "show":
        return {"panel": show_check(lang, settings, str(args.get("check") or ""))}
    if action == "ignore":
        key = str(args.get("key") or "")
        if key:
            with locked():
                ignored = read_json(IGNORED, [])
                if key not in ignored:
                    ignored.append(key)
                    write_json(IGNORED, ignored)
        return {"message": tr(lang, "Ignored."), "panel": show_check(lang, settings, str(args.get("check") or ""))}
    if action == "unignore":
        with locked():
            write_json(IGNORED, [])
        return {"panel": overview(lang, settings)}
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
    if event == "library.scan.finished":
        changed = any(data.get(k) for k in ("added", "updated", "removed"))
        if not changed and os.path.exists(REPORT):
            print(json.dumps({"skipped": "nothing changed"}))
            return
    elif event != "schedule.daily":
        print(json.dumps({"skipped": event}))
        return
    report = run_check(server, settings)
    print(json.dumps({"ok": True, "titles": report["total"], "found": {c: len(v) for c, v in report["found"].items() if v}}))


if __name__ == "__main__":
    main()
