"""Smart home plugin.

When playback starts, pauses, plays on or stops, the rules in the settings that fit (player, person,
kind of title) run their action for it: a Home Assistant scene, script or automation, a Home
Assistant webhook, or any http(s):// address, which gets the event as JSON.

Each person has a panel under Settings to switch it off for themselves. Admins also see the players
seen lately (their names go into a rule's "players") and can try each rule's actions.

What it remembers (who switched it off, the players seen) is kept in state.json in the plugin's folder.
"""
import contextlib
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
STATE = os.path.join(HERE, "state.json")
LOCK = os.path.join(HERE, ".state.lock")
TIMEOUT = 8
KEEP_PLAYERS = 20

WORDS = {
    "Smart home": "Smart Home",
    "The lights follow what you watch: {rules}.": "Das Licht folgt dem, was du schaust: {rules}.",
    "Smart home is not set up yet: an admin adds rules in the plugin's settings.": "Smart Home ist noch nicht eingerichtet: Ein Admin legt Regeln in den Einstellungen des Plugins an.",
    "It is switched off for you.": "Für dich ist es ausgeschaltet.",
    "Switch off for me": "Für mich ausschalten",
    "Switch on for me": "Für mich einschalten",
    "Switched off for you.": "Für dich ausgeschaltet.",
    "Switched on for you.": "Für dich eingeschaltet.",
    "No player has played anything yet; the players appear here once they do.": "Noch hat kein Player etwas abgespielt; die Player erscheinen hier, sobald einer es tut.",
    "in rule “{name}”": "in Regel „{name}“",
    "in no rule": "in keiner Regel",
    "Try “{what}”": "„{what}“ testen",
    "play": "Start",
    "pause": "Pause",
    "stop": "Stopp",
    "It went through.": "Hat geklappt.",
    "It did not go through; the plugin's log on the Plugins page says why.": "Hat nicht geklappt; das Protokoll des Plugins auf der Seite Plugins sagt, warum.",
    "Only an admin can try the actions.": "Nur ein Admin kann die Aktionen testen.",
    "No such rule.": "Diese Regel gibt es nicht.",
    "just now": "gerade eben",
    "{n} min ago": "vor {n} Min.",
    "{n} h ago": "vor {n} Std.",
    "{n} days ago": "vor {n} Tagen",
}


def tr(lang, text, **kw):
    if (lang or "").startswith("de"):
        text = WORDS.get(text, text)
    return text.format(**kw) if kw else text


def log(text):
    print(text, file=sys.stderr)


# ---------- what it remembers (several runs may happen at once, so with a lock)

@contextlib.contextmanager
def locked():
    end = time.time() + 5
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
                break  # better a lost note than a late light
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


# ---------- the rules

STEPS = ("play", "pause", "stop")


def step_of(event, data):
    """play, pause or stop for this event (None when nothing should happen)."""
    if event == "playback.started":
        return "pause" if data.get("state") == "paused" else "play"
    if event == "playback.progress":
        return {"paused": "pause", "playing": "play"}.get(data.get("state"))
    if event == "playback.stopped":
        return "stop"
    return None


def as_list(v):
    if isinstance(v, str):
        return [x.strip() for x in v.split(",") if x.strip()]
    return [str(x).strip() for x in (v or []) if str(x).strip()]


def rules(settings):
    return [r for r in (settings.get("rules") or []) if isinstance(r, dict)]


def fits(rule, user, device, kind):
    players = [p.lower() for p in as_list(rule.get("players"))]
    if players and not any(p in (device or "").lower() for p in players):
        return False
    users = [u.lower() for u in as_list(rule.get("users"))]
    if users and (user or "").lower() not in users:
        return False
    types = as_list(rule.get("types"))
    if types and kind not in types:
        return False
    return True


def rule_name(rule, i):
    return str(rule.get("name") or "").strip() or "#%d" % (i + 1)


# ---------- the actions

def post(url, body, token=""):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    req = urllib.request.Request(url, data=json.dumps(body).encode(), headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
        r.read()


def run_action(settings, action, body):
    """Runs one action. True if it went through."""
    action = str(action or "").strip()
    if not action:
        return False
    base = str(settings.get("home_assistant_url") or "").strip().rstrip("/")
    token = str(settings.get("home_assistant_token") or "").strip()
    try:
        if action.startswith(("http://", "https://")):
            post(action, body)
            return True
        if urllib.parse.urlsplit(base).scheme not in ("http", "https"):
            log("%s: the Home Assistant address is missing or does not start with http:// or https://" % action)
            return False
        if action.startswith("webhook:"):
            post(base + "/api/webhook/" + urllib.parse.quote(action[8:].strip(), safe=""), body)
            return True
        domain = action.split(".", 1)[0]
        if domain not in ("scene", "script", "automation") or "." not in action:
            log("%s: an action is scene.…, script.…, automation.…, webhook:… or an http(s):// address" % action)
            return False
        if not token:
            log("%s: the Home Assistant token is missing" % action)
            return False
        service = "trigger" if domain == "automation" else "turn_on"
        post("%s/api/services/%s/%s" % (base, domain, service), {"entity_id": action}, token)
        return True
    except urllib.error.HTTPError as e:
        log("%s: Home Assistant answered %d %s" % (action, e.code, e.reason))
    except (OSError, ValueError) as e:
        log("%s: %s" % (action, e))
    return False


def event_body(step, data):
    item = data.get("item") or {}
    player = data.get("player") or {}
    show = (item.get("show") or {}).get("title")
    return {"event": step, "user": data.get("user") or "", "player": player.get("device") or "",
            "type": item.get("type") or "", "title": item.get("title") or "", "show": show or "",
            "year": item.get("year"), "progress": data.get("progress")}


def playback(event, data, settings):
    step = step_of(event, data)
    user = data.get("user") or ""
    device = (data.get("player") or {}).get("device") or ""
    kind = (data.get("item") or {}).get("type") or ""
    with locked():
        state = load()
        if device:
            # The one seen last first.
            seen = [(d, v) for d, v in (state.get("players") or {}).items() if d != device]
            state["players"] = dict([(device, {"user": user, "at": int(time.time())})] + seen[:KEEP_PLAYERS - 1])
            save(state)
    if not step:
        return {"skipped": event}
    if user in (state.get("off") or []):
        return {"skipped": "switched off for %s" % user}
    body = event_body(step, data)
    ran = []
    for i, rule in enumerate(rules(settings)):
        if fits(rule, user, device, kind) and str(rule.get(step) or "").strip():
            if run_action(settings, rule[step], body):
                ran.append(rule_name(rule, i))
    return {"ok": True, "step": step, "ran": ran}


# ---------- the panel

def ago(lang, at):
    s = max(0, int(time.time()) - int(at or 0))
    if s < 60:
        return tr(lang, "just now")
    if s < 3600:
        return tr(lang, "{n} min ago", n=s // 60)
    if s < 86400:
        return tr(lang, "{n} h ago", n=s // 3600)
    return tr(lang, "{n} days ago", n=s // 86400)


def panel(lang, settings, user, admin):
    rs = rules(settings)
    out = {"title": tr(lang, "Smart home")}
    if not rs:
        out["text"] = tr(lang, "Smart home is not set up yet: an admin adds rules in the plugin's settings.")
    else:
        off = user in (load().get("off") or [])
        names = ", ".join(rule_name(r, i) for i, r in enumerate(rs))
        out["text"] = tr(lang, "The lights follow what you watch: {rules}.", rules=names)
        if off:
            out["text"] += " " + tr(lang, "It is switched off for you.")
        out["buttons"] = [{"label": tr(lang, "Switch on for me" if off else "Switch off for me"), "action": "toggle", "primary": off}]
    if admin:
        items = []
        for device, seen in (load().get("players") or {}).items():
            inside = [rule_name(r, i) for i, r in enumerate(rs)
                      if not as_list(r.get("players")) or any(p.lower() in device.lower() for p in as_list(r.get("players")))]
            items.append({"title": device, "subtitle": "%s · %s" % (seen.get("user") or "?", ago(lang, seen.get("at"))),
                          "status": tr(lang, "in rule “{name}”", name=", ".join(inside)) if inside else tr(lang, "in no rule"),
                          "status_kind": "default" if inside else ""})
        for i, r in enumerate(rs):
            for step in STEPS:
                if str(r.get(step) or "").strip():
                    out.setdefault("buttons", []).append({"label": tr(lang, "Try “{what}”", what="%s: %s" % (rule_name(r, i), tr(lang, step))),
                                                          "action": "test", "args": {"rule": i, "step": step}})
        if items:
            out["items"] = items
        else:
            out["text"] = (out.get("text") or "") + " " + tr(lang, "No player has played anything yet; the players appear here once they do.")
    return out


def toggle(user):
    with locked():
        state = load()
        off = [u for u in (state.get("off") or []) if u != user]
        now_off = len(off) == len(state.get("off") or [])
        if now_off:
            off.append(user)
        state["off"] = off
        save(state)
    return now_off


def test(lang, settings, user, args):
    rs = rules(settings)
    i, step = args.get("rule"), args.get("step")
    if not isinstance(i, int) or not 0 <= i < len(rs) or step not in STEPS:
        return {"error": tr(lang, "No such rule.")}
    body = {"event": step, "user": user, "player": "test", "type": "movie", "title": "Lichtspiel test", "show": "", "year": None, "progress": 0, "test": True}
    if run_action(settings, rs[i].get(step), body):
        return {"message": tr(lang, "It went through.")}
    return {"error": tr(lang, "It did not go through; the plugin's log on the Plugins page says why.")}


def main():
    # The server sends UTF-8, whatever the console code page is (Windows).
    msg = json.loads(getattr(sys.stdin, "buffer", sys.stdin).read())
    event = msg.get("event", "")
    data = msg.get("data") or {}
    settings = msg.get("settings") or {}
    if event.startswith("playback."):
        print(json.dumps(playback(event, data, settings)))
        return
    if event != "api.call":
        print(json.dumps({"skipped": event}))
        return
    body = data.get("body") if isinstance(data.get("body"), dict) else {}
    lang, user, admin = body.get("lang") or "en", data.get("user") or "", bool(data.get("admin"))
    action = data.get("action")
    args = body.get("args") if isinstance(body.get("args"), dict) else {}
    if action == "panel":
        out = panel(lang, settings, user, admin)
    elif action == "toggle" and user:
        now_off = toggle(user)
        out = {"message": tr(lang, "Switched off for you." if now_off else "Switched on for you."), "panel": panel(lang, settings, user, admin)}
    elif action == "test":
        out = test(lang, settings, user, args) if admin else {"error": tr(lang, "Only an admin can try the actions.")}
    else:
        out = {"error": "Unknown action"}
    # Escaped (ü), so the answer survives any console code page (Windows).
    print(json.dumps(out))


if __name__ == "__main__":
    main()
