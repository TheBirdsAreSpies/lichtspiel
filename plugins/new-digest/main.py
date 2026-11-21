"""New on Lichtspiel: a weekly (or daily) message with what was added.

library.item.added notes each new movie and episode. On the chosen day (schedule.daily) the plugin
reads the library (for the titles and posters as they are by then, after the online details were
looked up), groups the episodes by show and sends each person who asked for it one message: an
e-mail with the posters inside, and/or a Telegram message with the posters as an album.

Each person has a panel under Settings: their e-mail address, connecting Telegram (a link to the bot
and a code, like Trakt), a preview, and switching it off again.

What it keeps (the titles noted, who gets what) is in state.json in the plugin's folder.
"""
import contextlib
import datetime
import html
import json
import mimetypes
import os
import re
import secrets
import smtplib
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from email.message import EmailMessage
from email.utils import make_msgid

HERE = os.path.dirname(os.path.abspath(__file__))
STATE = os.path.join(HERE, "state.json")
LOCK = os.path.join(HERE, ".state.lock")
TIMEOUT = 20
MAX_MOVIES = 30
MAX_SHOWS = 30
MAX_MAIL_POSTERS = 30
MAX_TELEGRAM_POSTERS = 10  # an album holds at most 10
MAX_POSTER_BYTES = 2_000_000
CODE_MINUTES = 15
TELEGRAM = "https://api.telegram.org"
DAYS = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"]
TAGE = ["montag", "dienstag", "mittwoch", "donnerstag", "freitag", "samstag", "sonntag"]

WORDS = {
    "New on {name}": "Neu auf {name}",
    "New on {name}: {what}": "Neu auf {name}: {what}",
    "{n} movie": "{n} Film",
    "{n} movies": "{n} Filme",
    "{n} show": "{n} Serie",
    "{n} shows": "{n} Serien",
    "Movies": "Filme",
    "Shows": "Serien",
    "{n} new episode": "{n} neue Folge",
    "{n} new episodes": "{n} neue Folgen",
    "and {n} more": "und {n} weitere",
    "Nothing new this time.": "Diesmal ist nichts Neues dazugekommen.",
    "Open {name}": "{name} öffnen",
    "You get this because you asked for it under Settings in {name}.": "Du bekommst das, weil du es in {name} unter Einstellungen so eingestellt hast.",
    "every {day}": "jeden {day}",
    "every day": "jeden Tag",
    "Once a week ({when}) you get a message with what was added to {name}.": "Einmal pro Woche ({when}) bekommst du eine Nachricht mit dem, was zu {name} dazugekommen ist.",
    "Every day you get a message with what was added to {name} (when something was).": "Jeden Tag bekommst du eine Nachricht mit dem, was zu {name} dazugekommen ist (wenn etwas dazugekommen ist).",
    "So far: {what}.": "Bisher: {what}.",
    "Nothing new so far.": "Bisher nichts Neues.",
    "Not set up yet: an admin enters an SMTP server or a Telegram bot in the plugin's settings.": "Noch nicht eingerichtet: Ein Admin trägt einen SMTP-Server oder einen Telegram-Bot in den Einstellungen des Plugins ein.",
    "It goes to {where}.": "Es geht an {where}.",
    "You get nothing yet: enter your e-mail address or connect Telegram.": "Du bekommst noch nichts: Trag deine E-Mail-Adresse ein oder verbinde Telegram.",
    "You get nothing yet: enter your e-mail address.": "Du bekommst noch nichts: Trag deine E-Mail-Adresse ein.",
    "You get nothing yet: connect Telegram.": "Du bekommst noch nichts: Verbinde Telegram.",
    "Your e-mail address": "Deine E-Mail-Adresse",
    "Save": "Speichern",
    "Telegram ({name})": "Telegram ({name})",
    "Connect Telegram": "Telegram verbinden",
    "Disconnect Telegram": "Telegram trennen",
    "No more e-mails": "Keine E-Mails mehr",
    "Send me a preview": "Vorschau an mich senden",
    "Send to everyone now": "Jetzt an alle senden",
    "Send what was added so far to everyone now? The list starts again afterwards.": "Das bisher Dazugekommene jetzt an alle senden? Die Liste beginnt danach von vorn.",
    "That is not an e-mail address.": "Das ist keine E-Mail-Adresse.",
    "Saved. The next message goes to {email}.": "Gespeichert. Die nächste Nachricht geht an {email}.",
    "No more e-mails.": "Keine E-Mails mehr.",
    "Telegram disconnected.": "Telegram getrennt.",
    "Open the link (or search for @{bot} in Telegram) and press Start.": "Öffne den Link (oder such in Telegram nach @{bot}) und tipp auf Starten.",
    "Telegram is connected.": "Telegram ist verbunden.",
    "Connected. From now on {name} sends you what is new here.": "Verbunden. Ab jetzt schickt dir {name} hier, was neu ist.",
    "The code has expired. Try again.": "Der Code ist abgelaufen. Versuch es noch einmal.",
    "Sent: {n}.": "Gesendet: {n}.",
    "Sent to {n} people.": "An {n} Personen gesendet.",
    "Nothing was sent: {why}": "Nichts gesendet: {why}",
    "nothing new": "nichts Neues",
    "you get nothing yet": "du bekommst noch nichts",
    "the plugin's log on the Plugins page says why": "das Protokoll des Plugins auf der Seite Plugins sagt, warum",
    "Telegram could not be reached: {e}": "Telegram ist nicht erreichbar: {e}",
    "The e-mail could not be sent: {e}": "Die E-Mail konnte nicht gesendet werden: {e}",
    "Only admins can do that.": "Das dürfen nur Admins.",
}


class SendError(Exception):
    pass


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def plural(lang, n, one, many):
    return tr(lang, one if n == 1 else many, n=n)


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
                if time.time() - os.path.getmtime(LOCK) > 180:
                    os.rmdir(LOCK)
                    continue
            except OSError:
                pass
            if time.time() > end:
                raise SendError("busy, try again in a moment")
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


# ---------- the settings

def mail_ready(s):
    return bool(str(s.get("smtp_host") or "").strip() and str(s.get("smtp_from") or s.get("smtp_user") or "").strip())


def telegram_ready(s):
    return bool(str(s.get("telegram_bot_token") or "").strip())


def weekday(s):
    w = str(s.get("weekday") or "friday").strip().lower()
    for names in (DAYS, TAGE):
        for i, n in enumerate(names):
            if w == n or w == n[:2] or w == n[:3]:
                return i
    return 4


def every_day(s):
    return str(s.get("every") or "week").strip().lower() in ("day", "daily", "tag", "täglich")


def due(s, date, last):
    """The message is due on this day (YYYY-MM-DD; `last` is the day it was sent last)."""
    if last == date:
        return False
    if every_day(s):
        return True
    day = datetime.date.fromisoformat(date)
    if day.weekday() == weekday(s):
        return True
    # The day was missed (the server was off): send it now rather than a week later.
    try:
        return (day - datetime.date.fromisoformat(last)).days > 7
    except (TypeError, ValueError):
        return False


def today():
    """Today in UTC, like the server's schedule.daily."""
    return datetime.datetime.now(datetime.timezone.utc).date().isoformat()


def day_name(lang, s):
    return (TAGE if (lang or "").startswith("de") else DAYS)[weekday(s)].capitalize()


# ---------- what is new

def note(item):
    if not item or item.get("type") not in ("movie", "episode") or not item.get("id"):
        return False
    with locked():
        state = load()
        pending = state.setdefault("pending", [])
        if any(p.get("id") == item["id"] for p in pending):
            return False
        pending.append({"id": item["id"], "at": int(time.time())})
        save(state)
    return True


def read_library(server):
    url = (server or {}).get("library")
    if not url:
        raise SendError("the server did not give the library's address (an older server?)")
    with urllib.request.urlopen(url, timeout=60) as r:
        return json.loads(r.read())


def episodes_text(eps):
    """[(2, 1), (2, 2), (2, 3), (2, 5), (3, 1)] → 'S02E01–E03, S02E05, S03E01'."""
    out = []
    eps = sorted(set((s or 0, e or 0) for s, e in eps))
    i = 0
    while i < len(eps):
        s, e = eps[i]
        j = i
        while j + 1 < len(eps) and eps[j + 1] == (s, eps[j][1] + 1):
            j += 1
        out.append("S%02dE%02d" % (s, e) + ("–E%02d" % eps[j][1] if j > i else ""))
        i = j + 1
    return ", ".join(out)


def digest(library, pending, s):
    """{"movies": [...], "shows": [...], "more_movies": n, "more_shows": n} from the noted ids."""
    skip = [str(x).strip().lower() for x in (s.get("skip_libraries") or []) if str(x).strip()]
    sections = {sec.get("id"): sec for sec in library.get("sections") or []}
    items = {it.get("id"): it for it in library.get("items") or [] if not it.get("version_of")}
    movies, shows = [], {}
    for p in sorted(pending, key=lambda p: p.get("at") or 0):
        it = items.get(p.get("id"))
        if not it:
            continue  # gone again, or merged into another version
        sec = sections.get(it.get("section_id")) or {}
        if str(sec.get("name") or "").strip().lower() in skip:
            continue
        if it.get("type") == "movie":
            movies.append({"id": it["id"], "title": it.get("title") or "?", "year": it.get("year"), "poster": it.get("poster")})
        elif it.get("type") == "episode":
            show = it.get("show") or {}
            key = show.get("id") or show.get("title") or it.get("title")
            entry = shows.setdefault(key, {"id": show.get("id"), "title": show.get("title") or "?", "year": show.get("year"),
                                           "poster": show.get("poster"), "episodes": []})
            entry["episodes"].append((it.get("season"), it.get("episode")))
    shows = list(shows.values())
    for sh in shows:
        sh["count"] = len(sh["episodes"])
        sh["range"] = episodes_text(sh.pop("episodes"))
    return {"movies": movies[:MAX_MOVIES], "shows": shows[:MAX_SHOWS],
            "more_movies": max(0, len(movies) - MAX_MOVIES), "more_shows": max(0, len(shows) - MAX_SHOWS)}


def summary(lang, d):
    parts = []
    m = len(d["movies"]) + d["more_movies"]
    n = len(d["shows"]) + d["more_shows"]
    if m:
        parts.append(plural(lang, m, "{n} movie", "{n} movies"))
    if n:
        parts.append(plural(lang, n, "{n} show", "{n} shows"))
    return ", ".join(parts)


def empty(d):
    return not d["movies"] and not d["shows"]


def link(server, ident):
    base = str((server or {}).get("public_url") or "").rstrip("/")
    return "%s/web/#/item/%s" % (base, ident) if base and ident else ""


def poster_bytes(path):
    """The poster file's bytes and its type, or None (missing, too big, not a picture)."""
    if not path:
        return None
    try:
        if os.path.getsize(path) > MAX_POSTER_BYTES:
            return None
        with open(path, "rb") as f:
            data = f.read()
    except OSError:
        return None
    kind = "png" if data[:8] == b"\x89PNG\r\n\x1a\n" else "jpeg" if data[:3] == b"\xff\xd8\xff" else "webp" if data[8:12] == b"WEBP" else None
    return (data, kind) if kind else None


# ---------- e-mail

def mail_html(lang, server, d, cids):
    name = html.escape(server.get("name") or "Lichtspiel")
    e = html.escape

    def card(x, sub):
        url = link(server, x.get("id"))
        img = '<img src="cid:%s" width="120" style="width:120px;border-radius:6px;display:block" alt="">' % cids[id(x)][1:-1] if id(x) in cids else \
            '<div style="width:120px;height:180px;border-radius:6px;background:#333"></div>'
        title = "<b>%s</b>" % e(x["title"]) + (" (%s)" % x["year"] if x.get("year") else "")
        if url:
            img, title = '<a href="%s">%s</a>' % (e(url), img), '<a href="%s" style="color:#fff;text-decoration:none">%s</a>' % (e(url), title)
        return ('<td valign="top" style="padding:0 12px 18px 0;width:120px;font:13px/1.35 sans-serif;color:#eee">%s'
                '<div style="margin-top:6px">%s</div><div style="color:#aaa">%s</div></td>' % (img, title, e(sub)))

    def grid(entries, subs):
        rows, row = [], []
        for x, sub in zip(entries, subs):
            row.append(card(x, sub))
            if len(row) == 4:
                rows.append("<tr>%s</tr>" % "".join(row))
                row = []
        if row:
            rows.append("<tr>%s</tr>" % "".join(row))
        return '<table cellpadding="0" cellspacing="0" role="presentation">%s</table>' % "".join(rows)

    out = ['<div style="background:#141414;padding:24px;font:15px/1.4 sans-serif;color:#eee">',
           '<h1 style="font-size:22px;margin:0 0 18px">%s</h1>' % e(tr(lang, "New on {name}", name=server.get("name") or "Lichtspiel"))]
    if d["movies"]:
        out.append('<h2 style="font-size:17px;margin:8px 0 12px">%s</h2>' % e(tr(lang, "Movies")))
        out.append(grid(d["movies"], [""] * len(d["movies"])))
        if d["more_movies"]:
            out.append('<p style="color:#aaa">%s</p>' % e(tr(lang, "and {n} more", n=d["more_movies"])))
    if d["shows"]:
        out.append('<h2 style="font-size:17px;margin:8px 0 12px">%s</h2>' % e(tr(lang, "Shows")))
        out.append(grid(d["shows"], [plural(lang, s["count"], "{n} new episode", "{n} new episodes") + " · " + s["range"] for s in d["shows"]]))
        if d["more_shows"]:
            out.append('<p style="color:#aaa">%s</p>' % e(tr(lang, "and {n} more", n=d["more_shows"])))
    if server.get("public_url"):
        out.append('<p><a href="%s/web/" style="color:#e5a00d">%s</a></p>' % (e(server["public_url"]), e(tr(lang, "Open {name}", name=server.get("name") or "Lichtspiel"))))
    out.append('<p style="color:#777;font-size:12px">%s</p></div>' % e(tr(lang, "You get this because you asked for it under Settings in {name}.", name=name)))
    return "".join(out)


def plain_text(lang, server, d, for_telegram=False):
    """The message as text (the e-mail's plain part; Telegram's HTML when for_telegram)."""
    e = html.escape if for_telegram else (lambda x: x)
    name = server.get("name") or "Lichtspiel"
    lines = [("<b>%s</b>" if for_telegram else "%s") % e(tr(lang, "New on {name}", name=name)), ""]

    def entry(x, extra=""):
        t = e(x["title"]) + (" (%s)" % x["year"] if x.get("year") else "")
        url = link(server, x.get("id"))
        if for_telegram and url:
            t = '<a href="%s">%s</a>' % (e(url), t)
        elif url:
            t += " — " + url
        return "• " + t + (" — " + e(extra) if extra else "")

    if d["movies"]:
        lines.append(("<b>%s</b>" if for_telegram else "%s") % e(tr(lang, "Movies")))
        lines += [entry(m) for m in d["movies"]]
        if d["more_movies"]:
            lines.append(e(tr(lang, "and {n} more", n=d["more_movies"])))
        lines.append("")
    if d["shows"]:
        lines.append(("<b>%s</b>" if for_telegram else "%s") % e(tr(lang, "Shows")))
        lines += [entry(s, plural(lang, s["count"], "{n} new episode", "{n} new episodes") + " · " + s["range"]) for s in d["shows"]]
        if d["more_shows"]:
            lines.append(e(tr(lang, "and {n} more", n=d["more_shows"])))
    return "\n".join(lines).strip()


def send_mail(s, to, lang, server, d):
    msg = EmailMessage()
    msg["Subject"] = tr(lang, "New on {name}: {what}", name=server.get("name") or "Lichtspiel", what=summary(lang, d))
    msg["From"] = str(s.get("smtp_from") or s.get("smtp_user")).strip()
    msg["To"] = to
    msg.set_content(plain_text(lang, server, d))
    pictures, cids = [], {}
    for x in (d["movies"] + d["shows"])[:MAX_MAIL_POSTERS]:
        pic = poster_bytes(x.get("poster"))
        if pic:
            cids[id(x)] = make_msgid()
            pictures.append((cids[id(x)], pic))
    msg.add_alternative(mail_html(lang, server, d, cids), subtype="html")
    body = msg.get_payload()[1]
    for cid, (data, kind) in pictures:
        body.add_related(data, maintype="image", subtype=kind, cid=cid)
    host, port = str(s.get("smtp_host")).strip(), int(s.get("smtp_port") or 587)
    security = str(s.get("smtp_security") or "starttls").strip().lower()
    try:
        if security == "ssl":
            smtp = smtplib.SMTP_SSL(host, port, timeout=TIMEOUT, context=ssl.create_default_context())
        else:
            smtp = smtplib.SMTP(host, port, timeout=TIMEOUT)
        with smtp:
            if security == "starttls":
                smtp.starttls(context=ssl.create_default_context())
            if str(s.get("smtp_user") or "").strip():
                smtp.login(str(s["smtp_user"]).strip(), str(s.get("smtp_password") or ""))
            smtp.send_message(msg)
    except (OSError, smtplib.SMTPException) as e:
        raise SendError(tr(lang, "The e-mail could not be sent: {e}", e=e))


# ---------- Telegram

def tg(s, method, params=None, files=None):
    url = "%s/bot%s/%s" % (TELEGRAM, str(s.get("telegram_bot_token")).strip(), method)
    if files:
        boundary = uuid.uuid4().hex
        parts = []
        for k, v in (params or {}).items():
            parts.append(('--%s\r\nContent-Disposition: form-data; name="%s"\r\n\r\n' % (boundary, k)).encode() + str(v).encode() + b"\r\n")
        for k, (fname, data, ctype) in files.items():
            parts.append(('--%s\r\nContent-Disposition: form-data; name="%s"; filename="%s"\r\nContent-Type: %s\r\n\r\n'
                          % (boundary, k, fname, ctype)).encode() + data + b"\r\n")
        body = b"".join(parts) + ("--%s--\r\n" % boundary).encode()
        req = urllib.request.Request(url, data=body, headers={"Content-Type": "multipart/form-data; boundary=" + boundary})
    else:
        req = urllib.request.Request(url, data=json.dumps(params or {}).encode(), headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
            answer = json.loads(r.read())
    except urllib.error.HTTPError as e:
        try:
            answer = json.loads(e.read())
        except ValueError:
            raise SendError("Telegram answered %d" % e.code)
    except (OSError, ValueError) as e:
        # The address holds the bot's token: say what went wrong without it.
        raise SendError(str(getattr(e, "reason", e)).replace(str(s.get("telegram_bot_token")), "…"))
    if not answer.get("ok"):
        raise SendError("Telegram: %s" % answer.get("description"))
    return answer.get("result")


def send_telegram(s, chat, lang, server, d):
    text = plain_text(lang, server, d, for_telegram=True)
    # Telegram takes at most 4096 characters; cut at a line.
    if len(text) > 4000:
        text = text[:4000].rsplit("\n", 1)[0] + "\n…"
    pics = []
    for x in d["movies"] + d["shows"]:
        pic = poster_bytes(x.get("poster"))
        if pic:
            pics.append(pic)
        if len(pics) == MAX_TELEGRAM_POSTERS:
            break
    try:
        if len(pics) >= 2:
            media = [{"type": "photo", "media": "attach://p%d" % i} for i in range(len(pics))]
            tg(s, "sendMediaGroup", {"chat_id": chat, "media": json.dumps(media)},
               {"p%d" % i: ("p%d.%s" % (i, "jpg" if k == "jpeg" else k), data, "image/" + k) for i, (data, k) in enumerate(pics)})
        elif pics:
            data, k = pics[0]
            tg(s, "sendPhoto", {"chat_id": chat}, {"photo": ("p.%s" % ("jpg" if k == "jpeg" else k), data, "image/" + k)})
        tg(s, "sendMessage", {"chat_id": chat, "text": text, "parse_mode": "HTML", "link_preview_options": {"is_disabled": True}})
    except SendError as e:
        raise SendError(tr(lang, "Telegram could not be reached: {e}", e=e))


# ---------- sending

def send_to(s, server, p, d):
    """Sends the digest to one person. Returns the channels it went to; errors go to the log."""
    lang = p.get("lang") or "en"
    done = []
    if p.get("email") and mail_ready(s):
        try:
            send_mail(s, p["email"], lang, server, d)
            done.append("e-mail")
        except SendError as e:
            log(str(e))
    if p.get("telegram") and telegram_ready(s):
        try:
            send_telegram(s, p["telegram"], lang, server, d)
            done.append("Telegram")
        except SendError as e:
            log(str(e))
    return done


def send_all(s, server, date):
    """Sends what was noted to everyone and starts the list again. Returns how many got it."""
    # Sending takes a while: titles added meanwhile are noted for the next time, not lost.
    state = load()
    pending = state.get("pending") or []
    sent = 0
    if pending:
        d = digest(read_library(server), pending, s)
        if not empty(d):
            for user, p in sorted((state.get("people") or {}).items()):
                if send_to(s, server, p, d):
                    sent += 1
    done = {p.get("id") for p in pending}
    with locked():
        state = load()
        state["pending"] = [p for p in state.get("pending") or [] if p.get("id") not in done]
        state["last_sent"] = date
        save(state)
    return sent


# ---------- connecting Telegram

def telegram_link(lang, s, user):
    bot = tg(s, "getMe") or {}
    code = secrets.token_hex(4)
    with locked():
        state = load()
        codes = {c: v for c, v in (state.get("codes") or {}).items() if v.get("user") != user and time.time() - v.get("at", 0) < CODE_MINUTES * 60}
        codes[code] = {"user": user, "at": int(time.time())}
        state["codes"] = codes
        save(state)
    return waiting(lang, bot.get("username") or "", code)


def waiting(lang, bot, code):
    return {"message": tr(lang, "Open the link (or search for @{bot} in Telegram) and press Start.", bot=bot), "code": code,
            "link": "https://t.me/%s?start=%s" % (bot, code), "poll": {"action": "telegram_poll", "seconds": 4}}


def telegram_poll(lang, s, server, user, admin):
    """Looks for "/start <code>" sent to the bot; every person waiting is connected at once."""
    with locked():
        state = load()
        offset = state.get("tg_offset") or 0
        updates = tg(s, "getUpdates", {"offset": offset, "timeout": 0, "allowed_updates": ["message"]}) or []
        codes = state.get("codes") or {}
        for u in updates:
            state["tg_offset"] = max(state.get("tg_offset") or 0, u.get("update_id", 0) + 1)
            m = u.get("message") or {}
            text = str(m.get("text") or "")
            if not text.startswith("/start "):
                continue
            c = codes.pop(text.split(" ", 1)[1].strip(), None)
            if not c or time.time() - c.get("at", 0) > CODE_MINUTES * 60:
                continue
            chat = (m.get("chat") or {}).get("id")
            who = m.get("from") or {}
            p = person(state, c["user"])
            p["telegram"] = chat
            p["telegram_name"] = who.get("username") or who.get("first_name") or ""
            try:
                tg(s, "sendMessage", {"chat_id": chat, "text": tr(p.get("lang") or lang, "Connected. From now on {name} sends you what is new here.",
                                                                    name=server.get("name") or "Lichtspiel")})
            except SendError as e:
                log(str(e))
        state["codes"] = codes
        save(state)
        p = (state.get("people") or {}).get(user) or {}
        mine = [c for c, v in codes.items() if v.get("user") == user]
    if p.get("telegram") and not mine:
        return {"message": tr(lang, "Telegram is connected."), "panel": panel(lang, s, server, user, admin)}
    if not mine:
        return {"error": tr(lang, "The code has expired. Try again.")}
    bot = (tg(s, "getMe") or {}).get("username") or ""
    return waiting(lang, bot, mine[0])


# ---------- the panel

def panel(lang, s, server, user, admin):
    name = server.get("name") or "Lichtspiel"
    out = {"title": tr(lang, "New on {name}", name=name)}
    if not mail_ready(s) and not telegram_ready(s):
        out["text"] = tr(lang, "Not set up yet: an admin enters an SMTP server or a Telegram bot in the plugin's settings.")
        return out
    with locked():
        state = load()
        p = person(state, user)
        if p.get("lang") != lang:
            p["lang"] = lang
            save(state)
    if every_day(s):
        text = [tr(lang, "Every day you get a message with what was added to {name} (when something was).", name=name)]
    else:
        when = tr(lang, "every {day}", day=day_name(lang, s))
        text = [tr(lang, "Once a week ({when}) you get a message with what was added to {name}.", when=when, name=name)]
    where = []
    if p.get("email") and mail_ready(s):
        where.append(p["email"])
    if p.get("telegram") and telegram_ready(s):
        where.append(tr(lang, "Telegram ({name})", name=p.get("telegram_name") or "?"))
    if where:
        text.append(tr(lang, "It goes to {where}.", where=", ".join(where)))
    elif mail_ready(s) and telegram_ready(s):
        text.append(tr(lang, "You get nothing yet: enter your e-mail address or connect Telegram."))
    else:
        text.append(tr(lang, "You get nothing yet: enter your e-mail address." if mail_ready(s) else "You get nothing yet: connect Telegram."))
    n = len(state.get("pending") or [])
    if admin:
        text.append(tr(lang, "So far: {what}.", what=str(n)) if n else tr(lang, "Nothing new so far."))
    out["text"] = " ".join(text)
    if mail_ready(s):
        out["input"] = {"placeholder": tr(lang, "Your e-mail address"), "action": "email", "label": tr(lang, "Save"), "value": p.get("email") or ""}
    buttons = []
    if telegram_ready(s):
        if p.get("telegram"):
            buttons.append({"label": tr(lang, "Disconnect Telegram"), "action": "telegram_off"})
        else:
            buttons.append({"label": tr(lang, "Connect Telegram"), "action": "telegram_link", "primary": True})
    if p.get("email") and mail_ready(s):
        buttons.append({"label": tr(lang, "No more e-mails"), "action": "email_off"})
    if where:
        buttons.append({"label": tr(lang, "Send me a preview"), "action": "preview"})
    if admin:
        buttons.append({"label": tr(lang, "Send to everyone now"), "action": "send_now",
                        "confirm": tr(lang, "Send what was added so far to everyone now? The list starts again afterwards.")})
    out["buttons"] = buttons
    return out


EMAIL = re.compile(r"^[^@\s<>,;\"]+@[^@\s<>,;\"]+\.[^@\s<>,;\"]+$")


def set_person(user, **kw):
    with locked():
        state = load()
        p = person(state, user)
        for k, v in kw.items():
            if v is None:
                p.pop(k, None)
            else:
                p[k] = v
        save(state)


def preview(lang, s, server, user):
    state = load()
    p = dict((state.get("people") or {}).get(user) or {}, lang=lang)
    d = digest(read_library(server), state.get("pending") or [], s)
    if empty(d):
        return {"error": tr(lang, "Nothing was sent: {why}", why=tr(lang, "nothing new"))}
    done = send_to(s, server, p, d)
    if not done:
        return {"error": tr(lang, "Nothing was sent: {why}", why=tr(lang, "you get nothing yet") if not (p.get("email") or p.get("telegram"))
                            else tr(lang, "the plugin's log on the Plugins page says why"))}
    return {"message": tr(lang, "Sent: {n}.", n=", ".join(done))}


def call(action, lang, s, server, user, admin, body):
    if action == "panel":
        return panel(lang, s, server, user, admin)
    if action == "email":
        email = str(body.get("query") or "").strip()
        if not EMAIL.match(email) or len(email) > 200:
            return {"error": tr(lang, "That is not an e-mail address.")}
        set_person(user, email=email, lang=lang)
        return {"message": tr(lang, "Saved. The next message goes to {email}.", email=email), "panel": panel(lang, s, server, user, admin)}
    if action == "email_off":
        set_person(user, email=None)
        return {"message": tr(lang, "No more e-mails."), "panel": panel(lang, s, server, user, admin)}
    if action == "telegram_link" and telegram_ready(s):
        return telegram_link(lang, s, user)
    if action == "telegram_poll" and telegram_ready(s):
        return telegram_poll(lang, s, server, user, admin)
    if action == "telegram_off":
        set_person(user, telegram=None, telegram_name=None)
        return {"message": tr(lang, "Telegram disconnected."), "panel": panel(lang, s, server, user, admin)}
    if action == "preview":
        return preview(lang, s, server, user)
    if action == "send_now":
        if not admin:
            return {"error": tr(lang, "Only admins can do that.")}
        n = send_all(s, server, today())
        return {"message": tr(lang, "Sent to {n} people.", n=n), "panel": panel(lang, s, server, user, admin)}
    return {"error": "Unknown action"}


def main():
    # The server sends UTF-8, whatever the console code page is (Windows).
    msg = json.loads(getattr(sys.stdin, "buffer", sys.stdin).read())
    event = msg.get("event", "")
    data = msg.get("data") or {}
    s = msg.get("settings") or {}
    server = msg.get("server") or {}
    try:
        if event == "library.item.added":
            out = {"ok": True, "noted": note(data.get("item"))}
        elif event == "schedule.daily":
            date = data.get("date") or today()
            if due(s, date, load().get("last_sent") or ""):
                out = {"ok": True, "sent_to": send_all(s, server, date)}
            else:
                out = {"ok": True, "due": False}
        elif event == "api.call":
            body = data.get("body") if isinstance(data.get("body"), dict) else {}
            out = call(data.get("action"), body.get("lang") or "en", s, server, data.get("user") or "", bool(data.get("admin")), body)
        else:
            out = {"skipped": event}
    except SendError as e:
        out = {"error": str(e)}
    except (OSError, ValueError) as e:  # the library could not be read …
        out = {"error": str(e)}
    # Escaped (ü), so the answer survives any console code page (Windows).
    print(json.dumps(out))


if __name__ == "__main__":
    main()
