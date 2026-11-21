"""Tests for the Telegram / Pushover plugin: python3 -I -m unittest discover -s plugins/push-notify"""
import io
import json
import os
import shutil
import sys
import tempfile
import threading
import unittest
import urllib.parse
from contextlib import redirect_stderr, redirect_stdout
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

JPEG = b"\xff\xd8\xff\xe0" + b"x" * 200
SEEN = []     # (service, method, content type, body)
UPDATES = []  # what getUpdates answers


class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def reply(self, obj, code=200):
        raw = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        ctype = self.headers.get("Content-Type") or ""
        if self.path.startswith("/pushover"):
            SEEN.append(("pushover", "", ctype, body))
            if b"badkey" in body:
                return self.reply({"status": 0, "errors": ["user identifier is invalid"]}, 400)
            return self.reply({"status": 1})
        method = self.path.rsplit("/", 1)[1]
        SEEN.append(("telegram", method, ctype, body))
        if method == "getMe":
            return self.reply({"ok": True, "result": {"username": "kino_bot"}})
        if method == "getUpdates":
            offset = json.loads(body).get("offset") or 0
            return self.reply({"ok": True, "result": [u for u in UPDATES if u["update_id"] >= offset]})
        self.reply({"ok": True, "result": {}})


class PushTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.srv = ThreadingHTTPServer(("127.0.0.1", 0), Fake)
        threading.Thread(target=cls.srv.serve_forever, daemon=True).start()
        cls.base = "http://127.0.0.1:%d" % cls.srv.server_port

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()

    def setUp(self):
        SEEN.clear()
        UPDATES.clear()
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir, True)
        self.poster = os.path.join(self.dir, "poster.jpg")
        with open(self.poster, "wb") as f:
            f.write(JPEG)
        for name, value in (("STATE", os.path.join(self.dir, "state.json")), ("LOCK", os.path.join(self.dir, ".lock")),
                            ("TELEGRAM", self.base), ("PUSHOVER", self.base + "/pushover")):
            p = mock.patch.object(main, name, value)
            p.start()
            self.addCleanup(p.stop)
        main.save({"chats": [{"id": 42, "name": "Family", "group": True}], "lang": "de"})
        self.server = {"name": "Kino", "public_url": "https://kino.example", "version": "1.2"}
        self.settings = {"telegram_bot_token": "123:abc", "pushover_token": "apptoken", "pushover_users": ["ukey"],
                         "events": ["library.item.added", "playback.started", "item.problem", "user.joined"], "language": ""}

    def run_main(self, event, data, settings=None):
        stdin = io.TextIOWrapper(io.BytesIO(json.dumps({"event": event, "data": data, "settings": settings or self.settings, "server": self.server}).encode()))
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "stdin", stdin), redirect_stdout(out), redirect_stderr(err):
            main.main()
        self.err = err.getvalue()
        return json.loads(out.getvalue().strip().splitlines()[-1])

    def sent(self, service):
        return [x for x in SEEN if x[0] == service and x[1] not in ("getMe", "getUpdates")]

    def test_playback_with_poster_to_both(self):
        r = self.run_main("playback.started", {"id": 7, "user": "anna", "player": {"device": "Chrome on Windows"},
                                               "item": {"id": 7, "type": "movie", "title": "Heat", "year": 1995, "poster": self.poster}})
        self.assertEqual(r, {"ok": True, "sent": 2})
        (_, method, ctype, body), = self.sent("telegram")
        self.assertEqual(method, "sendPhoto")
        self.assertTrue(ctype.startswith("multipart/form-data"))
        self.assertIn("anna schaut Heat (1995) auf Chrome on Windows".encode(), body)
        self.assertIn(b'https://kino.example/web/#/item/7', body)
        self.assertIn(JPEG, body)
        (_, _, ctype, body), = self.sent("pushover")
        self.assertIn(b'name="attachment"', body)
        self.assertIn(b'name="token"\r\n\r\napptoken', body)

    def test_a_scan_is_one_message(self):
        for i, it in enumerate([{"type": "movie", "title": "Heat", "year": 1995},
                                {"type": "episode", "title": "Pilot", "season": 1, "episode": 1, "show": {"title": "Lost"}},
                                {"type": "episode", "title": "Two", "season": 1, "episode": 2, "show": {"title": "Lost"}},
                                {"type": "episode", "title": "Pilot", "season": 1, "episode": 1, "show": {"title": "Dark"}},
                                {"type": "show", "title": "Lost"}]):
            it["id"] = i + 1
            self.assertNotIn("sent", self.run_main("library.item.added", {"id": i + 1, "item": dict(it, files=[{"path": "/x"}])}))
        self.assertEqual(SEEN, [])
        r = self.run_main("library.scan.finished", {"added": 4, "removed": 0})
        self.assertEqual(r, {"ok": True, "new": 2})  # scan.finished itself is not chosen
        (_, method, _, body), = self.sent("telegram")
        text = json.loads(body)["text"]
        self.assertEqual(method, "sendMessage")
        self.assertEqual(text, "<b>Neu auf Kino</b>\nHeat (1995)\nLost · 2 neue Folgen\nDark S01E01 · Pilot")
        self.assertEqual(main.load()["waiting"], [])
        # The next scan with nothing new says nothing.
        SEEN.clear()
        self.assertEqual(self.run_main("library.scan.finished", {"added": 0, "removed": 0}), {"ok": True})
        self.assertEqual(SEEN, [])

    def test_one_new_title_has_its_poster_and_link(self):
        self.run_main("library.item.added", {"id": 3, "item": {"id": 3, "type": "movie", "title": "Heat", "poster": self.poster}})
        self.run_main("library.scan.finished", {"added": 1})
        (_, method, _, body), = self.sent("telegram")
        self.assertEqual(method, "sendPhoto")
        self.assertIn(b"https://kino.example/web/#/item/3", body)

    def test_events_not_chosen_and_errors(self):
        self.assertEqual(self.run_main("playback.watched", {"user": "anna", "item": {}}), {"skipped": "playback.watched"})
        s = dict(self.settings, pushover_users=["ukey", "badkey"], language="en")
        r = self.run_main("user.joined", {"username": "bob"}, s)
        self.assertEqual(r, {"ok": True, "sent": 2})
        self.assertIn("user identifier is invalid", self.err)
        body = urllib.parse.parse_qs(self.sent("pushover")[0][3].decode())
        self.assertEqual(body["message"], ["bob joined Kino"])
        # The invite's link and address never leave the server.
        r = self.run_main("user.invited", {"by": "admin", "url": "https://secret", "email": "x@y"}, dict(s, events=["user.invited"]))
        self.assertNotIn(b"secret", b"".join(x[3] for x in SEEN))

    def test_a_full_disk(self):
        data = {"holds": ["Filme", "data"], "paths": ["/media/filme", "/config"], "free_bytes": 230_400_000_000,
                "total_bytes": 8_000_000_000_000, "free_percent": 2.9, "threshold_percent": 5}
        # Not in "events" (saved before the warning existed), still sent.
        r = self.run_main("disk.low", data, dict(self.settings, language="de"))
        self.assertEqual(r, {"ok": True, "sent": 2})
        body = urllib.parse.parse_qs(self.sent("pushover")[0][3].decode())
        self.assertEqual(body["message"], ["Platte fast voll: 230,4 GB frei (2,9 %) auf der Platte von Filme, dem Datenordner"])
        SEEN.clear()
        self.assertEqual(self.run_main("disk.low", data, dict(self.settings, disk_warnings=False)), {"skipped": "disk.low"})
        self.assertEqual(self.sent("pushover"), [])

    def test_the_admin_panel(self):
        p = self.run_main("api.call", {"action": "panel", "user": "admin", "admin": True, "body": {"lang": "de"}})
        self.assertEqual(p["text"], "Sendet neue Titel, Wiedergaben, Probleme, neue Benutzer, Warnungen bei voller Platte an 1 Telegram-Chat, 1 Pushover-Empfänger.")
        self.assertEqual(p["items"][0]["title"], "Family")
        self.assertIn("error", self.run_main("api.call", {"action": "panel", "user": "bob", "admin": False, "body": {}}))
        r = self.run_main("api.call", {"action": "test", "user": "admin", "admin": True, "body": {"lang": "en"}})
        self.assertEqual(r, {"message": "Sent to 2 of 2."})
        r = self.run_main("api.call", {"action": "remove_chat", "user": "admin", "admin": True, "body": {"lang": "en", "args": {"id": 42}}})
        self.assertEqual(main.load()["chats"], [])
        self.assertIn("1 Pushover user", r["panel"]["text"])

    def test_connecting_a_group(self):
        r = self.run_main("api.call", {"action": "telegram_link", "user": "admin", "admin": True, "body": {"lang": "en"}})
        code = r["code"]
        self.assertIn("/start@kino_bot " + code, r["message"])
        self.assertIn("poll", self.run_main("api.call", {"action": "telegram_poll", "user": "admin", "admin": True, "body": {}}))
        UPDATES.append({"update_id": 3, "message": {"text": "/start@kino_bot " + code, "chat": {"id": -100, "type": "supergroup", "title": "Movie night"}}})
        r = self.run_main("api.call", {"action": "telegram_poll", "user": "admin", "admin": True, "body": {}})
        self.assertEqual(r, {"message": "The chat is connected."})
        self.assertEqual(main.load()["chats"][-1], {"id": -100, "name": "Movie night", "group": True})


if __name__ == "__main__":
    unittest.main()
