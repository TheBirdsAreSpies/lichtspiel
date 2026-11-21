"""Tests for the SIMKL / MyAnimeList plugin: python3 -I -m unittest discover -s plugins/scrobble-more"""
import io
import json
import os
import shutil
import sys
import tempfile
import threading
import time
import unittest
import urllib.parse
from contextlib import redirect_stderr, redirect_stdout
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

SEEN = []  # (method, path, headers, body)
PIN_OK = []
# MyAnimeList: 100 = season 1 (12 episodes) → 200 = season 2 (12) → 300 = season 3 (13)
ANIME = {100: {"num_episodes": 12, "sequel": 200}, 200: {"num_episodes": 12, "sequel": 300}, 300: {"num_episodes": 13}, 900: {"num_episodes": 1}}
LIST = {}  # anime id → my_list_status


class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def reply(self, obj, code=200):
        raw = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def handle_any(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0)).decode()
        SEEN.append((self.command, self.path, dict(self.headers), body))
        path = urllib.parse.urlsplit(self.path).path
        if path == "/simkl/oauth/pin":
            return self.reply({"result": "OK", "user_code": "ABCDE", "verification_url": "https://simkl.com/pin/", "expires_in": 900, "interval": 5})
        if path == "/simkl/oauth/pin/ABCDE":
            return self.reply({"result": "OK", "access_token": "simkltok"} if PIN_OK else {"result": "KO", "message": "Authorization pending"})
        if path == "/simkl/users/settings":
            return self.reply({"user": {"name": "Anna S"}})
        if path == "/simkl/sync/history":
            if self.headers.get("Authorization") == "Bearer expired":
                return self.reply({"error": "user_token_failed"}, 401)
            b = json.loads(body)
            if b.get("movies") and b["movies"][0]["ids"].get("imdb") == "tt0000000":
                return self.reply({"added": {"movies": 0}, "not_found": {"movies": b["movies"]}})
            return self.reply({"added": {"movies": len(b.get("movies") or []), "episodes": 1}, "not_found": {"movies": [], "shows": []}})
        if path == "/malauth/token":
            f = urllib.parse.parse_qs(body)
            if f["grant_type"] == ["authorization_code"] and f["code"] == ["thecode"] and f.get("code_verifier"):
                return self.reply({"access_token": "malaccess", "refresh_token": "malrefresh", "expires_in": 2678400})
            if f["grant_type"] == ["refresh_token"] and f["refresh_token"] == ["malrefresh"]:
                return self.reply({"access_token": "malaccess2", "refresh_token": "malrefresh2", "expires_in": 2678400})
            return self.reply({"error": "invalid_grant"}, 400)
        if path == "/mal/users/@me":
            return self.reply({"name": "anna_mal"})
        parts = path.split("/")
        if parts[1:3] == ["mal", "anime"]:
            aid = int(parts[3])
            if self.command == "PATCH":
                f = {k: v[0] for k, v in urllib.parse.parse_qs(body).items()}
                LIST[aid] = {"status": f["status"], "num_episodes_watched": int(f["num_watched_episodes"])}
                return self.reply(LIST[aid])
            a = ANIME[aid]
            out = {"id": aid, "num_episodes": a["num_episodes"], "related_anime": []}
            if a.get("sequel"):
                out["related_anime"] = [{"node": {"id": 999}, "relation_type": "side_story"}, {"node": {"id": a["sequel"]}, "relation_type": "sequel"}]
            if aid in LIST:
                out["my_list_status"] = LIST[aid]
            return self.reply(out)
        self.reply({"error": "not found"}, 404)

    do_GET = do_POST = do_PATCH = handle_any


class ScrobbleTest(unittest.TestCase):
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
        PIN_OK.clear()
        LIST.clear()
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir, True)
        for name, value in (("STATE", os.path.join(self.dir, "state.json")), ("LOCK", os.path.join(self.dir, ".lock")),
                            ("SIMKL", self.base + "/simkl"), ("MAL_AUTH", self.base + "/malauth"), ("MAL_API", self.base + "/mal")):
            p = mock.patch.object(main, name, value)
            p.start()
            self.addCleanup(p.stop)
        self.settings = {"simkl_client_id": "skid", "mal_client_id": "malid", "mal_client_secret": "", "mal_redirect_url": "https://kino.example/web/"}

    def run_main(self, event, data, settings=None):
        stdin = io.TextIOWrapper(io.BytesIO(json.dumps({"event": event, "data": data, "settings": settings or self.settings}).encode()))
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "stdin", stdin), redirect_stdout(out), redirect_stderr(err):
            main.main()
        self.err = err.getvalue()
        return json.loads(out.getvalue().strip().splitlines()[-1])

    def call(self, action, user="anna", **body):
        body.setdefault("lang", "en")
        return self.run_main("api.call", {"action": action, "user": user, "body": body})

    def episode(self, season, ep, user="anna"):
        return self.run_main("playback.watched", {"user": user, "item": {"type": "episode", "season": season, "episode": ep, "title": "x",
                                                                         "show": {"title": "Frieren", "year": 2023, "ids": {"tvdb": 424536, "mal": 100}}}})

    def connect_mal(self):
        r = self.call("mal_link")
        url = urllib.parse.urlsplit(r["panel"]["link"])
        q = urllib.parse.parse_qs(url.query)
        self.assertEqual((q["client_id"], q["code_challenge_method"], q["redirect_uri"]), (["malid"], ["plain"], ["https://kino.example/web/"]))
        return self.call("mal_code", query="https://kino.example/web/?code=thecode&state=" + q["state"][0])

    def test_connecting_simkl_and_marking(self):
        r = self.call("simkl_link")
        self.assertEqual((r["code"], r["poll"]["action"]), ("ABCDE", "simkl_poll"))
        self.assertIn("poll", self.call("simkl_poll"))  # not entered yet
        PIN_OK.append(1)
        self.assertEqual(self.call("simkl_poll"), {"message": "SIMKL is connected."})
        p = self.call("panel", lang="de")
        self.assertEqual(p["items"][0]["status"], "verbunden als Anna S")
        SEEN.clear()
        r = self.run_main("playback.watched", {"user": "anna", "manual": True, "item": {"type": "movie", "title": "Heat", "year": 1995, "ids": {"imdb": "tt0113277", "tmdb": 949}}})
        self.assertEqual(r, {"ok": True, "simkl": "added"})
        (method, path, headers, body), = SEEN
        headers = {k.lower(): v for k, v in headers.items()}
        self.assertEqual((method, path, headers["authorization"], headers["simkl-api-key"]), ("POST", "/simkl/sync/history", "Bearer simkltok", "skid"))
        movie = json.loads(body)["movies"][0]
        self.assertEqual((movie["ids"], movie["year"]), ({"imdb": "tt0113277", "tmdb": 949}, 1995))
        r = self.episode(1, 3)
        show = json.loads(SEEN[-1][3])["shows"][0]
        self.assertEqual((show["ids"], show["seasons"][0]["number"], show["seasons"][0]["episodes"][0]["number"]), ({"tvdb": 424536, "mal": 100}, 1, 3))
        r = self.run_main("playback.watched", {"user": "anna", "item": {"type": "movie", "title": "?", "ids": {"imdb": "tt0000000"}}})
        self.assertEqual(r["simkl"], "not found")
        # Someone not connected: nothing happens.
        SEEN.clear()
        self.assertEqual(self.episode(1, 1, user="bob"), {"ok": True})
        self.assertEqual(SEEN, [])

    def test_a_simkl_sign_out_is_noticed(self):
        main.save({"people": {"anna": {"simkl": {"token": "expired", "name": "A"}}}})
        self.assertEqual(self.episode(1, 1)["simkl"], "error")
        self.assertNotIn("simkl", main.load()["people"]["anna"])

    def test_myanimelist_seasons_and_counting(self):
        self.assertEqual(self.connect_mal(), {"message": "MyAnimeList is connected.", "panel": mock.ANY})
        self.assertEqual(main.load()["people"]["anna"]["mal"]["name"], "anna_mal")
        s = dict(self.settings, simkl_client_id="")
        run = lambda season, ep: self.run_main("playback.watched", {"user": "anna", "item": {"type": "episode", "season": season, "episode": ep,
                                                                                             "show": {"title": "Frieren", "ids": {"mal": 100}}}}, s)
        self.assertEqual(run(1, 3)["mal"], "watching")
        self.assertEqual(LIST[100], {"status": "watching", "num_episodes_watched": 3})
        run(1, 2)  # an earlier one again: the count stays
        self.assertEqual(LIST[100]["num_episodes_watched"], 3)
        self.assertEqual(run(1, 12)["mal"], "completed")
        # Season 2 is the sequel of season 1; season 3 the sequel of that.
        run(2, 1)
        run(3, 13)
        self.assertEqual(LIST[200], {"status": "watching", "num_episodes_watched": 1})
        self.assertEqual(LIST[300], {"status": "completed", "num_episodes_watched": 13})
        # Counted on from the start of the show: episode 14 of "season 1" is season 2's second.
        run(1, 14)
        self.assertEqual(LIST[200]["num_episodes_watched"], 2)
        # A rewatch of a finished season leaves it finished.
        self.assertEqual(run(1, 1)["mal"], "already completed")
        # Sequels are remembered: no second look-up of season 1's relations.
        looks = [p for m, p, _, _ in SEEN if m == "GET" and p.startswith("/mal/anime/100?fields=related")]
        self.assertEqual(len(looks), 1)
        # A movie, and a title without a MyAnimeList id.
        r = self.run_main("playback.watched", {"user": "anna", "item": {"type": "movie", "title": "Movie", "ids": {"mal": 900}}}, s)
        self.assertEqual((r["mal"], LIST[900]["status"]), ("completed", "completed"))
        r = self.run_main("playback.watched", {"user": "anna", "item": {"type": "movie", "title": "Heat", "ids": {"imdb": "tt1"}}}, s)
        self.assertEqual(r["mal"], "skipped")

    def test_myanimelist_token_is_refreshed(self):
        self.connect_mal()
        st = main.load()
        st["people"]["anna"]["mal"]["until"] = int(time.time()) - 1
        main.save(st)
        self.run_main("playback.watched", {"user": "anna", "item": {"type": "movie", "ids": {"mal": 900}}}, dict(self.settings, simkl_client_id=""))
        self.assertEqual(main.load()["people"]["anna"]["mal"]["access"], "malaccess2")
        self.assertEqual(SEEN[-1][2]["Authorization"], "Bearer malaccess2")

    def test_myanimelist_wrong_paste(self):
        self.call("mal_link")
        self.assertIn("no code", self.call("mal_code", query="https://kino.example/web/")["error"])
        self.assertIn("older attempt", self.call("mal_code", query="https://x/?code=thecode&state=nope")["error"])
        # The code alone works too.
        self.assertEqual(self.call("mal_code", query="thecode")["message"], "MyAnimeList is connected.")
        r = self.call("disconnect", args={"site": "mal"})
        self.assertEqual(r["message"], "Disconnected.")
        self.assertNotIn("mal", main.load()["people"]["anna"])

    def test_not_set_up(self):
        p = self.run_main("api.call", {"action": "panel", "user": "anna", "body": {}}, {"simkl_client_id": "", "mal_client_id": ""})
        self.assertIn("Not set up yet", p["text"])
        p = self.run_main("api.call", {"action": "panel", "user": "anna", "body": {}}, {"simkl_client_id": "x"})
        self.assertEqual([i["title"] for i in p["items"]], ["SIMKL"])


if __name__ == "__main__":
    unittest.main()
