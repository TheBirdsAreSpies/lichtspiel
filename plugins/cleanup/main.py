"""Clean-up suggestions plugin.

Once a day and when an admin asks, looks through the whole library with who watched what and
lists, for admins under Settings, what could go when the disk fills up:

- titles nobody watched for a long time (default 12 months) that have been there at least as long,
- titles everyone has seen (the people named in the settings, or everyone) and nobody played in
  the last 30 days,
- the biggest titles for their running time (gigabytes per hour).

A show counts as one title (all its episodes together). Nothing is deleted by itself: each entry
has a "Delete" button, shown only when the server allows media deletion and the admin may delete,
and the server deletes with the admin's own right. Entries can be kept; they stay off the lists
until the kept list is cleared. The report is kept in report.json, the kept entries in kept.json,
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
KEPT = os.path.join(HERE, "kept.json")
LOCK = os.path.join(HERE, ".cleanup.lock")
SHOW_AT_MOST = 100
BIGGEST = 50
DAY = 86400
MONTH = 30.44 * DAY
# "Everyone has seen it" only once nobody played it for this long.
SEEN_REST_DAYS = 30

CHECKS = ["unwatched", "everyone_seen", "biggest"]

WORDS = {
    "Clean-up suggestions": "Aufräum-Vorschläge",
    "Only admins can see the clean-up suggestions.": "Nur Admins sehen die Aufräum-Vorschläge.",
    "The library has not been looked through yet.": "Die Mediathek wurde noch nicht durchgesehen.",
    "Looked through {when}: {n} titles.": "Durchgesehen am {when}: {n} Titel.",
    "Deleting from here needs \"Allow media deletion\" in the settings and the right to delete.":
        "Löschen von hier aus braucht „Löschen von Medien erlauben“ in den Einstellungen und das Recht zu löschen.",
    "Look through now": "Jetzt durchsehen",
    "Show": "Anzeigen",
    "Back": "Zurück",
    "Keep": "Behalten",
    "Delete": "Löschen",
    "Delete the files of {title} from disk ({size})? This cannot be undone.":
        "Die Dateien von {title} von der Festplatte löschen ({size})? Das lässt sich nicht rückgängig machen.",
    "Deleted: {title}": "Gelöscht: {title}",
    "Kept.": "Behalten.",
    "You may not delete files.": "Du darfst keine Dateien löschen.",
    "Show kept again ({n})": "Behaltene wieder zeigen ({n})",
    "Show all kept titles again?": "Alle behaltenen Titel wieder zeigen?",
    "{n} titles · {size}": "{n} Titel · {size}",
    "nothing": "nichts",
    "and {n} more": "und {n} weitere",
    "Nothing left here.": "Hier ist nichts mehr.",
    "The library could not be read: {e}": "Die Mediathek konnte nicht gelesen werden: {e}",
    "This plugin needs a server that lets plugins read the library.": "Dieses Plugin braucht einen Server, der Plugins die Mediathek lesen lässt.",
    # checks
    "Not watched for {n} months": "Seit {n} Monaten nicht gesehen",
    "Everyone has seen it": "Alle haben es gesehen",
    "Biggest for their running time": "Am größten für ihre Laufzeit",
    # details
    "never watched": "nie gesehen",
    "last watched {date}": "zuletzt gesehen am {date}",
    "added {date}": "hinzugefügt am {date}",
    "{rate} per hour": "{rate} pro Stunde",
    "{n} episodes": "{n} Folgen",
}


def de(lang):
    return (lang or "").startswith("de")


def tr(lang, text, **kw):
    if de(lang):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def norm(text):
    text = unicodedata.normalize("NFKD", str(text or "")).encode("ascii", "ignore").decode().lower()
    return re.sub(r"[^a-z0-9]+", " ", text).strip()


def size_text(n, lang):
    """1500000000 → "1.5 GB" ("1,5 GB" in German)."""
    n = float(n or 0)
    for unit, scale in (("TB", 1e12), ("GB", 1e9), ("MB", 1e6)):
        if n >= scale:
            text = "%.1f %s" % (n / scale, unit)
            break
    else:
        text = "%.1f MB" % (n / 1e6) if n else "0 MB"
    return text.replace(".", ",") if de(lang) else text


def date_text(ts, lang):
    return time.strftime("%d.%m.%Y" if de(lang) else "%Y-%m-%d", time.localtime(ts))


def when(ts, lang):
    return time.strftime("%d.%m.%Y, %H:%M" if de(lang) else "%Y-%m-%d %H:%M", time.localtime(ts))


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

def label(title, year):
    return (title or "?") + (" (%s)" % year if year else "")


def titles(listing, settings):
    """Movies and shows (a show's episodes taken together), each with its size, running time and who watched it."""
    skip = {norm(x) for x in (settings.get("skip_libraries") or []) if str(x).strip()}
    skipped = {s.get("id") for s in listing.get("sections") or [] if norm(s.get("name")) in skip}
    out = {}
    for it in listing.get("items") or []:
        # A version merged into another title is part of it, not a title of its own.
        if it.get("version_of") or it.get("section_id") in skipped:
            continue
        show = it.get("show") if it.get("type") == "episode" else None
        if show and show.get("id"):
            key, tid, name = "show:%s" % show["id"], show["id"], label(show.get("title"), show.get("year"))
        else:
            key, tid, name = "movie:%s" % it.get("id"), it.get("id"), label(it.get("title"), it.get("year"))
        files = it.get("files") or []
        t = out.get(key)
        if t is None:
            t = out[key] = {"id": tid, "title": name, "show": bool(show), "size": 0, "duration_ms": 0, "added_at": 0,
                            "last_played": 0, "in_progress": False, "watched": None, "episodes": 0,
                            "path": files[0].get("path", "") if files and not show else ""}
        t["size"] += sum(int(f.get("size") or 0) for f in files)
        t["duration_ms"] += int(it.get("duration_ms") or 0) or max((int(f.get("duration_ms") or 0) for f in files), default=0)
        t["added_at"] = max(t["added_at"], int(it.get("added_at") or 0))
        t["last_played"] = max(t["last_played"], int(it.get("last_played") or 0))
        t["in_progress"] = t["in_progress"] or bool(it.get("in_progress_by"))
        # Seen by a person = every episode seen by them.
        seen = set(it.get("watched_by") or [])
        t["watched"] = seen if t["watched"] is None else t["watched"] & seen
        t["episodes"] += 1 if show else 0
    return list(out.values())


def entry(t, **extra):
    e = {"key": "%s:%s" % ("show" if t["show"] else "movie", t["id"]), "id": t["id"], "title": t["title"], "size": t["size"],
         "added_at": t["added_at"], "last_played": t["last_played"], "episodes": t["episodes"], "path": t["path"]}
    e.update(extra)
    return e


def check(listing, settings, now=None):
    """{check: [entry]} and the number of titles looked at."""
    now = now or time.time()
    months = max(1, int(settings.get("unwatched_months") or 12))
    cutoff = now - months * MONTH
    people = [p for p in (settings.get("people") or []) if str(p).strip()] or list(listing.get("users") or [])
    all_titles = titles(listing, settings)
    found = {c: [] for c in CHECKS}
    for t in all_titles:
        # Started long ago and left counts as not watched; someone watching it again keeps it off "everyone has seen it".
        if 0 < t["added_at"] <= cutoff and t["last_played"] <= cutoff:
            found["unwatched"].append(entry(t))
        if people and not t["in_progress"] and set(people) <= (t["watched"] or set()) and t["last_played"] <= now - SEEN_REST_DAYS * DAY:
            found["everyone_seen"].append(entry(t))
    for c in ("unwatched", "everyone_seen"):
        found[c].sort(key=lambda e: -e["size"])
    rated = [(t["size"] / 1e9 / (t["duration_ms"] / 3.6e6), t) for t in all_titles if t["size"] > 0 and t["duration_ms"] >= 60_000]
    rated.sort(key=lambda x: -x[0])
    found["biggest"] = [entry(t, rate=round(r * 1e9)) for r, t in rated[:BIGGEST]]
    return found, len(all_titles)


def run_check(server, settings):
    listing = fetch_library(server)
    found, total = check(listing, settings)
    report = {"at": int(time.time()), "total": total, "found": found}
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
    """One change to the report or the kept list at a time (two clicks at once must not lose one)."""
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

def title_of(c, lang, settings):
    return {
        "unwatched": tr(lang, "Not watched for {n} months", n=max(1, int(settings.get("unwatched_months") or 12))),
        "everyone_seen": tr(lang, "Everyone has seen it"),
        "biggest": tr(lang, "Biggest for their running time"),
    }.get(c, c)


def visible(report, kept):
    found = report.get("found") or {}
    return {c: [e for e in found.get(c) or [] if e["key"] not in kept] for c in CHECKS}


def overview(lang, settings, may_delete, message=None):
    report = read_json(REPORT, None)
    kept = read_json(KEPT, [])
    buttons = [{"label": tr(lang, "Look through now"), "action": "check", "primary": True}]
    if kept:
        buttons.append({"label": tr(lang, "Show kept again ({n})", n=len(kept)), "action": "unkeep",
                        "confirm": tr(lang, "Show all kept titles again?")})
    if not report:
        return {"title": tr(lang, "Clean-up suggestions"), "text": tr(lang, "The library has not been looked through yet."), "buttons": buttons}
    shown = visible(report, set(kept))
    items = []
    for c in CHECKS:
        entries = shown[c]
        status = tr(lang, "{n} titles · {size}", n=len(entries), size=size_text(sum(e["size"] for e in entries), lang)) if entries else tr(lang, "nothing")
        items.append({"title": title_of(c, lang, settings), "status": status,
                      "buttons": [{"label": tr(lang, "Show"), "action": "show", "args": {"check": c}}] if entries else []})
    text = tr(lang, "Looked through {when}: {n} titles.", when=when(report.get("at", 0), lang), n=report.get("total", 0))
    if not may_delete:
        text += " " + tr(lang, "Deleting from here needs \"Allow media deletion\" in the settings and the right to delete.")
    if message:
        text = message + " " + text
    return {"title": tr(lang, "Clean-up suggestions"), "text": text, "items": items, "buttons": buttons}


def describe(c, e, lang):
    parts = []
    if c == "biggest" and e.get("rate"):
        parts.append(tr(lang, "{rate} per hour", rate=size_text(e["rate"], lang)))
    parts.append(size_text(e.get("size"), lang))
    if c != "biggest":
        parts.append(tr(lang, "last watched {date}", date=date_text(e["last_played"], lang)) if e.get("last_played") else tr(lang, "never watched"))
        if e.get("added_at"):
            parts.append(tr(lang, "added {date}", date=date_text(e["added_at"], lang)))
    if e.get("episodes"):
        parts.append(tr(lang, "{n} episodes", n=e["episodes"]))
    if e.get("path"):
        parts.append(e["path"])
    return " · ".join(parts)


def show_check(lang, settings, may_delete, c):
    report = read_json(REPORT, None) or {}
    if c not in CHECKS:
        return overview(lang, settings, may_delete)
    entries = visible(report, set(read_json(KEPT, [])))[c]
    if not entries:
        return overview(lang, settings, may_delete, tr(lang, "Nothing left here."))
    items = []
    for e in entries[:SHOW_AT_MOST]:
        buttons = [{"label": tr(lang, "Keep"), "action": "keep", "args": {"key": e["key"], "check": c}}]
        if may_delete:
            buttons.append({"label": tr(lang, "Delete"), "action": "delete", "danger": True, "args": {"id": e["id"], "check": c},
                            "confirm": tr(lang, "Delete the files of {title} from disk ({size})? This cannot be undone.",
                                          title=e["title"], size=size_text(e["size"], lang))})
        items.append({"title": e.get("title") or "?", "subtitle": describe(c, e, lang), "href": "#/item/%s" % e["id"], "buttons": buttons})
    text = tr(lang, "{n} titles · {size}", n=len(entries), size=size_text(sum(e["size"] for e in entries), lang))
    if len(entries) > SHOW_AT_MOST:
        text += " · " + tr(lang, "and {n} more", n=len(entries) - SHOW_AT_MOST)
    return {"title": title_of(c, lang, settings), "text": text, "items": items,
            "buttons": [{"label": tr(lang, "Back"), "action": "panel"}]}


def answer_call(data, settings, server):
    body = data.get("body") if isinstance(data.get("body"), dict) else {}
    lang = body.get("lang") or "en"
    args = body.get("args") if isinstance(body.get("args"), dict) else {}
    if not data.get("admin"):
        return {"error": tr(lang, "Only admins can see the clean-up suggestions.")}
    may_delete = bool(data.get("may_delete"))
    action = data.get("action")
    c = str(args.get("check") or "")
    if action == "panel":
        return overview(lang, settings, may_delete)
    if action == "check":
        try:
            run_check(server, settings)
        except LookupError:
            return {"error": tr(lang, "This plugin needs a server that lets plugins read the library.")}
        except Exception as e:  # noqa: BLE001 — shown to the admin
            return {"error": tr(lang, "The library could not be read: {e}", e=e)}
        return {"panel": overview(lang, settings, may_delete)}
    if action == "show":
        return {"panel": show_check(lang, settings, may_delete, c)}
    if action == "keep":
        key = str(args.get("key") or "")
        if key:
            with locked():
                kept = read_json(KEPT, [])
                if key not in kept:
                    kept.append(key)
                    write_json(KEPT, kept)
        return {"message": tr(lang, "Kept."), "panel": show_check(lang, settings, may_delete, c)}
    if action == "unkeep":
        with locked():
            write_json(KEPT, [])
        return {"panel": overview(lang, settings, may_delete)}
    if action == "delete":
        if not may_delete:
            return {"error": tr(lang, "You may not delete files.")}
        tid = args.get("id")
        if not isinstance(tid, int):
            return {"error": "no id"}
        # Off every list now; the server deletes it (and says so if it could not).
        # Should a file stay, the next look through finds the title again.
        with locked():
            report = read_json(REPORT, None) or {}
            found = report.get("found") or {}
            title = next((e["title"] for v in found.values() for e in v if e["id"] == tid), "?")
            report["found"] = {k: [e for e in v if e["id"] != tid] for k, v in found.items()}
            write_json(REPORT, report)
        return {"delete_media": [tid], "message": tr(lang, "Deleted: {title}", title=title),
                "panel": show_check(lang, settings, may_delete, c)}
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
    report = run_check(server, settings)
    print(json.dumps({"ok": True, "titles": report["total"], "found": {c: len(v) for c, v in report["found"].items() if v}}))


if __name__ == "__main__":
    main()
