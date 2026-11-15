"""Tests for the Trakt plugin: python3 -I -m unittest discover -s plugins/trakt"""
import io
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import main  # noqa: E402

SETTINGS = {"client_id": "id", "client_secret": "secret"}


class TokenTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        state = os.path.join(self.tmp.name, "tokens.json")
        for p in (mock.patch.object(main, "STATE", state), mock.patch.object(main, "LOCK", state + ".lock")):
            p.start()
            self.addCleanup(p.stop)
        self.addCleanup(self.tmp.cleanup)

    def state(self, expires_in):
        return {"users": {"ste": {"access_token": "old", "refresh_token": "r1", "expires_at": int(time.time()) + expires_in}},
                "pending": {}, "scrobbled": {}, "last": {}}

    def test_a_fresh_24_hour_token_is_not_refreshed(self):
        st = self.state(86399)
        with mock.patch.object(main, "call") as call:
            self.assertEqual(main.access_token(SETTINGS, st, "ste"), "old")
        call.assert_not_called()

    def test_a_token_about_to_run_out_is_refreshed(self):
        st = self.state(300)
        with mock.patch.object(main, "call", return_value={"access_token": "new", "refresh_token": "r2", "expires_in": 86400}):
            self.assertEqual(main.access_token(SETTINGS, st, "ste"), "new")
        self.assertEqual(main.load()["users"]["ste"]["refresh_token"], "r2")

    def test_a_refresh_token_trakt_no_longer_knows_disconnects(self):
        st = self.state(300)
        err = main.TraktError(400, '{"error":"invalid_grant","error_description":"revoked"}')
        with mock.patch.object(main, "call", side_effect=err):
            self.assertIsNone(main.access_token(SETTINGS, st, "ste"))
        self.assertNotIn("ste", main.load()["users"])

    def test_other_refusals_keep_the_login(self):
        for code, text in ((400, '{"error":"invalid_request"}'), (401, '{"error":"invalid_client"}'), (502, "Bad gateway")):
            st = self.state(300)
            main.save(st)
            with mock.patch.object(main, "call", side_effect=main.TraktError(code, text)):
                with self.assertRaises(main.TraktError):
                    main.access_token(SETTINGS, st, "ste")
            self.assertIn("ste", main.load()["users"], code)


class NetworkTest(unittest.TestCase):
    def test_unreachable_trakt_is_a_trakt_error(self):
        with mock.patch.object(main, "API", "http://127.0.0.1:1"):
            with self.assertRaises(main.TraktError) as cm:
                main.call(SETTINGS, "GET", "/users/settings")
        self.assertEqual(cm.exception.code, 0)

    def test_unreachable_trakt_on_playback_gives_one_line_not_a_traceback(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        # A copy, so the tokens file of this test doesn't land next to the real plugin.
        with open(os.path.join(HERE, "main.py")) as f:
            src = f.read().replace('API = "https://api.trakt.tv"', 'API = "http://127.0.0.1:1"')
        with open(os.path.join(tmp.name, "main.py"), "w") as f:
            f.write(src)
        with open(os.path.join(tmp.name, "tokens.json"), "w") as f:
            json.dump({"users": {"ste": {"access_token": "a", "refresh_token": "r", "expires_at": 0}}}, f)
        msg = {"event": "playback.started", "data": {"user": "ste", "item": {"type": "movie", "title": "X"}}, "settings": SETTINGS}
        r = subprocess.run([sys.executable, "-I", "-B", "main.py"], cwd=tmp.name, input=json.dumps(msg), capture_output=True, text=True, timeout=30)
        self.assertNotEqual(r.returncode, 0)
        self.assertNotIn("Traceback", r.stderr)
        self.assertIn("Trakt", r.stderr)

    def test_unreachable_trakt_on_a_button_gives_a_message(self):
        with tempfile.TemporaryDirectory() as d, mock.patch.object(main, "STATE", os.path.join(d, "t.json")), \
                mock.patch.object(main, "LOCK", os.path.join(d, "t.lock")), mock.patch.object(main, "API", "http://127.0.0.1:1"):
            out = main.on_call({"action": "connect", "user": "ste"}, SETTINGS)
        self.assertIn("error", out)


if __name__ == "__main__":
    unittest.main()
