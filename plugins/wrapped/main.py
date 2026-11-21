"""Lichtspiel Wrapped: a look back at the year for each person.

The year comes from the server's play history (server.history): every movie and episode watched to
the end, here or brought in from Plex, also from before the plugin was switched on. Titles marked as
watched by hand do not count (a whole show ticked off at once is not a binge). The panel shows the
person's own year: hours watched, movies and episodes, favourite genres, most watched shows, longest
binge, busiest month, and the first and last title; earlier years stay one click away.

A server without that history: playback.watched notes each finished title in state.json in the
plugin's folder, for this year and the three before it, and those notes are used.
"""
import collections
import contextlib
import datetime
import json
import os
import ssl
import sys
import time
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
STATE = os.path.join(HERE, "state.json")
LOCK = os.path.join(HERE, ".state.lock")
KEEP_YEARS = 4

MONTHS = {"en": ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
          "de": ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"]}

WORDS = {
    "Your {year}": "Dein {year}",
    "Your {year}, {name}": "Dein {year}, {name}",
    "Nothing finished in {year} yet. What you watch to the end from now on is counted here.": "In {year} noch nichts fertig geschaut. Was du ab jetzt zu Ende schaust, wird hier gezählt.",
    "Counted since {date}.": "Gezählt seit dem {date}.",
    "Nothing watched to the end in {year} yet.": "In {year} noch nichts zu Ende geschaut.",
    "{h} hours": "{h} Stunden",
    "1 hour": "1 Stunde",
    "{m} minutes": "{m} Minuten",
    "watched": "geschaut",
    "{n} movies": "{n} Filme",
    "1 movie": "1 Film",
    "{n} episodes": "{n} Folgen",
    "1 episode": "1 Folge",
    "of {n} shows": "aus {n} Serien",
    "of 1 show": "aus 1 Serie",
    "Favourite genres": "Lieblingsgenres",
    "Most watched show": "Meistgeschaute Serie",
    "Also a lot": "Auch viel",
    "Longest binge": "Längster Marathon",
    "{n} episodes in a row on {date}": "{n} Folgen am Stück am {date}",
    "{n} episodes in a row from {date}": "{n} Folgen am Stück ab dem {date}",
    "Busiest month": "Stärkster Monat",
    "Favourite day": "Lieblingstag",
    "First of the year": "Der erste im Jahr",
    "Latest": "Zuletzt",
    "Back to {year}": "Zurück zu {year}",
    "Mondays": "montags", "Tuesdays": "dienstags", "Wednesdays": "mittwochs", "Thursdays": "donnerstags",
    "Fridays": "freitags", "Saturdays": "samstags", "Sundays": "sonntags",
    "{n} titles": "{n} Titel",
}
DAYS = ["Mondays", "Tuesdays", "Wednesdays", "Thursdays", "Fridays", "Saturdays", "Sundays"]


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def de(lang):
    return (lang or "").startswith("de")


def count(lang, n, one, many):
    return tr(lang, one) if n == 1 else tr(lang, many, n=n)


# ---------- what it keeps (several runs may happen at once, so with a lock)

@contextlib.contextmanager
def locked():
    end = time.time() + 10
    while True:
        try:
            os.mkdir(LOCK)
            break
        except FileExistsError:
            try:  # left behind by a run that was stopped
                if time.time() - os.path.getmtime(LOCK) > 30:
                    os.rmdir(LOCK)
                    continue
            except OSError:
                pass
            if time.time() > end:
                raise RuntimeError("busy")
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
        json.dump(state, f, ensure_ascii=False, separators=(",", ":"))
    os.replace(tmp, STATE)


# ---------- noting what was finished

def entry_of(item, at):
    """What is kept of a finished movie or episode; None for anything else."""
    if item.get("type") not in ("movie", "episode"):
        return None
    entry = {"at": int(at), "id": item.get("id"), "type": item["type"], "title": item.get("title") or "?",
             "year": item.get("year"), "min": round((item.get("duration_ms") or 0) / 60000), "genres": item.get("genres") or []}
    if item["type"] == "episode":
        show = item.get("show") or {}
        entry.update(show=show.get("title") or "?", show_id=show.get("id"), season=item.get("season"), episode=item.get("episode"))
        # An episode's genres are the show's (episodes have none of their own).
        entry["genres"] = entry["genres"] or show.get("genres") or []
    return entry


def note(data, now=None):
    item = data.get("item") or {}
    user = data.get("user")
    now = int(now or time.time())
    entry = entry_of(item, now)
    if data.get("manual") or not user or not entry:
        return False
    cut = datetime.date.fromtimestamp(now).year - KEEP_YEARS + 1
    with locked():
        state = load()
        st = state.setdefault("people", {}).setdefault(user, {"since": now, "seen": []})
        seen = [e for e in st["seen"] if datetime.date.fromtimestamp(e["at"]).year >= cut]
        # The same title finished twice within a few minutes (two players reporting it) counts once.
        if not any(e.get("id") == entry["id"] and now - e["at"] < 600 for e in seen[-5:]):
            seen.append(entry)
        st["seen"] = seen
        save(state)
    return True


# ---------- the server's play history

def server_seen(server, user):
    """Everything the person watched to the end in the years kept, from the server; None when it cannot say."""
    address = (server or {}).get("history")
    if not address or not user:
        return None
    since = int(time.mktime(datetime.date(datetime.date.today().year - KEEP_YEARS + 1, 1, 1).timetuple()))
    url = address + ("&" if "?" in address else "?") + urllib.parse.urlencode({"user": user, "since": since})
    ctx = None
    if url.startswith("https://") and urllib.parse.urlparse(url).hostname in ("127.0.0.1", "localhost", "::1"):
        # The server's own certificate may be made for its public name, not for 127.0.0.1.
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers={"Accept": "application/json"}), timeout=15, context=ctx) as r:
            plays = json.loads(r.read().decode("utf-8")).get("plays") or []
    except (OSError, ValueError, AttributeError):
        return None
    seen = []
    for p in sorted(plays, key=lambda p: p.get("at") or 0):
        e = None if p.get("manual") or not p.get("at") else entry_of(p.get("item") or {}, p["at"])
        # The same title finished twice within a few minutes (two players reporting it) counts once.
        if e and not any(x["id"] == e["id"] and e["at"] - x["at"] < 600 for x in seen[-5:]):
            seen.append(e)
    return seen


# ---------- the year

def year_of(e):
    return datetime.date.fromtimestamp(e["at"]).year


def day_text(lang, ts):
    d = datetime.date.fromtimestamp(ts)
    return ("%d. %s" % (d.day, MONTHS["de"][d.month - 1])) if de(lang) else ("%d %s" % (d.day, MONTHS["en"][d.month - 1]))


def hours_text(lang, minutes):
    if minutes < 60:
        return tr(lang, "{m} minutes", m=minutes)
    h = int(minutes / 60 + 0.5)  # 4.5 hours is 5, not 4
    return tr(lang, "1 hour") if h == 1 else tr(lang, "{h} hours", h=h)


def longest_binge(seen, gap_hours):
    """(episodes, show, show id, start, end) of the longest run of one show's episodes, each finished within gap_hours of the last."""
    best = (0, "", None, 0, 0)
    run = []
    for e in sorted((e for e in seen if e["type"] == "episode"), key=lambda e: e["at"]):
        if run and (e.get("show") != run[-1].get("show") or e["at"] - run[-1]["at"] > gap_hours * 3600):
            run = []
        run.append(e)
        if len(run) > best[0]:
            best = (len(run), e.get("show"), e.get("show_id"), run[0]["at"], e["at"])
    return best


def summary(seen, gap_hours=3):
    """The year's numbers from its notes."""
    movies = [e for e in seen if e["type"] == "movie"]
    episodes = [e for e in seen if e["type"] == "episode"]
    shows = collections.Counter(e.get("show") for e in episodes)
    show_ids = {e.get("show"): e.get("show_id") for e in episodes}
    genres = collections.Counter(g for e in seen for g in (e.get("genres") or []))
    months = collections.Counter()
    days = collections.Counter()
    for e in seen:
        d = datetime.date.fromtimestamp(e["at"])
        months[d.month] += e.get("min") or 0
        days[d.weekday()] += 1
    first = min(seen, key=lambda e: e["at"]) if seen else None
    last = max(seen, key=lambda e: e["at"]) if seen else None
    return {"minutes": sum(e.get("min") or 0 for e in seen), "movies": len(movies), "episodes": len(episodes),
            "shows": [(s, n, show_ids.get(s)) for s, n in shows.most_common(5)], "show_count": len(shows),
            "genres": [g for g, _ in genres.most_common(3)], "binge": longest_binge(seen, gap_hours),
            "month": months.most_common(1)[0] if months and months.most_common(1)[0][1] else None,
            "day": days.most_common(1)[0] if days else None, "first": first, "last": last}


def label(e):
    if e["type"] == "episode":
        return "%s · S%02dE%02d" % (e.get("show") or "?", e.get("season") or 0, e.get("episode") or 0)
    return (e.get("title") or "?") + (" (%s)" % e["year"] if e.get("year") else "")


def thumb(ident):
    return "/library/metadata/%s/thumb" % ident if ident else ""


def first_name(data):
    """The first word of the person's full name; empty when they gave none (user_name is then the user name)."""
    shown = str(data.get("user_name") or "").strip()
    return shown.split()[0] if shown and shown != data.get("user") else ""


def href(ident):
    return "#/item/%s" % ident if ident else ""


def panel(lang, s, user, year=None, first="", server=None):
    from_server = server_seen(server, user)
    st = {"seen": from_server} if from_server is not None else (load().get("people") or {}).get(user) or {"seen": []}
    this_year = datetime.date.today().year
    year = int(year or this_year)
    seen = [e for e in st.get("seen") or [] if year_of(e) == year]
    years = sorted({year_of(e) for e in st.get("seen") or []} | {this_year}, reverse=True)
    out = {"title": tr(lang, "Your {year}, {name}", year=year, name=first) if first else tr(lang, "Your {year}", year=year)}
    buttons = [{"label": str(y), "action": "year", "args": {"year": y}, "primary": y == year} for y in years if len(years) > 1]
    if buttons:
        out["buttons"] = buttons
    if not seen:
        out["text"] = tr(lang, "Nothing watched to the end in {year} yet.", year=year) if from_server is not None else \
            tr(lang, "Nothing finished in {year} yet. What you watch to the end from now on is counted here.", year=year)
        return out
    sm = summary(seen, float(s.get("binge_gap_hours") or 3))
    if st.get("since") and year_of({"at": st["since"]}) == year:
        out["text"] = tr(lang, "Counted since {date}.", date=day_text(lang, st["since"]))
    parts = []
    if sm["movies"]:
        parts.append(count(lang, sm["movies"], "1 movie", "{n} movies"))
    if sm["episodes"]:
        parts.append(count(lang, sm["episodes"], "1 episode", "{n} episodes") + " " + count(lang, sm["show_count"], "of 1 show", "of {n} shows"))
    items = [{"title": hours_text(lang, sm["minutes"]), "subtitle": tr(lang, "watched") + " · " + ", ".join(parts)}]
    if sm["genres"]:
        items.append({"title": tr(lang, "Favourite genres"), "subtitle": ", ".join(sm["genres"])})
    if sm["shows"]:
        top, n, sid = sm["shows"][0]
        items.append({"title": tr(lang, "Most watched show"), "subtitle": "%s · %s" % (top, count(lang, n, "1 episode", "{n} episodes")),
                      "image": thumb(sid), "href": href(sid)})
        if len(sm["shows"]) > 1:
            items.append({"title": tr(lang, "Also a lot"), "subtitle": ", ".join("%s (%d)" % (t, k) for t, k, _ in sm["shows"][1:])})
    n, show, sid, start, end = sm["binge"]
    if n >= 2:
        same_day = datetime.date.fromtimestamp(start) == datetime.date.fromtimestamp(end)
        what = "{n} episodes in a row on {date}" if same_day else "{n} episodes in a row from {date}"
        items.append({"title": tr(lang, "Longest binge"), "subtitle": "%s · %s" % (show, tr(lang, what, n=n, date=day_text(lang, start))),
                      "image": thumb(sid), "href": href(sid)})
    if sm["month"]:
        month, minutes = sm["month"]
        items.append({"title": tr(lang, "Busiest month"), "subtitle": "%s · %s" % (MONTHS["de" if de(lang) else "en"][month - 1], hours_text(lang, minutes))})
    if sm["day"] and len(seen) >= 5:
        day, k = sm["day"]
        items.append({"title": tr(lang, "Favourite day"), "subtitle": "%s · %s" % (tr(lang, DAYS[day]), tr(lang, "{n} titles", n=k))})
    for key, name in (("first", "First of the year"), ("last", "Latest")):
        e = sm[key]
        if e and (key == "first" or sm["first"] is not sm["last"]):
            ident = e.get("show_id") if e["type"] == "episode" else e.get("id")
            items.append({"title": tr(lang, name), "subtitle": "%s · %s" % (label(e), day_text(lang, e["at"])), "image": thumb(ident), "href": href(e.get("id"))})
    # The picture column only when some entry has a picture.
    if not any(i.get("image") for i in items):
        for i in items:
            i.pop("image", None)
    out["items"] = items
    return out


def main():
    # The server sends UTF-8, whatever the console code page is (Windows).
    msg = json.loads(getattr(sys.stdin, "buffer", sys.stdin).read())
    event = msg.get("event", "")
    data = msg.get("data") or {}
    s = msg.get("settings") or {}
    server = msg.get("server") or {}
    if event == "playback.watched":
        out = {"ok": True, "noted": note(data)}
    elif event == "api.call":
        body = data.get("body") if isinstance(data.get("body"), dict) else {}
        args = body.get("args") if isinstance(body.get("args"), dict) else {}
        lang, user = body.get("lang") or "en", data.get("user") or ""
        first = first_name(data)
        if data.get("action") == "panel":
            out = panel(lang, s, user, first=first, server=server)
        elif data.get("action") == "year" and isinstance(args.get("year"), int):
            out = {"panel": panel(lang, s, user, args["year"], first, server)}
        else:
            out = {"error": "Unknown action"}
    else:
        out = {"skipped": event}
    # Escaped (ü), so the answer survives any console code page (Windows).
    print(json.dumps(out))


if __name__ == "__main__":
    main()
