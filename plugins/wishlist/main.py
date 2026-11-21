"""Wish list plugin.

Everyone gets a panel under Settings: search for a movie (Radarr) or a show (Sonarr), wish for it,
and see where the wishes stand (wished, downloading, downloaded, here). A wish goes straight to
Radarr / Sonarr, which look for it. When the title arrives in the library (library.item.added) the
wish is marked as here; wishes that are here leave the list after 30 days (schedule.daily).

The wishes are kept in wishes.json in the plugin's folder.
"""
import contextlib
import json
import os
import re
import sys
import time
import unicodedata
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
STATE = os.path.join(HERE, "wishes.json")
LOCK = os.path.join(HERE, ".wishes.lock")
TIMEOUT = 15
KEEP_DAYS = 30

WORDS = {
    "Wish list": "Wünsche",
    "Search for a movie or show you would like to see here. Radarr and Sonarr look for it.": "Such nach einem Film oder einer Serie, die du hier gern sehen würdest. Radarr und Sonarr suchen danach.",
    "Search for a movie you would like to see here. Radarr looks for it.": "Such nach einem Film, den du hier gern sehen würdest. Radarr sucht danach.",
    "Search for a show you would like to see here. Sonarr looks for it.": "Such nach einer Serie, die du hier gern sehen würdest. Sonarr sucht danach.",
    "The wish list is not set up yet: an admin enters the address and API key of Radarr and/or Sonarr in the plugin's settings.": "Die Wunschliste ist noch nicht eingerichtet: Ein Admin trägt Adresse und API-Key von Radarr und/oder Sonarr in den Einstellungen des Plugins ein.",
    "Only admins can wish for titles here.": "Hier dürfen nur Admins Titel wünschen.",
    "Movie or show…": "Film oder Serie …",
    "Movie…": "Film …",
    "Show…": "Serie …",
    "Search": "Suchen",
    "Results for “{q}”": "Ergebnisse für „{q}“",
    "Nothing found for “{q}”.": "Nichts gefunden für „{q}“.",
    "Back to my wishes": "Zurück zu meinen Wünschen",
    "Wish for it": "Wünschen",
    "Wish too": "Auch wünschen",
    "Withdraw": "Zurückziehen",
    "Remove": "Entfernen",
    "Remove this wish for everyone?": "Diesen Wunsch für alle entfernen?",
    "Movie": "Film",
    "Show": "Serie",
    "wished": "gewünscht",
    "downloading {p} %": "wird geladen, {p} %",
    "downloading": "wird geladen",
    "downloaded": "heruntergeladen",
    "here": "da",
    "failed": "fehlgeschlagen",
    "already here": "schon da",
    "wished by you": "von dir gewünscht",
    "{n} of {m} episodes": "{n} von {m} Folgen",
    "wished by {who}": "gewünscht von {who}",
    "You have no wishes yet.": "Du hast noch keine Wünsche.",
    "All wishes": "Alle Wünsche",
    "Wished. {app} is looking for it.": "Gewünscht. {app} sucht danach.",
    "You wished for this already.": "Das hast du dir schon gewünscht.",
    "You have {n} open wishes, the most allowed. Withdraw one first.": "Du hast {n} offene Wünsche, mehr geht nicht. Zieh erst einen zurück.",
    "Withdrawn.": "Zurückgezogen.",
    "Removed.": "Entfernt.",
    "No such wish.": "Diesen Wunsch gibt es nicht.",
    "{app} could not be reached: {e}": "{app} ist nicht erreichbar: {e}",
}


class ArrError(Exception):
    pass


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def log(text):
    print(text, file=sys.stderr)


# ---------- the wishes, kept in a file (several runs may happen at once, so with a lock)

@contextlib.contextmanager
def locked():
    end = time.time() + 10
    while True:
        try:
            os.mkdir(LOCK)
            break
        except FileExistsError:
            try:  # left behind by a run that was stopped
                if time.time() - os.path.getmtime(LOCK) > 60:
                    os.rmdir(LOCK)
                    continue
            except OSError:
                pass
            if time.time() > end:
                raise ArrError("The wish list is busy, try again in a moment")
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
        return data.get("wishes") or []
    except (OSError, ValueError):
        return []


def save(wishes):
    tmp = STATE + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump({"wishes": wishes}, f, ensure_ascii=False, indent=1)
    os.replace(tmp, STATE)


def norm(title):
    """A title for comparing: lower case, no accents, no punctuation, no leading article."""
    t = unicodedata.normalize("NFKD", str(title or "")).encode("ascii", "ignore").decode().lower()
    t = re.sub(r"[^a-z0-9]+", " ", t).strip()
    return re.sub(r"^(the|a|an|der|die|das) ", "", t)


# ---------- Radarr and Sonarr

APPS = {"movie": ("radarr", "Radarr"), "show": ("sonarr", "Sonarr")}


def configured(settings, kind):
    pre = APPS[kind][0]
    return bool((settings.get(pre + "_url") or "").strip() and (settings.get(pre + "_api_key") or "").strip())


def arr(settings, kind, method, path, params=None, body=None):
    pre, name = APPS[kind]
    base = settings[pre + "_url"].strip().rstrip("/")
    if urllib.parse.urlsplit(base).scheme not in ("http", "https"):
        raise ArrError("%s: the address must start with http:// or https://" % name)
    url = base + "/api/v3" + path
    if params:
        url += "?" + urllib.parse.urlencode(params)
    headers = {"X-Api-Key": settings[pre + "_api_key"].strip(), "Accept": "application/json"}
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data, headers=headers, method=method), timeout=TIMEOUT) as r:
            raw = r.read()
            return json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        try:
            msg = json.loads(e.read() or b"null")
            if isinstance(msg, list):
                msg = "; ".join(str(m.get("errorMessage") or m) for m in msg)
            elif isinstance(msg, dict):
                msg = msg.get("message") or msg
        except Exception:
            msg = e.reason
        raise ArrError("%s answered %d: %s" % (name, e.code, msg))
    except (OSError, ValueError) as e:
        raise ArrError(str(e))


def poster(found):
    for i in found.get("images") or []:
        if i.get("coverType") == "poster" and str(i.get("remoteUrl") or "").startswith("https://"):
            return i["remoteUrl"]
    p = found.get("remotePoster") or ""
    return p if p.startswith("https://") else ""


# Radarr's release dates and what they are called in "Coming soon" (the web app translates them).
RELEASES = (("inCinemas", "in cinemas"), ("digitalRelease", "digital release"), ("physicalRelease", "on disc"))


def calendar(settings, start, end):
    """What Radarr and Sonarr expect from `start` to `end` (YYYY-MM-DD) and do not have yet, for
    "Coming soon": movies on each of their release dates, episodes on their air date."""
    entries = []
    if configured(settings, "movie"):
        for m in arr(settings, "movie", "GET", "/calendar", {"start": start, "end": end}) or []:
            if m.get("hasFile"):
                continue
            for field, note in RELEASES:
                date = str(m.get(field) or "")[:10]
                if start <= date <= end:
                    entries.append({"type": "movie", "title": m.get("title") or "", "year": m.get("year"), "date": date,
                                    "poster": poster(m), "note": note})
    if configured(settings, "show"):
        for e in arr(settings, "show", "GET", "/calendar", {"start": start, "end": end, "includeSeries": "true"}) or []:
            date = str(e.get("airDate") or e.get("airDateUtc") or "")[:10]
            if e.get("hasFile") or not (start <= date <= end):
                continue
            series = e.get("series") or {}
            entries.append({"type": "episode", "title": series.get("title") or "", "year": series.get("year"),
                            "season": e.get("seasonNumber"), "episode": e.get("episodeNumber"), "name": e.get("title") or "",
                            "date": date, "poster": poster(series)})
    return entries


def key_of(kind, found):
    return "movie:%s" % found.get("tmdbId") if kind == "movie" else "show:%s" % found.get("tvdbId")


def search(settings, query):
    """(kind, found) for what Radarr and Sonarr know by this name, the best first."""
    out = []
    for kind in ("movie", "show"):
        if not configured(settings, kind):
            continue
        path = "/movie/lookup" if kind == "movie" else "/series/lookup"
        for f in (arr(settings, kind, "GET", path, {"term": query}) or [])[:8]:
            if f.get("tmdbId" if kind == "movie" else "tvdbId"):
                out.append((kind, f))
    q = norm(query)
    # Exact titles first, then each app's own order.
    out.sort(key=lambda kf: norm(kf[1].get("title")) != q)
    return out[:12]


def lookup_one(settings, kind, ident):
    if kind == "movie":
        found = arr(settings, kind, "GET", "/movie/lookup/tmdb", {"tmdbId": ident})
        return found if isinstance(found, dict) and found.get("tmdbId") else None
    found = arr(settings, kind, "GET", "/series/lookup", {"term": "tvdb:%s" % ident}) or []
    return found[0] if found else None


def pick(settings, kind, path, setting, field):
    """The quality profile or root folder: the one named in the settings, else the first."""
    pre = APPS[kind][0]
    want = str(settings.get(pre + "_" + setting) or "").strip()
    items = arr(settings, kind, "GET", path) or []
    if not items:
        raise ArrError("%s has no %s set up" % (APPS[kind][1], setting.replace("_", " ")))
    for i in items:
        if want and (str(i.get(field)) == want or str(i.get("id")) == want):
            return i
    return items[0]


def add_to_arr(settings, kind, found):
    """Hands the title to Radarr / Sonarr (if it is not there yet). Returns its id there."""
    if found.get("id"):
        return found["id"]
    profile = pick(settings, kind, "/qualityprofile", "quality_profile", "name")["id"]
    root = pick(settings, kind, "/rootfolder", "root_folder", "path")["path"]
    body = dict(found, qualityProfileId=profile, rootFolderPath=root, monitored=True)
    if kind == "movie":
        body.update(minimumAvailability="released", addOptions={"searchForMovie": True})
        return arr(settings, kind, "POST", "/movie", body=body)["id"]
    body.update(seasonFolder=True, addOptions={"monitor": "all", "searchForMissingEpisodes": True})
    try:  # Sonarr v3 needs a language profile; v4 has none.
        langs = arr(settings, kind, "GET", "/languageprofile") or []
        if langs:
            body["languageProfileId"] = langs[0]["id"]
    except ArrError:
        pass
    return arr(settings, kind, "POST", "/series", body=body)["id"]


def refresh_states(settings, wishes):
    """Asks Radarr / Sonarr how the open wishes stand (downloading, downloaded)."""
    for kind in ("movie", "show"):
        open_ = [w for w in wishes if w["kind"] == kind and w.get("arr_id") and w.get("status") in ("wished", "downloading", "downloaded")]
        if not open_ or not configured(settings, kind):
            continue
        try:
            queue = arr(settings, kind, "GET", "/queue", {"pageSize": 500}) or {}
            records = queue.get("records") or [] if isinstance(queue, dict) else []
            for w in open_:
                if kind == "movie":
                    m = arr(settings, kind, "GET", "/movie/%s" % w["arr_id"]) or {}
                    if m.get("hasFile"):
                        w["status"], w["progress"] = "downloaded", None
                        continue
                else:
                    st = (arr(settings, kind, "GET", "/series/%s" % w["arr_id"]) or {}).get("statistics") or {}
                    w["episodes"] = [st.get("episodeFileCount") or 0, st.get("episodeCount") or 0]
                    if st.get("episodeFileCount"):
                        w["status"] = "downloaded"
                idk = "movieId" if kind == "movie" else "seriesId"
                mine = [r for r in records if r.get(idk) == w["arr_id"]]
                if mine and w["status"] != "downloaded":
                    size = sum(r.get("size") or 0 for r in mine)
                    left = sum(r.get("sizeleft") or 0 for r in mine)
                    w["status"] = "downloading"
                    w["progress"] = int(round(100 * (size - left) / size)) if size else None
        except ArrError as e:
            log("%s: %s" % (APPS[kind][1], e))


# ---------- the panel

def status_text(lang, w):
    s = w.get("status") or "wished"
    if s == "downloading" and w.get("progress") is not None:
        return tr(lang, "downloading {p} %", p=w["progress"])
    return tr(lang, s)


def wish_item(lang, w, user, admin):
    kind_word = tr(lang, "Movie" if w["kind"] == "movie" else "Show")
    sub = [kind_word + (" · %s" % w["year"] if w.get("year") else "")]
    if w["kind"] == "show" and w.get("episodes") and w["episodes"][1]:
        sub.append(tr(lang, "{n} of {m} episodes", n=w["episodes"][0], m=w["episodes"][1]))
    if admin:
        sub.append(tr(lang, "wished by {who}", who=", ".join(w.get("by") or [])))
    if w.get("status") == "failed" and w.get("error"):
        sub.append(w["error"])
    buttons = []
    if user in (w.get("by") or []):
        buttons.append({"label": tr(lang, "Withdraw"), "action": "withdraw", "args": {"key": w["key"]}})
    if admin:
        buttons.append({"label": tr(lang, "Remove"), "action": "remove", "args": {"key": w["key"]}, "confirm": tr(lang, "Remove this wish for everyone?")})
    s = w.get("status") or "wished"
    return {"title": w["title"], "subtitle": " · ".join(sub), "image": w.get("image") or "",
            "status": status_text(lang, w), "status_kind": "default" if s == "here" else "smart" if s in ("wished", "downloading", "downloaded") else "",
            "buttons": buttons}


def may_wish(settings, admin):
    return admin or not settings.get("only_admins")


def intro(lang, settings):
    m, s = configured(settings, "movie"), configured(settings, "show")
    if m and s:
        return tr(lang, "Search for a movie or show you would like to see here. Radarr and Sonarr look for it."), tr(lang, "Movie or show…")
    return (tr(lang, "Search for a movie you would like to see here. Radarr looks for it."), tr(lang, "Movie…")) if m else \
        (tr(lang, "Search for a show you would like to see here. Sonarr looks for it."), tr(lang, "Show…"))


def panel(lang, settings, user, admin):
    if not configured(settings, "movie") and not configured(settings, "show"):
        return {"title": tr(lang, "Wish list"), "text": tr(lang, "The wish list is not set up yet: an admin enters the address and API key of Radarr and/or Sonarr in the plugin's settings.")}
    with locked():
        wishes = prune(load())
        refresh_states(settings, wishes)
        save(wishes)
    shown = wishes if admin else [w for w in wishes if user in (w.get("by") or [])]
    shown.sort(key=lambda w: (w.get("status") == "here", -(w.get("at") or 0)))
    text, placeholder = intro(lang, settings)
    out = {"title": tr(lang, "Wish list")}
    if may_wish(settings, admin):
        out["text"] = text
        out["input"] = {"placeholder": placeholder, "action": "search", "label": tr(lang, "Search")}
    else:
        out["text"] = tr(lang, "Only admins can wish for titles here.")
    if shown:
        out["items"] = [wish_item(lang, w, user, admin) for w in shown]
    elif may_wish(settings, admin):
        out["text"] += " " + tr(lang, "You have no wishes yet.")
    return out


def results_panel(lang, settings, user, query, results, wishes):
    by_key = {w["key"]: w for w in wishes}
    items = []
    for kind, f in results:
        k = key_of(kind, f)
        w = by_key.get(k)
        sub = tr(lang, "Movie" if kind == "movie" else "Show") + (" · %s" % f["year"] if f.get("year") else "")
        overview = (f.get("overview") or "").strip()
        if overview:
            sub += " · " + (overview[:160] + "…" if len(overview) > 160 else overview)
        ident = f.get("tmdbId") if kind == "movie" else f.get("tvdbId")
        item = {"title": f.get("title") or "?", "subtitle": sub, "image": poster(f), "buttons": []}
        have = f.get("hasFile") if kind == "movie" else ((f.get("statistics") or {}).get("episodeFileCount") or 0) > 0
        if w and user in (w.get("by") or []):
            item["status"], item["status_kind"] = tr(lang, "wished by you"), "smart"
        elif have or (w and w.get("status") == "here"):
            item["status"], item["status_kind"] = tr(lang, "already here"), "default"
        else:
            if w:
                item["status"], item["status_kind"] = status_text(lang, w), "smart"
            item["buttons"].append({"label": tr(lang, "Wish too" if w else "Wish for it"), "action": "wish", "primary": True,
                                    "args": {"kind": kind, "id": ident}})
        items.append(item)
    return {"title": tr(lang, "Results for “{q}”", q=query), "text": "" if items else tr(lang, "Nothing found for “{q}”.", q=query),
            "input": {"placeholder": intro(lang, settings)[1], "action": "search", "label": tr(lang, "Search"), "value": query},
            "items": items, "buttons": [{"label": tr(lang, "Back to my wishes"), "action": "panel"}]}


def prune(wishes):
    """Wishes that arrived more than KEEP_DAYS ago leave the list."""
    cut = time.time() - KEEP_DAYS * 86400
    return [w for w in wishes if not (w.get("status") == "here" and (w.get("here_at") or 0) < cut)]


def wish(lang, settings, user, admin, args):
    kind, ident = args.get("kind"), args.get("id")
    if kind not in APPS or not ident or not configured(settings, kind):
        return {"error": tr(lang, "No such wish.")}
    if not may_wish(settings, admin):
        return {"error": tr(lang, "Only admins can wish for titles here.")}
    key = "%s:%s" % (kind, ident)
    limit = int(settings.get("max_open_wishes") or 0)
    with locked():
        wishes = load()
        w = next((x for x in wishes if x["key"] == key), None)
        if w and user in (w.get("by") or []):
            return {"message": tr(lang, "You wished for this already.")}
        mine = [x for x in wishes if user in (x.get("by") or []) and x.get("status") != "here"]
        if limit and not admin and len(mine) >= limit:
            return {"error": tr(lang, "You have {n} open wishes, the most allowed. Withdraw one first.", n=len(mine))}
        if w is None:
            found = lookup_one(settings, kind, ident)
            if not found:
                return {"error": tr(lang, "No such wish.")}
            w = {"key": key, "kind": kind, "title": found.get("title") or "?", "year": found.get("year"),
                 "tmdb": found.get("tmdbId"), "tvdb": found.get("tvdbId"), "imdb": found.get("imdbId"),
                 "image": poster(found), "by": [], "at": int(time.time()), "status": "wished"}
            try:
                w["arr_id"] = add_to_arr(settings, kind, found)
            except ArrError as e:
                return {"error": tr(lang, "{app} could not be reached: {e}", app=APPS[kind][1], e=e)}
            wishes.append(w)
        w["by"] = (w.get("by") or []) + [user]
        save(wishes)
    return {"message": tr(lang, "Wished. {app} is looking for it.", app=APPS[kind][1]), "panel": panel(lang, settings, user, admin)}


def withdraw(lang, settings, user, admin, args, everyone):
    key = args.get("key")
    with locked():
        wishes = load()
        w = next((x for x in wishes if x["key"] == key), None)
        if not w or (not everyone and user not in (w.get("by") or [])):
            return {"error": tr(lang, "No such wish.")}
        if everyone:
            wishes.remove(w)
        else:
            w["by"] = [u for u in w["by"] if u != user]
            if not w["by"]:
                wishes.remove(w)
        save(wishes)
    # Only the list here: what Radarr / Sonarr already have stays there.
    return {"message": tr(lang, "Removed." if everyone else "Withdrawn."), "panel": panel(lang, settings, user, admin)}


# ---------- a title arrived

def arrived(item):
    """Marks the wishes for this movie (or this episode's show) as here. Returns their titles."""
    if not item:
        return []
    if item.get("type") == "movie":
        kind, title, year, ids = "movie", item.get("title"), item.get("year"), item.get("ids") or {}
    elif item.get("type") in ("episode", "season", "show"):
        show = item.get("show") or item
        kind, title, year, ids = "show", show.get("title"), show.get("year"), show.get("ids") or {}
    else:
        return []
    done = []
    with locked():
        wishes = load()
        for w in wishes:
            if w["kind"] != kind or w.get("status") == "here":
                continue
            same_id = any(ids.get(k) and str(ids.get(k)) == str(w.get(k)) for k in ("tmdb", "tvdb", "imdb"))
            same_name = norm(w["title"]) == norm(title) and (not year or not w.get("year") or abs(int(year) - int(w["year"])) <= 1)
            if same_id or same_name:
                w["status"], w["here_at"], w["progress"] = "here", int(time.time()), None
                done.append(w["title"])
        if done:
            save(wishes)
    return done


def main():
    # The server sends UTF-8, whatever the console code page is (Windows).
    msg = json.loads(getattr(sys.stdin, "buffer", sys.stdin).read())
    event = msg.get("event", "")
    data = msg.get("data") or {}
    settings = msg.get("settings") or {}
    if event == "library.item.added":
        print(json.dumps({"ok": True, "here": arrived(data.get("item"))}))
        return
    if event == "schedule.daily":
        with locked():
            wishes = load()
            kept = prune(wishes)
            if len(kept) != len(wishes):
                save(kept)
        print(json.dumps({"ok": True, "pruned": len(wishes) - len(kept)}))
        return
    if event == "calendar":
        try:
            out = {"entries": calendar(settings, str(data.get("start") or ""), str(data.get("end") or ""))}
        except ArrError as e:
            out = {"error": str(e)}
        print(json.dumps(out))
        return
    if event != "api.call":
        print(json.dumps({"skipped": event}))
        return
    body = data.get("body") if isinstance(data.get("body"), dict) else {}
    lang, user, admin = body.get("lang") or "en", data.get("user") or "", bool(data.get("admin"))
    action = data.get("action")
    args = body.get("args") if isinstance(body.get("args"), dict) else {}
    try:
        if action == "panel":
            out = panel(lang, settings, user, admin)
        elif action == "search":
            query = str(body.get("query") or "").strip()[:200]
            if not may_wish(settings, admin):
                out = {"error": tr(lang, "Only admins can wish for titles here.")}
            else:
                results = search(settings, query) if query else []
                out = {"panel": results_panel(lang, settings, user, query, results, load())}
        elif action == "wish":
            out = wish(lang, settings, user, admin, args)
        elif action == "withdraw":
            out = withdraw(lang, settings, user, admin, args, False)
        elif action == "remove" and admin:
            out = withdraw(lang, settings, user, admin, args, True)
        else:
            out = {"error": "Unknown action"}
    except ArrError as e:
        out = {"error": str(e)}
    # Escaped (\u00fc), so the answer survives any console code page (Windows).
    print(json.dumps(out))


if __name__ == "__main__":
    main()
