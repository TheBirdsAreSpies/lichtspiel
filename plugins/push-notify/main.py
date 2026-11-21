"""Telegram / Pushover plugin.

Sends the events chosen in the settings to Telegram chats and Pushover users, each in the message
format they need (webhook-notify covers Discord, ntfy and Home Assistant). New titles are collected
during a scan and sent as one message when it finishes, so a big scan is one message and not a
thousand; a message about one title carries its poster.

Admins get a panel under Settings: connect a Telegram chat or group (a link to the bot and a code,
like Trakt), see the chats connected, remove one, send a test message.

The chats connected and the titles waiting for the end of a scan are kept in state.json in the
plugin's folder. Messages are in the language of the admin who opened the panel last.
"""
import contextlib
import html
import json
import os
import secrets
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
STATE = os.path.join(HERE, "state.json")
LOCK = os.path.join(HERE, ".state.lock")
TIMEOUT = 15
TELEGRAM = "https://api.telegram.org"
PUSHOVER = "https://api.pushover.net/1/messages.json"
CODE_MINUTES = 15
MAX_LISTED = 25
MAX_POSTER_BYTES = 2_000_000  # Pushover takes at most 2.5 MB, Telegram 10 MB
EVENTS = ["library.item.added", "library.scan.finished", "playback.started", "playback.stopped", "playback.watched",
          "item.problem", "download.ready", "user.invited", "user.joined", "server.started"]

WORDS = {
    "New on {name}": "Neu auf {name}",
    "and {n} more": "und {n} weitere",
    "{n} new episode": "{n} neue Folge",
    "{n} new episodes": "{n} neue Folgen",
    "{user} is watching {what}": "{user} schaut {what}",
    "on {device}": "auf {device}",
    "{user} stopped {what} at {p} %": "{user} hat {what} bei {p} % beendet",
    "{user} stopped {what}": "{user} hat {what} beendet",
    "{user} finished {what}": "{user} hat {what} fertig geschaut",
    "{user} reported a problem with {what}": "{user} hat ein Problem mit {what} gemeldet",
    "{user} can download {what} now": "{user} kann {what} jetzt herunterladen",
    "{by} sent an invite": "{by} hat eine Einladung verschickt",
    "{user} joined {name}": "{user} ist {name} beigetreten",
    "{name} started": "{name} wurde gestartet",
    "Scan finished: {added} added, {removed} removed": "Scan fertig: {added} hinzugefügt, {removed} entfernt",
    "Test message from {name}": "Testnachricht von {name}",
    "Telegram / Pushover": "Telegram / Pushover",
    "Not set up yet: enter a Telegram bot token and/or a Pushover token and user keys in the plugin's settings.": "Noch nicht eingerichtet: Trag einen Telegram-Bot-Token und/oder einen Pushover-Token mit Benutzerschlüsseln in den Einstellungen des Plugins ein.",
    "Sends {events} to {where}.": "Sendet {events} an {where}.",
    "nothing yet": "noch nichts",
    "{n} Telegram chats": "{n} Telegram-Chats",
    "1 Telegram chat": "1 Telegram-Chat",
    "{n} Pushover users": "{n} Pushover-Empfänger",
    "1 Pushover user": "1 Pushover-Empfänger",
    "nowhere yet: connect a Telegram chat": "noch nirgendwohin: Verbinde einen Telegram-Chat",
    "Connect a Telegram chat": "Telegram-Chat verbinden",
    "Send a test message": "Testnachricht senden",
    "Remove": "Entfernen",
    "Telegram chat": "Telegram-Chat",
    "Telegram group": "Telegram-Gruppe",
    "Open the link (or search for @{bot} in Telegram) and press Start. For a group: add @{bot} to it and send /start@{bot} {code} there.": "Öffne den Link (oder such in Telegram nach @{bot}) und tipp auf Starten. Für eine Gruppe: Füg @{bot} hinzu und schick dort /start@{bot} {code}.",
    "Connected. {name} sends its messages here from now on.": "Verbunden. {name} schickt seine Nachrichten ab jetzt hierher.",
    "The chat is connected.": "Der Chat ist verbunden.",
    "The code has expired. Try again.": "Der Code ist abgelaufen. Versuch es noch einmal.",
    "Removed.": "Entfernt.",
    "Sent to {n} of {m}.": "An {n} von {m} gesendet.",
    "Only admins can do that.": "Das dürfen nur Admins.",
    "new titles": "neue Titel",
    "scans": "Scans",
    "playback": "Wiedergaben",
    "finished titles": "fertig geschaute Titel",
    "problems": "Probleme",
    "downloads": "Downloads",
    "invites": "Einladungen",
    "new users": "neue Benutzer",
    "server starts": "Serverstarts",
}

EVENT_NAMES = {"library.item.added": "new titles", "library.scan.finished": "scans", "playback.started": "playback",
               "playback.stopped": "playback", "playback.watched": "finished titles", "item.problem": "problems",
               "download.ready": "downloads", "user.invited": "invites", "user.joined": "new users", "server.started": "server starts"}


class SendError(Exception):
    pass


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def log(text):
    print(text, file=sys.stderr)


# ---------- what it keeps (several runs may happen at once, so with a lock)

@contextlib.contextmanager
def locked():
    end = time.time() + 15
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


# ---------- the settings

def as_list(v):
    if isinstance(v, str):
        return [x.strip() for x in v.split(",") if x.strip()]
    return [str(x).strip() for x in (v or []) if str(x).strip()]


def telegram_ready(s):
    return bool(str(s.get("telegram_bot_token") or "").strip())


def pushover_ready(s):
    return bool(str(s.get("pushover_token") or "").strip() and as_list(s.get("pushover_users")))


# ---------- the messages

def what(item):
    """'Heat (1995)' or 'Lost S01E02 · Two'."""
    if not item:
        return "?"
    if item.get("type") == "episode":
        show = (item.get("show") or {}).get("title") or ""
        se = "S%02dE%02d" % (item.get("season") or 0, item.get("episode") or 0)
        return ("%s %s" % (show, se)).strip() + (" · " + item["title"] if item.get("title") else "")
    return (item.get("title") or "?") + (" (%s)" % item["year"] if item.get("year") else "")


def poster_of(item):
    item = item or {}
    return item.get("poster") or (item.get("show") or {}).get("poster")


def link(server, ident):
    base = str((server or {}).get("public_url") or "").rstrip("/")
    return "%s/web/#/item/%s" % (base, ident) if base and ident else ""


def new_titles(lang, items):
    """The lines for the titles of a scan: movies one by one, episodes by show."""
    lines, shows = [], {}
    for it in items:
        if it.get("type") == "episode":
            show = (it.get("show") or {}).get("title") or "?"
            shows.setdefault(show, []).append(it)
        elif it.get("type") == "movie":
            lines.append(what(it))
    for show, eps in shows.items():
        lines.append(what(eps[0]) if len(eps) == 1 else "%s · %s" % (show, tr(lang, "{n} new episodes", n=len(eps))))
    if len(lines) > MAX_LISTED:
        lines = lines[:MAX_LISTED] + [tr(lang, "and {n} more", n=len(lines) - MAX_LISTED)]
    return lines


def message(lang, event, data, server):
    """(title, text, the item it is about or None) for an event; None when there is nothing to say."""
    name = server.get("name") or "Lichtspiel"
    item = data.get("item") or {}
    # user_name: the full name when the person gave one, else the user name.
    user = data.get("user_name") or data.get("user") or "?"
    if event == "playback.started":
        text = tr(lang, "{user} is watching {what}", user=user, what=what(item))
        device = (data.get("player") or {}).get("device")
        return name, text + (" " + tr(lang, "on {device}", device=device) if device else ""), item
    if event == "playback.stopped":
        p = data.get("progress")
        if p is not None:
            return name, tr(lang, "{user} stopped {what} at {p} %", user=user, what=what(item), p=int(round(p))), item
        return name, tr(lang, "{user} stopped {what}", user=user, what=what(item)), item
    if event == "playback.watched":
        return name, tr(lang, "{user} finished {what}", user=user, what=what(item)), item
    if event == "item.problem":
        text = tr(lang, "{user} reported a problem with {what}", user=user, what=data.get("title") or "?")
        extra = " · ".join(str(x) for x in (data.get("kind"), data.get("note")) if x)
        return name, text + (": " + extra if extra else ""), {"id": data.get("id")}
    if event == "download.ready":
        return name, tr(lang, "{user} can download {what} now", user=user, what=data.get("title") or "#%s" % data.get("id")), {"id": data.get("id")}
    if event == "user.invited":
        return name, tr(lang, "{by} sent an invite", by=data.get("by") or "?"), None
    if event == "user.joined":
        return name, tr(lang, "{user} joined {name}", user=data.get("user_name") or data.get("username") or "?", name=name), None
    if event == "server.started":
        return name, tr(lang, "{name} started", name=name) + (" (%s)" % server["version"] if server.get("version") else ""), None
    if event == "library.scan.finished":
        if not data.get("added") and not data.get("removed"):
            return None
        return name, tr(lang, "Scan finished: {added} added, {removed} removed", added=data.get("added") or 0, removed=data.get("removed") or 0), None
    return None


def poster_bytes(path):
    if not path:
        return None
    try:
        if os.path.getsize(path) > MAX_POSTER_BYTES:
            return None
        with open(path, "rb") as f:
            data = f.read()
    except OSError:
        return None
    kind = "png" if data[:8] == b"\x89PNG\r\n\x1a\n" else "jpeg" if data[:3] == b"\xff\xd8\xff" else None
    return (data, kind) if kind else None


# ---------- sending

def multipart(fields, files):
    boundary = uuid.uuid4().hex
    parts = []
    for k, v in fields.items():
        parts.append(('--%s\r\nContent-Disposition: form-data; name="%s"\r\n\r\n' % (boundary, k)).encode() + str(v).encode() + b"\r\n")
    for k, (fname, data, ctype) in files.items():
        parts.append(('--%s\r\nContent-Disposition: form-data; name="%s"; filename="%s"\r\nContent-Type: %s\r\n\r\n'
                      % (boundary, k, fname, ctype)).encode() + data + b"\r\n")
    return b"".join(parts) + ("--%s--\r\n" % boundary).encode(), "multipart/form-data; boundary=" + boundary


def request(url, body, ctype, secret=""):
    req = urllib.request.Request(url, data=body, headers={"Content-Type": ctype})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
            return json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        try:
            return json.loads(e.read())
        except ValueError:
            raise SendError("answered %d" % e.code)
    except (OSError, ValueError) as e:
        # A Telegram address holds the bot's token: say what went wrong without it.
        text = str(getattr(e, "reason", e))
        raise SendError(text.replace(secret, "…") if secret else text)


def tg(s, method, params=None, files=None):
    token = str(s.get("telegram_bot_token")).strip()
    url = "%s/bot%s/%s" % (TELEGRAM, token, method)
    if files:
        body, ctype = multipart(params or {}, files)
    else:
        body, ctype = json.dumps(params or {}).encode(), "application/json"
    answer = request(url, body, ctype, token)
    if not answer.get("ok"):
        raise SendError("Telegram: %s" % answer.get("description"))
    return answer.get("result")


def to_telegram(s, chat, title, text, url, pic):
    body = "<b>%s</b>\n%s" % (html.escape(title), html.escape(text))
    if url:
        body += '\n<a href="%s">%s</a>' % (html.escape(url), html.escape(url))
    if pic:
        data, kind = pic
        tg(s, "sendPhoto", {"chat_id": chat, "caption": body[:1024], "parse_mode": "HTML"},
           {"photo": ("poster.%s" % ("jpg" if kind == "jpeg" else kind), data, "image/" + kind)})
    else:
        tg(s, "sendMessage", {"chat_id": chat, "text": body[:4096], "parse_mode": "HTML", "link_preview_options": {"is_disabled": True}})


def to_pushover(s, user, title, text, url, pic):
    fields = {"token": str(s["pushover_token"]).strip(), "user": user, "title": title[:250], "message": text[:1024]}
    if url:
        fields["url"] = url[:512]
    if pic:
        data, kind = pic
        body, ctype = multipart(fields, {"attachment": ("poster.%s" % ("jpg" if kind == "jpeg" else kind), data, "image/" + kind)})
    else:
        body, ctype = urllib.parse.urlencode(fields).encode(), "application/x-www-form-urlencoded"
    answer = request(PUSHOVER, body, ctype)
    if answer.get("status") != 1:
        raise SendError("Pushover: %s" % "; ".join(answer.get("errors") or ["status %s" % answer.get("status")]))


def send(s, title, text, url="", poster=None):
    """Sends to every chat and Pushover user at once. Returns (sent, tried)."""
    pic = poster_bytes(poster)
    targets = []
    if telegram_ready(s):
        targets += [(to_telegram, c["id"]) for c in load().get("chats") or []]
    if pushover_ready(s):
        targets += [(to_pushover, u) for u in as_list(s.get("pushover_users"))]
    done = []

    def one(fn, to):
        try:
            fn(s, to, title, text, url, pic)
            done.append(to)
        except SendError as e:
            log("%s: %s" % (to, e))

    threads = [threading.Thread(target=one, args=t, daemon=True) for t in targets]
    for t in threads:
        t.start()
    end = time.monotonic() + TIMEOUT + 5
    for t in threads:
        t.join(max(0, end - time.monotonic()))
    return len(done), len(targets)


# ---------- events

def lang_of(s):
    return str(s.get("language") or load().get("lang") or "en")


def on_event(event, data, s, server):
    chosen = as_list(s.get("events"))
    lang = lang_of(s)
    if event == "library.item.added":
        if event not in chosen or (data.get("item") or {}).get("type") not in ("movie", "episode"):
            return {"skipped": event}
        # Kept until the scan has finished: one message per scan.
        with locked():
            state = load()
            it = data["item"]
            keep = {k: it.get(k) for k in ("id", "type", "title", "year", "season", "episode", "poster")}
            keep["show"] = {k: (it.get("show") or {}).get(k) for k in ("title", "poster")}
            state.setdefault("waiting", []).append(keep)
            save(state)
        return {"ok": True, "waiting": True}
    if event == "library.scan.finished":
        with locked():
            state = load()
            items, state["waiting"] = state.get("waiting") or [], []
            save(state)
        out = {"ok": True}
        if items and "library.item.added" in chosen:
            title = tr(lang, "New on {name}", name=server.get("name") or "Lichtspiel")
            one = items[0] if len(items) == 1 else None
            out["new"] = send(s, title, "\n".join(new_titles(lang, items)),
                              link(server, one.get("id")) if one else "", poster_of(one) if one else None)[0]
        if event in chosen:
            m = message(lang, event, data, server)
            if m:
                out["sent"] = send(s, m[0], m[1])[0]
        return out
    if event not in chosen:
        return {"skipped": event}
    m = message(lang, event, data, server)
    if not m:
        return {"skipped": event}
    title, text, item = m
    url = link(server, (item or {}).get("id"))
    poster = poster_of(item) if event.startswith("playback.") else None
    return {"ok": True, "sent": send(s, title, text, url, poster)[0]}


# ---------- the admins' panel

def chats_text(lang, n):
    return tr(lang, "1 Telegram chat") if n == 1 else tr(lang, "{n} Telegram chats", n=n)


def panel(lang, s, server):
    out = {"title": tr(lang, "Telegram / Pushover")}
    if not telegram_ready(s) and not pushover_ready(s):
        out["text"] = tr(lang, "Not set up yet: enter a Telegram bot token and/or a Pushover token and user keys in the plugin's settings.")
        return out
    with locked():
        state = load()
        if state.get("lang") != lang:
            state["lang"] = lang
            save(state)
    chats = state.get("chats") or [] if telegram_ready(s) else []
    where = []
    if chats:
        where.append(chats_text(lang, len(chats)))
    if pushover_ready(s):
        n = len(as_list(s.get("pushover_users")))
        where.append(tr(lang, "1 Pushover user") if n == 1 else tr(lang, "{n} Pushover users", n=n))
    names = []
    for e in as_list(s.get("events")):
        w = tr(lang, EVENT_NAMES.get(e, e))
        if w not in names:
            names.append(w)
    out["text"] = tr(lang, "Sends {events} to {where}.", events=", ".join(names) or tr(lang, "nothing yet"),
                     where=", ".join(where) if where else tr(lang, "nowhere yet: connect a Telegram chat"))
    if chats:
        out["items"] = [{"title": c.get("name") or str(c["id"]), "subtitle": tr(lang, "Telegram group" if c.get("group") else "Telegram chat"),
                         "buttons": [{"label": tr(lang, "Remove"), "action": "remove_chat", "args": {"id": c["id"]}}]} for c in chats]
    buttons = []
    if telegram_ready(s):
        buttons.append({"label": tr(lang, "Connect a Telegram chat"), "action": "telegram_link", "primary": not chats})
    if where:
        buttons.append({"label": tr(lang, "Send a test message"), "action": "test"})
    out["buttons"] = buttons
    return out


def waiting(lang, bot, code):
    return {"message": tr(lang, "Open the link (or search for @{bot} in Telegram) and press Start. For a group: add @{bot} to it and send /start@{bot} {code} there.",
                          bot=bot, code=code),
            "code": code, "link": "https://t.me/%s?start=%s" % (bot, code), "poll": {"action": "telegram_poll", "seconds": 4}}


def telegram_link(lang, s, user):
    bot = (tg(s, "getMe") or {}).get("username") or ""
    code = secrets.token_hex(4)
    with locked():
        state = load()
        codes = {c: v for c, v in (state.get("codes") or {}).items() if v.get("user") != user and time.time() - v.get("at", 0) < CODE_MINUTES * 60}
        codes[code] = {"user": user, "at": int(time.time())}
        state["codes"] = codes
        save(state)
    return waiting(lang, bot, code)


def telegram_poll(lang, s, server, user):
    """Looks for "/start <code>" (or "/start@bot <code>" in a group) sent to the bot."""
    connected = False
    with locked():
        state = load()
        updates = tg(s, "getUpdates", {"offset": state.get("tg_offset") or 0, "timeout": 0, "allowed_updates": ["message"]}) or []
        codes = state.get("codes") or {}
        for u in updates:
            state["tg_offset"] = max(state.get("tg_offset") or 0, u.get("update_id", 0) + 1)
            m = u.get("message") or {}
            words = str(m.get("text") or "").split()
            if len(words) != 2 or not words[0].startswith("/start"):
                continue
            c = codes.pop(words[1], None)
            if not c or time.time() - c.get("at", 0) > CODE_MINUTES * 60:
                continue
            chat = m.get("chat") or {}
            name = chat.get("title") or chat.get("username") or chat.get("first_name") or str(chat.get("id"))
            chats = [x for x in state.get("chats") or [] if x.get("id") != chat.get("id")]
            chats.append({"id": chat.get("id"), "name": name, "group": chat.get("type") in ("group", "supergroup")})
            state["chats"] = chats
            connected = connected or c.get("user") == user
            try:
                tg(s, "sendMessage", {"chat_id": chat.get("id"), "text": tr(lang, "Connected. {name} sends its messages here from now on.",
                                                                              name=server.get("name") or "Lichtspiel")})
            except SendError as e:
                log(str(e))
        state["codes"] = codes
        save(state)
        mine = [c for c, v in codes.items() if v.get("user") == user]
    if connected or not mine:
        return {"message": tr(lang, "The chat is connected.")} if connected else {"error": tr(lang, "The code has expired. Try again.")}
    bot = (tg(s, "getMe") or {}).get("username") or ""
    return waiting(lang, bot, mine[0])


def call(action, lang, s, server, user, admin, body):
    if not admin:
        return {"error": tr(lang, "Only admins can do that.")}
    args = body.get("args") if isinstance(body.get("args"), dict) else {}
    if action == "panel":
        return panel(lang, s, server)
    if action == "test":
        name = server.get("name") or "Lichtspiel"
        n, m = send(s, name, tr(lang, "Test message from {name}", name=name))
        out = {"message" if n else "error": tr(lang, "Sent to {n} of {m}.", n=n, m=m)}
        return out
    if action == "telegram_link" and telegram_ready(s):
        return telegram_link(lang, s, user)
    if action == "telegram_poll" and telegram_ready(s):
        return telegram_poll(lang, s, server, user)
    if action == "remove_chat":
        with locked():
            state = load()
            state["chats"] = [c for c in state.get("chats") or [] if c.get("id") != args.get("id")]
            save(state)
        return {"message": tr(lang, "Removed."), "panel": panel(lang, s, server)}
    return {"error": "Unknown action"}


def main():
    # The server sends UTF-8, whatever the console code page is (Windows).
    msg = json.loads(getattr(sys.stdin, "buffer", sys.stdin).read())
    event = msg.get("event", "")
    data = msg.get("data") or {}
    s = msg.get("settings") or {}
    server = msg.get("server") or {}
    try:
        if event == "api.call":
            body = data.get("body") if isinstance(data.get("body"), dict) else {}
            out = call(data.get("action"), body.get("lang") or "en", s, server, data.get("user") or "", bool(data.get("admin")), body)
        elif event in EVENTS:
            out = on_event(event, data, s, server)
        else:
            out = {"skipped": event}
    except SendError as e:
        out = {"error": str(e)}
    # Escaped (ü), so the answer survives any console code page (Windows).
    print(json.dumps(out))


if __name__ == "__main__":
    main()
