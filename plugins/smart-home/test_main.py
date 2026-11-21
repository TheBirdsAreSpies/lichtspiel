"""Tests for the smart home plugin: python3 -I -m unittest discover -s plugins/smart-home"""
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

SEEN = []  # (path, auth header, body) of every request the fake Home Assistant got


class FakeHA(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"null")
        SEEN.append((self.path, self.headers.get("Authorization"), body))
        code = 401 if self.path.endswith("/scene/turn_on") and self.headers.get("Authorization") != "Bearer tok" else 200
        self.send_response(code)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"[]")


class SmartHomeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.srv = ThreadingHTTPServer(("127.0.0.1", 0), FakeHA)
        threading.Thread(target=cls.srv.serve_forever, daemon=True).start()
        cls.base = "http://127.0.0.1:%d" % cls.srv.server_port

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()

    def setUp(self):
        SEEN.clear()
        self.dir = tempfile.mkdtemp()
        for name, value in (("STATE", os.path.join(self.dir, "state.json")), ("LOCK", os.path.join(self.dir, ".lock"))):
            p = mock.patch.object(main, name, value)
            p.start()
            self.addCleanup(p.stop)
        self.addCleanup(shutil.rmtree, self.dir, True)
        self.settings = {"home_assistant_url": self.base + "/", "home_assistant_token": "tok", "rules": [
            {"name": "Living room", "players": ["Samsung"], "types": ["movie", "episode"],
             "play": "scene.movie", "pause": "script.pause_lights", "stop": "webhook:lights on"},
            {"name": "Anna", "users": ["anna"], "play": self.base + "/hook"},
        ]}

    def run_main(self, event, data, settings=None):
        stdin = io.TextIOWrapper(io.BytesIO(json.dumps({"event": event, "data": data, "settings": settings or self.settings}).encode()))
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "stdin", stdin), redirect_stdout(out), redirect_stderr(err):
            main.main()
        return json.loads(out.getvalue().strip().splitlines()[-1])

    def play(self, event, state="playing", user="bob", device="Plex for Samsung — TV 2020", kind="movie"):
        return self.run_main(event, {"id": 1, "user": user, "state": state, "player": {"device": device},
                                     "item": {"type": kind, "title": "Heat", "year": 1995}})

    def test_start_pause_stop_in_the_living_room(self):
        self.assertEqual(self.play("playback.started")["ran"], ["Living room"])
        self.play("playback.progress", state="paused")
        self.play("playback.progress", state="playing")
        self.play("playback.stopped", state="stopped")
        paths = [p for p, _, _ in SEEN]
        self.assertEqual(paths, ["/api/services/scene/turn_on", "/api/services/script/turn_on", "/api/services/scene/turn_on", "/api/webhook/lights%20on"])
        self.assertEqual(SEEN[0][1], "Bearer tok")
        self.assertEqual(SEEN[0][2], {"entity_id": "scene.movie"})
        # The webhook gets the event, without the token.
        self.assertIsNone(SEEN[3][1])
        self.assertEqual(SEEN[3][2]["event"], "stop")
        self.assertEqual(SEEN[3][2]["title"], "Heat")

    def test_other_players_people_and_kinds_are_left_alone(self):
        self.assertEqual(self.play("playback.started", device="Chrome on Windows")["ran"], [])
        self.assertEqual(self.play("playback.started", kind="track")["ran"], [])
        self.assertEqual(SEEN, [])
        # A rule for a person works on every player.
        self.assertEqual(self.play("playback.started", user="Anna", device="Chrome on Windows")["ran"], ["Anna"])
        self.assertEqual(SEEN[0][0], "/hook")

    def test_a_person_can_switch_it_off_for_themselves(self):
        r = self.run_main("api.call", {"action": "toggle", "user": "bob", "body": {"lang": "de"}})
        self.assertEqual(r["message"], "Für dich ausgeschaltet.")
        self.assertIn("Für mich einschalten", [b["label"] for b in r["panel"]["buttons"]])
        self.assertIn("skipped", self.play("playback.started"))
        self.assertEqual(SEEN, [])
        self.run_main("api.call", {"action": "toggle", "user": "bob", "body": {}})
        self.assertEqual(self.play("playback.started")["ran"], ["Living room"])

    def test_the_admin_sees_the_players_and_can_try_actions(self):
        self.play("playback.started", device="Chrome on Windows")
        self.play("playback.started")
        p = self.run_main("api.call", {"action": "panel", "user": "admin", "admin": True, "body": {}})
        self.assertEqual([i["title"] for i in p["items"]], ["Plex for Samsung — TV 2020", "Chrome on Windows"])
        self.assertEqual(p["items"][0]["status"], "in rule “Living room, Anna”")
        self.assertEqual(p["items"][1]["status"], "in rule “Anna”")  # Anna's rule has no players: all fit
        tests = [b for b in p["buttons"] if b["action"] == "test"]
        self.assertEqual(len(tests), 4)
        SEEN.clear()
        r = self.run_main("api.call", {"action": "test", "user": "admin", "admin": True, "body": {"args": tests[0]["args"]}})
        self.assertEqual(r, {"message": "It went through."})
        self.assertEqual(SEEN[0][2], {"entity_id": "scene.movie"})
        # Not for everyone, and nothing for others to see.
        self.assertIn("error", self.run_main("api.call", {"action": "test", "user": "bob", "body": {"args": tests[0]["args"]}}))
        self.assertNotIn("items", self.run_main("api.call", {"action": "panel", "user": "bob", "body": {}}))

    def test_a_wrong_token_or_action_is_logged_not_fatal(self):
        s = dict(self.settings, home_assistant_token="wrong")
        self.assertEqual(self.run_main("playback.started", {"user": "bob", "state": "playing", "player": {"device": "Samsung"}, "item": {"type": "movie"}}, s)["ran"], [])
        s = dict(self.settings, rules=[{"play": "light.kitchen"}])
        self.assertEqual(self.run_main("playback.started", {"user": "bob", "player": {}, "item": {}}, s)["ran"], [])

    def test_not_set_up(self):
        p = self.run_main("api.call", {"action": "panel", "user": "bob", "body": {}}, {"rules": []})
        self.assertIn("not set up", p["text"])


if __name__ == "__main__":
    unittest.main()
