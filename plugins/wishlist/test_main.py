"""Tests for the wish list plugin: python3 -I -m unittest discover -s plugins/wishlist"""
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

SEEN = []  # (app, method, path, query, body, api key)

MATRIX = {"title": "The Matrix", "year": 1999, "tmdbId": 603, "imdbId": "tt0133093", "overview": "A hacker learns the truth.",
          "images": [{"coverType": "poster", "remoteUrl": "https://image.tmdb.org/t/p/original/matrix.jpg"}]}
HEAT = {"title": "Heat", "year": 1995, "tmdbId": 949, "id": 5, "hasFile": True}
DARK = {"title": "Dark", "year": 2017, "tvdbId": 334824, "images": [{"coverType": "poster", "remoteUrl": "http://insecure/x.jpg"}]}


class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def reply(self, code, body):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def answer(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        u = urllib.parse.urlsplit(self.path)
        q = dict(urllib.parse.parse_qsl(u.query))
        app, path = u.path.split("/api/v3")
        app = app.strip("/")
        body = json.loads(raw or b"null")
        SEEN.append((app, self.command, path, q, body, self.headers.get("X-Api-Key")))
        st = STATE[app]
        if st.get("down"):
            return self.reply(500, {"message": "broken"})
        if path == "/movie/lookup":
            return self.reply(200, [MATRIX, HEAT] if "matrix" in q["term"].lower() or "heat" in q["term"].lower() else [])
        if path == "/movie/lookup/tmdb":
            return self.reply(200, {"603": MATRIX, "949": HEAT}.get(q["tmdbId"], {}))
        if path == "/series/lookup":
            return self.reply(200, [DARK] if "dark" in q["term"].lower() or q["term"] == "tvdb:334824" else [])
        if path == "/qualityprofile":
            return self.reply(200, [{"id": 1, "name": "Any"}, {"id": 4, "name": "HD-1080p"}])
        if path == "/rootfolder":
            return self.reply(200, [{"id": 1, "path": "/movies"}, {"id": 2, "path": "/kids"}])
        if path == "/languageprofile":
            return self.reply(404, {"message": "gone in v4"})
        if path in ("/movie", "/series") and self.command == "POST":
            return self.reply(201, dict(body, id=77))
        if path == "/calendar" and app == "radarr":
            return self.reply(200, [
                dict(MATRIX, inCinemas="2026-11-10T00:00:00Z", digitalRelease="2026-12-01T00:00:00Z", physicalRelease="2027-03-01T00:00:00Z"),
                dict(HEAT, digitalRelease="2026-12-02T00:00:00Z")])
        if path == "/calendar" and app == "sonarr":
            return self.reply(200, [
                {"seasonNumber": 4, "episodeNumber": 1, "title": "Anfang", "airDate": "2026-11-26", "hasFile": False, "series": DARK},
                {"seasonNumber": 3, "episodeNumber": 9, "title": "Alt", "airDate": "2026-11-21", "hasFile": True, "series": DARK}])
        if path == "/queue":
            return self.reply(200, {"records": st.get("queue", [])})
        if path.startswith("/movie/"):
            return self.reply(200, {"id": 77, "hasFile": st.get("has_file", False)})
        if path.startswith("/series/"):
            return self.reply(200, {"id": 77, "statistics": {"episodeFileCount": st.get("files", 0), "episodeCount": 18}})
        self.reply(404, {})

    do_GET = do_POST = answer


STATE = {"radarr": {}, "sonarr": {}}


class WishTest(unittest.TestCase):
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
        STATE.update(radarr={}, sonarr={})
        self.dir = tempfile.mkdtemp()
        self.patches = [mock.patch.object(main, "STATE", os.path.join(self.dir, "wishes.json")),
                        mock.patch.object(main, "LOCK", os.path.join(self.dir, ".lock"))]
        for p in self.patches:
            p.start()
        self.settings = {"radarr_url": self.base + "/radarr", "radarr_api_key": "rk", "radarr_quality_profile": "HD-1080p",
                         "sonarr_url": self.base + "/sonarr/", "sonarr_api_key": "sk", "max_open_wishes": 10}

    def tearDown(self):
        for p in self.patches:
            p.stop()
        shutil.rmtree(self.dir, ignore_errors=True)

    def run_plugin(self, event, data, settings=None):
        out, err = io.StringIO(), io.StringIO()
        stdin = io.StringIO(json.dumps({"event": event, "data": data, "settings": self.settings if settings is None else settings}))
        with mock.patch("sys.stdin", stdin), redirect_stdout(out), redirect_stderr(err):
            main.main()
        return json.loads(out.getvalue().strip().splitlines()[-1])

    def call(self, action, user="anna", admin=False, settings=None, **body):
        body.setdefault("lang", "de")
        return self.run_plugin("api.call", {"action": action, "body": body, "user": user, "admin": admin}, settings)

    def wishes(self):
        with open(main.STATE, encoding="utf-8") as f:
            return json.load(f)["wishes"]

    def test_without_radarr_or_sonarr_the_panel_says_how_to_set_it_up(self):
        p = self.call("panel", settings={})
        self.assertIn("noch nicht eingerichtet", p["text"])
        self.assertNotIn("input", p)

    def test_the_panel_has_a_search_and_finds_movies_and_shows(self):
        p = self.call("panel")
        self.assertEqual(p["input"]["action"], "search")
        r = self.call("search", query="Matrix")["panel"]
        titles = [i["title"] for i in r["items"]]
        self.assertEqual(titles[0], "The Matrix")
        self.assertIn("Heat", titles)
        matrix = r["items"][0]
        self.assertEqual(matrix["buttons"][0]["args"], {"kind": "movie", "id": 603})
        self.assertEqual(matrix["image"], "https://image.tmdb.org/t/p/original/matrix.jpg")
        heat = [i for i in r["items"] if i["title"] == "Heat"][0]
        self.assertEqual((heat["status"], heat["buttons"]), ("schon da", []))  # Radarr has its file
        r = self.call("search", query="Dark")["panel"]
        self.assertEqual(r["items"][0]["image"], "")  # no http pictures
        self.assertTrue(all(s[5] in ("rk", "sk") for s in SEEN))

    def test_a_wish_goes_to_radarr_with_the_chosen_profile(self):
        r = self.call("wish", args={"kind": "movie", "id": 603})
        self.assertIn("Radarr sucht", r["message"])
        post = [s for s in SEEN if s[1] == "POST"][0]
        self.assertEqual((post[0], post[2]), ("radarr", "/movie"))
        self.assertEqual((post[4]["qualityProfileId"], post[4]["rootFolderPath"], post[4]["tmdbId"]), (4, "/movies", 603))
        self.assertTrue(post[4]["addOptions"]["searchForMovie"])
        w = self.wishes()[0]
        self.assertEqual((w["title"], w["by"], w["arr_id"], w["status"]), ("The Matrix", ["anna"], 77, "wished"))
        self.assertEqual(r["panel"]["items"][0]["title"], "The Matrix")
        # Someone else wishes it too: no second copy in Radarr.
        SEEN.clear()
        self.call("wish", user="ben", args={"kind": "movie", "id": 603})
        self.assertFalse([s for s in SEEN if s[1] == "POST"])
        self.assertEqual(self.wishes()[0]["by"], ["anna", "ben"])
        self.assertIn("schon gewünscht", self.call("wish", user="ben", args={"kind": "movie", "id": 603})["message"])

    def test_a_show_goes_to_sonarr(self):
        self.call("wish", args={"kind": "show", "id": 334824})
        post = [s for s in SEEN if s[1] == "POST"][0]
        self.assertEqual((post[0], post[2], post[4]["qualityProfileId"]), ("sonarr", "/series", 1))
        self.assertTrue(post[4]["addOptions"]["searchForMissingEpisodes"])
        self.assertNotIn("languageProfileId", post[4])

    def test_everyone_sees_their_own_wishes_and_an_admin_sees_all(self):
        self.call("wish", user="anna", args={"kind": "movie", "id": 603})
        self.call("wish", user="ben", args={"kind": "show", "id": 334824})
        anna = self.call("panel", user="anna")
        self.assertEqual([i["title"] for i in anna["items"]], ["The Matrix"])
        self.assertEqual([b["action"] for b in anna["items"][0]["buttons"]], ["withdraw"])
        admin = self.call("panel", user="boss", admin=True)
        self.assertEqual(sorted(i["title"] for i in admin["items"]), ["Dark", "The Matrix"])
        self.assertIn("gewünscht von anna", [i for i in admin["items"] if i["title"] == "The Matrix"][0]["subtitle"])

    def test_withdrawing_and_removing(self):
        self.call("wish", user="anna", args={"kind": "movie", "id": 603})
        self.call("wish", user="ben", args={"kind": "movie", "id": 603})
        self.call("withdraw", user="anna", args={"key": "movie:603"})
        self.assertEqual(self.wishes()[0]["by"], ["ben"])
        self.assertIn("error", self.call("remove", user="ben", args={"key": "movie:603"}))  # not an admin
        self.call("remove", user="boss", admin=True, args={"key": "movie:603"})
        self.assertEqual(self.wishes(), [])

    def test_open_wishes_are_limited(self):
        self.settings["max_open_wishes"] = 1
        self.call("wish", args={"kind": "movie", "id": 603})
        r = self.call("wish", args={"kind": "show", "id": 334824})
        self.assertIn("1 offene Wünsche", r["error"])
        self.assertIn("message", self.call("wish", user="boss", admin=True, args={"kind": "show", "id": 334824}))

    def test_only_admins_when_set(self):
        self.settings["only_admins"] = True
        p = self.call("panel")
        self.assertNotIn("input", p)
        self.assertIn("error", self.call("wish", args={"kind": "movie", "id": 603}))
        self.assertIn("input", self.call("panel", user="boss", admin=True))

    def test_the_panel_shows_downloads_and_what_radarr_has(self):
        self.call("wish", args={"kind": "movie", "id": 603})
        STATE["radarr"]["queue"] = [{"movieId": 77, "size": 1000, "sizeleft": 250}]
        item = self.call("panel")["items"][0]
        self.assertEqual(item["status"], "wird geladen, 75 %")
        STATE["radarr"].update(queue=[], has_file=True)
        self.assertEqual(self.call("panel")["items"][0]["status"], "heruntergeladen")

    def test_a_new_title_in_the_library_fulfils_the_wish(self):
        self.call("wish", args={"kind": "movie", "id": 603})
        self.call("wish", args={"kind": "show", "id": 334824})
        r = self.run_plugin("library.item.added", {"id": 9, "item": {"type": "movie", "title": "Matrix, The", "year": 1999, "ids": {}}})
        self.assertEqual(r["here"], [])  # the name is compared as it is ("Matrix, The" is fixed by the scanner)
        r = self.run_plugin("library.item.added", {"id": 9, "item": {"type": "movie", "title": "The Matrix", "year": 1999, "ids": {}}})
        self.assertEqual(r["here"], ["The Matrix"])
        r = self.run_plugin("library.item.added", {"id": 10, "item": {"type": "episode", "title": "Secrets", "ids": {},
                                                                     "show": {"title": "Dark", "year": None, "ids": {"tvdb": 334824}}}})
        self.assertEqual(r["here"], ["Dark"])
        items = self.call("panel")["items"]
        self.assertEqual({i["status"] for i in items}, {"da"})
        self.assertEqual({i["status_kind"] for i in items}, {"default"})

    def test_wishes_that_arrived_long_ago_leave_the_list(self):
        self.call("wish", args={"kind": "movie", "id": 603})
        self.run_plugin("library.item.added", {"item": {"type": "movie", "title": "The Matrix", "year": 1999}})
        w = self.wishes()
        w[0]["here_at"] = time.time() - 31 * 86400
        main.save(w)
        self.assertEqual(self.run_plugin("schedule.daily", {"date": "x"})["pruned"], 1)
        self.assertEqual(self.wishes(), [])

    def test_the_calendar_lists_what_radarr_and_sonarr_still_expect(self):
        out = self.run_plugin("calendar", {"start": "2026-11-21", "end": "2027-02-19"})
        self.assertEqual(out["entries"], [
            {"type": "movie", "title": "The Matrix", "year": 1999, "date": "2026-12-01", "poster": "https://image.tmdb.org/t/p/original/matrix.jpg", "note": "digital release"},
            {"type": "episode", "title": "Dark", "year": 2017, "season": 4, "episode": 1, "name": "Anfang", "date": "2026-11-26", "poster": ""}])
        self.assertIn(("sonarr", "GET", "/calendar", {"start": "2026-11-21", "end": "2027-02-19", "includeSeries": "true"}, None, "sk"), SEEN)
        self.assertEqual(self.run_plugin("calendar", {"start": "2026-11-21", "end": "2027-02-19"}, settings={})["entries"], [])

    def test_radarr_down_gives_a_message(self):
        STATE["radarr"]["down"] = True
        self.assertIn("error", self.call("search", query="Matrix"))
        self.assertIn("Radarr", self.call("wish", args={"kind": "movie", "id": 603})["error"])
        self.assertFalse(os.path.exists(main.STATE) and self.wishes())

    def test_the_english_panel(self):
        self.assertEqual(self.call("panel", lang="en")["title"], "Wish list")


if __name__ == "__main__":
    unittest.main()
