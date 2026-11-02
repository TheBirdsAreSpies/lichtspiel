"""Webhook notify plugin.

Reads {"event", "data", "settings", "server"} from standard input and posts
a JSON message to every address in settings["urls"].
Its own API action: POST /api/plugins/webhook-notify/call/test sends a test message.
"""
import json
import sys
import urllib.request

WORDS = {
    "library.item.added": "New in the library: {title}",
    "playback.started": "{user} started watching item {id}",
    "playback.watched": "{user} finished item {id}",
    "user.invited": "{by} invited {email}",
    "user.joined": "{username} joined the server",
    "library.scan.finished": "Scan finished: {added} added, {removed} removed",
    "api.call": "Test message from the media server",
}


def send(urls, body):
    sent = 0
    for url in urls:
        req = urllib.request.Request(url, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
        try:
            urllib.request.urlopen(req, timeout=10).read()
            sent += 1
        except Exception as e:  # keep going with the other addresses
            print("could not reach %s: %s" % (url, e), file=sys.stderr)
    return sent


def main():
    msg = json.load(sys.stdin)
    event = msg.get("event", "")
    data = msg.get("data") or {}
    s = msg.get("settings") or {}
    urls = s.get("urls") or []
    if isinstance(urls, str):
        urls = [u.strip() for u in urls.split(",") if u.strip()]
    if event != "api.call" and event not in (s.get("events") or []):
        print(json.dumps({"skipped": event}))
        return
    try:
        text = WORDS.get(event, event).format_map(type("D", (dict,), {"__missing__": lambda self, k: "?"})(data))
    except Exception:
        text = event
    body = {"content": text} if s.get("discord_style") else {"event": event, "text": text, "data": data}
    print(json.dumps({"ok": True, "sent": send(urls, body), "text": text}))


if __name__ == "__main__":
    main()
