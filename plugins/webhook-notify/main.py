"""Webhook notify plugin.

Reads {"event", "data", "settings", "server"} from standard input and posts
a JSON message to every address in settings["urls"].
Its own API action: POST /api/plugins/webhook-notify/call/test sends a test message.
"""
import json
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

# All addresses together get this long; the server stops the plugin after 15 s (plugin.json).
DEADLINE = 12

WORDS = {
    "library.item.added": "New in the library: {title}",
    "playback.started": "{user} started watching item {id}",
    "playback.watched": "{user} finished item {id}",
    "user.invited": "{by} sent an invite",
    "user.joined": "{username} joined the server",
    "library.scan.finished": "Scan finished: {added} added, {removed} removed",
    "api.call": "Test message from Lichtspiel",
}


class NoRedirect(urllib.request.HTTPRedirectHandler):
    """urllib would follow a redirect with a GET and drop the message, so say where it points instead."""
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise urllib.error.HTTPError(req.full_url, code, "redirected to %s; put that address in the settings" % newurl, headers, fp)


OPENER = urllib.request.build_opener(NoRedirect)


def send(urls, body):
    done = []

    def one(url):
        req = urllib.request.Request(url, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
        try:
            OPENER.open(req, timeout=DEADLINE).read()
            done.append(url)
        except Exception as e:  # keep going with the other addresses
            print("could not reach %s: %s" % (url, e), file=sys.stderr)

    # All at once, so one slow address doesn't hold up the others; whatever is still running at
    # the deadline is left behind (daemon threads end with the program).
    threads = []
    for url in urls:
        if urllib.parse.urlsplit(str(url)).scheme not in ("http", "https"):
            print("skipped %s: the address must start with http:// or https://" % url, file=sys.stderr)
            continue
        threads.append(threading.Thread(target=one, args=(url,), daemon=True))
        threads[-1].start()
    end = time.monotonic() + DEADLINE
    for t in threads:
        t.join(max(0, end - time.monotonic()))
    return len(done)


def main():
    msg = json.load(sys.stdin)
    event = msg.get("event", "")
    data = msg.get("data") or {}
    s = msg.get("settings") or {}
    urls = s.get("urls") or []
    if isinstance(urls, str):
        urls = [u.strip() for u in urls.split(",") if u.strip()]
    # Only an admin may send the test message, so other users can't flood the channel with it.
    if event == "api.call" and not data.get("admin"):
        print(json.dumps({"error": "Only an admin can send a test message"}))
        return
    if event != "api.call" and event not in (s.get("events") or []):
        print(json.dumps({"skipped": event}))
        return
    try:
        text = WORDS.get(event, event).format_map(type("D", (dict,), {"__missing__": lambda self, k: "?"})(data))
    except Exception:
        text = event
    # The invite link and code are enough to create an account, and the invited email is someone
    # else's address, so they stay on the server.
    data = {k: v for k, v in data.items() if k not in ("url", "invite", "code", "email")}
    body = {"content": text} if s.get("discord_style") else {"event": event, "text": text, "data": data}
    print(json.dumps({"ok": True, "sent": send(urls, body), "text": text}))


if __name__ == "__main__":
    main()
