"""Tests for the webhook plugin: python3 -I -m unittest discover -s plugins/webhook-notify"""
import io
import json
import os
import sys
import threading
import time
import unittest
from contextlib import redirect_stderr, redirect_stdout
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

SEEN = []  # (method, path, body) of every request the fake endpoint got


class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def answer(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        SEEN.append((self.command, self.path, body))
        if self.path == "/hang":
            time.sleep(5)
        if self.path == "/redir":
            self.send_response(302)
            self.send_header("Location", "/ok")
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Length", "100")
        self.end_headers()
        if self.path == "/drip":  # an answer that trickles out, one byte a second
            for _ in range(100):
                self.wfile.write(b"x")
                self.wfile.flush()
                time.sleep(1)
        else:
            self.wfile.write(b"x" * 100)

    do_GET = do_POST = answer


class SendTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.srv = ThreadingHTTPServer(("127.0.0.1", 0), Fake)
        cls.srv.daemon_threads = True
        threading.Thread(target=cls.srv.serve_forever, daemon=True).start()
        cls.base = "http://127.0.0.1:%d" % cls.srv.server_address[1]

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()
        cls.srv.server_close()

    def setUp(self):
        SEEN.clear()

    def test_address_without_scheme_is_skipped_and_the_others_still_get_it(self):
        err = io.StringIO()
        with redirect_stderr(err):
            sent = main.send(["example.com/hook", self.base + "/ok"], {"text": "hi"})
        self.assertEqual(sent, 1)
        self.assertEqual([p for _, p, _ in SEEN], ["/ok"])
        self.assertIn("example.com/hook", err.getvalue())

    def test_slow_addresses_do_not_hold_up_the_others_or_the_deadline(self):
        start = time.monotonic()
        with redirect_stderr(io.StringIO()), mock.patch.object(main, "DEADLINE", 2):
            sent = main.send([self.base + "/hang", self.base + "/hang", self.base + "/drip", self.base + "/ok"], {"text": "hi"})
        self.assertLess(time.monotonic() - start, 3.5)
        self.assertEqual(sent, 1)
        self.assertIn("/ok", [p for _, p, _ in SEEN])

    def test_deadline_stays_under_the_plugin_time_limit(self):
        with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "plugin.json")) as f:
            limit = json.load(f)["timeout_seconds"]
        self.assertLessEqual(main.DEADLINE, limit - 2)

    def test_a_redirect_is_not_counted_as_sent(self):
        err = io.StringIO()
        with redirect_stderr(err):
            sent = main.send([self.base + "/redir"], {"text": "hi"})
        self.assertEqual(sent, 0)
        self.assertEqual([(m, p) for m, p, _ in SEEN], [("POST", "/redir")])
        self.assertIn("/ok", err.getvalue())


class PrivacyTest(unittest.TestCase):
    def run_main(self, msg):
        got = {}
        out = io.StringIO()
        with mock.patch.object(main, "send", lambda urls, body: got.setdefault("body", body) and 1), \
                mock.patch("sys.stdin", io.StringIO(json.dumps(msg))), redirect_stdout(out):
            main.main()
        return got["body"]

    def test_invite_link_and_code_are_not_sent(self):
        data = {"by": "admin", "email": "a@b.c", "url": "https://x/invite/SECRET", "code": "SECRET", "note": "hi"}
        body = self.run_main({"event": "user.invited", "data": data, "settings": {"urls": ["http://h"], "events": ["user.invited"]}})
        self.assertNotIn("SECRET", json.dumps(body))
        self.assertNotIn("a@b.c", json.dumps(body))
        self.assertEqual(body["data"]["by"], "admin")

    def test_discord_message_does_not_name_the_invited_email(self):
        data = {"by": "admin", "email": "a@b.c", "url": "https://x/invite/SECRET"}
        body = self.run_main({"event": "user.invited", "data": data,
                              "settings": {"urls": ["http://h"], "events": ["user.invited"], "discord_style": True}})
        self.assertNotIn("a@b.c", json.dumps(body))
        self.assertNotIn("SECRET", json.dumps(body))


if __name__ == "__main__":
    unittest.main()
