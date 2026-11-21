"""Tests for the digest plugin: python3 -I -m unittest discover -s plugins/new-digest"""
import io
import json
import os
import shutil
import sys
import tempfile
import threading
import unittest
from contextlib import redirect_stderr, redirect_stdout
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

JPEG = b"\xff\xd8\xff\xe0" + b"x" * 200
LIBRARY = {}
TG = []       # (method, body) of every Telegram call
UPDATES = []  # what getUpdates answers
MAILS = []    # every e-mail sent


class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def reply(self, obj):
        raw = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        self.reply(LIBRARY)

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        method = self.path.rsplit("/", 1)[1]
        TG.append((method, body))
        if method == "getMe":
            return self.reply({"ok": True, "result": {"username": "lichtspiel_bot"}})
        if method == "getUpdates":
            offset = json.loads(body).get("offset") or 0
            return self.reply({"ok": True, "result": [u for u in UPDATES if u["update_id"] >= offset]})
        self.reply({"ok": True, "result": {}})


class FakeSMTP:
    def __init__(self, host, port, timeout=None, context=None):
        self.host, self.port, self.tls, self.user = host, port, False, None

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def starttls(self, context=None):
        self.tls = True

    def login(self, user, password):
        self.user = user

    def send_message(self, msg):
        MAILS.append((self, msg))


class DigestTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.srv = ThreadingHTTPServer(("127.0.0.1", 0), Fake)
        threading.Thread(target=cls.srv.serve_forever, daemon=True).start()
        cls.base = "http://127.0.0.1:%d" % cls.srv.server_port

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()

    def setUp(self):
        TG.clear()
        UPDATES.clear()
        MAILS.clear()
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir, True)
        poster = os.path.join(self.dir, "poster.jpg")
        with open(poster, "wb") as f:
            f.write(JPEG)
        for name, value in (("STATE", os.path.join(self.dir, "state.json")), ("LOCK", os.path.join(self.dir, ".lock")),
                            ("TELEGRAM", self.base), ("smtplib", mock.Mock(SMTP=FakeSMTP, SMTP_SSL=FakeSMTP, SMTPException=Exception))):
            p = mock.patch.object(main, name, value)
            p.start()
            self.addCleanup(p.stop)
        LIBRARY.clear()
        LIBRARY.update({"sections": [{"id": 1, "name": "Movies"}, {"id": 2, "name": "Shows"}, {"id": 3, "name": "Kids"}], "items": [
            {"id": 10, "type": "movie", "title": "Heat", "year": 1995, "section_id": 1, "poster": poster},
            {"id": 11, "type": "movie", "title": "Cars", "year": 2006, "section_id": 3, "poster": poster},
            {"id": 12, "type": "movie", "title": "Heat (4K)", "year": 1995, "section_id": 1, "version_of": 10},
            {"id": 20, "type": "episode", "title": "Pilot", "season": 1, "episode": 1, "section_id": 2, "show": {"id": 5, "title": "Lost", "year": 2004, "poster": poster}},
            {"id": 21, "type": "episode", "title": "Two", "season": 1, "episode": 2, "section_id": 2, "show": {"id": 5, "title": "Lost", "year": 2004, "poster": poster}},
            {"id": 22, "type": "episode", "title": "Four", "season": 1, "episode": 4, "section_id": 2, "show": {"id": 5, "title": "Lost", "year": 2004, "poster": None}},
        ]})
        self.server = {"library": self.base + "/library?key=k", "name": "Kino", "public_url": "https://kino.example"}
        self.settings = {"every": "week", "weekday": "friday", "skip_libraries": ["kids"], "smtp_host": "mail.example", "smtp_port": 587,
                         "smtp_security": "starttls", "smtp_user": "me", "smtp_password": "pw", "smtp_from": "kino@example.com",
                         "telegram_bot_token": "123:abc"}

    def run_main(self, event, data, settings=None):
        stdin = io.TextIOWrapper(io.BytesIO(json.dumps({"event": event, "data": data, "settings": settings or self.settings, "server": self.server}).encode()))
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "stdin", stdin), redirect_stdout(out), redirect_stderr(err):
            main.main()
        self.err = err.getvalue()
        return json.loads(out.getvalue().strip().splitlines()[-1])

    def call(self, action, user="anna", admin=False, **body):
        body.setdefault("lang", "de")
        return self.run_main("api.call", {"action": action, "user": user, "admin": admin, "body": body})

    def add_all(self):
        for i in (10, 11, 12, 20, 21, 22):
            it = next(x for x in LIBRARY["items"] if x["id"] == i)
            self.run_main("library.item.added", {"id": i, "item": it})
        self.run_main("library.item.added", {"id": 5, "item": {"id": 5, "type": "show"}})  # a show itself is not noted

    def test_due(self):
        s = {"weekday": "Freitag"}
        self.assertTrue(main.due(s, "2026-11-20", ""))       # a Friday
        self.assertFalse(main.due(s, "2026-11-20", "2026-11-20"))
        self.assertFalse(main.due(s, "2026-11-21", "2026-11-14"))  # a Saturday, a week after the last
        self.assertTrue(main.due(s, "2026-11-21", "2026-11-13"))   # the Friday was missed: now
        self.assertFalse(main.due(s, "2026-11-19", "2026-11-13"))
        self.assertTrue(main.due({"every": "day"}, "2026-11-19", "2026-11-18"))

    def test_episodes_text(self):
        self.assertEqual(main.episodes_text([(2, 1), (2, 2), (2, 3), (2, 5), (3, 1)]), "S02E01–E03, S02E05, S03E01")

    def test_the_weekly_mail_and_telegram(self):
        self.call("email", query="anna@example.com")
        self.call("email", user="bob", query="bob@example.com", lang="en")
        self.call("email", user="carl", query="carl@example.com")
        self.call("email_off", user="carl")
        st = main.load()
        st["people"]["dora"] = {"telegram": 77, "lang": "de"}
        main.save(st)
        self.add_all()
        self.assertEqual(len(main.load()["pending"]), 6)
        self.assertEqual(self.run_main("schedule.daily", {"date": "2026-11-19"}), {"ok": True, "due": False})
        self.assertEqual(self.run_main("schedule.daily", {"date": "2026-11-20"}), {"ok": True, "sent_to": 3})
        self.assertEqual(sorted(m["To"] for _, m in MAILS), ["anna@example.com", "bob@example.com"])
        smtp, de = next((s, m) for s, m in MAILS if m["To"] == "anna@example.com")
        self.assertTrue(smtp.tls)
        self.assertEqual(smtp.user, "me")
        self.assertEqual(de["Subject"], "Neu auf Kino: 1 Film, 1 Serie")
        html = de.get_body(("html",)).get_content()
        self.assertIn("Heat", html)
        self.assertNotIn("Cars", html)       # in a library left out
        self.assertNotIn("Heat (4K)", html)  # a version of Heat
        self.assertIn("3 neue Folgen · S01E01–E02, S01E04", html)
        self.assertIn("https://kino.example/web/#/item/10", html)
        self.assertEqual(html.count("cid:"), 2)  # both posters inside
        self.assertEqual(len([p for p in de.walk() if p.get_content_type() == "image/jpeg"]), 2)
        en = next(m for _, m in MAILS if m["To"] == "bob@example.com")
        self.assertEqual(en["Subject"], "New on Kino: 1 movie, 1 show")
        # Telegram: an album with both posters, then the text.
        self.assertEqual([m for m, _ in TG], ["sendMediaGroup", "sendMessage"])
        text = json.loads(TG[1][1])
        self.assertEqual(text["chat_id"], 77)
        self.assertIn('<a href="https://kino.example/web/#/item/10">Heat (1995)</a>', text["text"])
        # The list starts again, and the week is done.
        st = main.load()
        self.assertEqual((st["pending"], st["last_sent"]), ([], "2026-11-20"))
        MAILS.clear()
        self.assertEqual(self.run_main("schedule.daily", {"date": "2026-11-27"}), {"ok": True, "sent_to": 0})
        self.assertEqual(MAILS, [])

    def test_connecting_telegram(self):
        r = self.call("telegram_link")
        code = r["code"]
        self.assertEqual(r["link"], "https://t.me/lichtspiel_bot?start=" + code)
        self.assertEqual(r["poll"]["action"], "telegram_poll")
        # Not yet: keep waiting.
        self.assertIn("poll", self.call("telegram_poll"))
        UPDATES.append({"update_id": 5, "message": {"text": "/start wrong", "chat": {"id": 1}}})
        UPDATES.append({"update_id": 6, "message": {"text": "/start " + code, "chat": {"id": 99}, "from": {"username": "anna_tg"}}})
        r = self.call("telegram_poll")
        self.assertEqual(r["message"], "Telegram ist verbunden.")
        self.assertIn("Telegram (anna_tg)", r["panel"]["text"])
        self.assertEqual(json.loads(TG[-1][1])["chat_id"], 99)  # told in Telegram too
        st = main.load()
        self.assertEqual((st["people"]["anna"]["telegram"], st["tg_offset"]), (99, 7))
        r = self.call("telegram_off")
        self.assertIn("Telegram verbinden", [b["label"] for b in r["panel"]["buttons"]])

    def test_panel_and_preview(self):
        p = self.call("panel")
        self.assertIn("jeden Freitag", p["text"])
        self.assertIn("Trag deine E-Mail-Adresse ein oder verbinde Telegram", p["text"])
        self.assertNotIn("Jetzt an alle senden", [b["label"] for b in p["buttons"]])
        self.assertEqual(self.call("email", query="nope")["error"], "Das ist keine E-Mail-Adresse.")
        self.assertIn("error", self.call("preview"))  # nothing new yet
        self.call("email", query="anna@example.com")
        self.add_all()
        self.assertEqual(self.call("preview"), {"message": "Gesendet: e-mail."})
        self.assertEqual(len(MAILS), 1)
        # A preview leaves the list as it is.
        self.assertEqual(len(main.load()["pending"]), 6)
        self.assertIn("error", self.call("send_now"))
        r = self.call("send_now", user="admin", admin=True)
        self.assertEqual(r["message"], "An 1 Personen gesendet.")
        self.assertIn("Bisher nichts Neues.", r["panel"]["text"])

    def test_not_set_up(self):
        p = self.run_main("api.call", {"action": "panel", "user": "anna", "body": {}}, {"smtp_host": "", "telegram_bot_token": ""})
        self.assertIn("Not set up yet", p["text"])
        self.assertNotIn("input", p)


if __name__ == "__main__":
    unittest.main()
