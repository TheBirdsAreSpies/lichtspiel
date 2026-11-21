"""Tests for the subtitles plugin: python3 -I -m unittest discover -s plugins/subtitles"""
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

SEEN = []  # (method, path, query, body, headers) of every request the fake OpenSubtitles got
STATE = {"search": [], "status": 200, "logins": 0, "base": ""}


class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def reply(self, code, body, ctype="application/json"):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def answer(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        u = urllib.parse.urlsplit(self.path)
        q = dict(urllib.parse.parse_qsl(u.query))
        SEEN.append((self.command, u.path, q, json.loads(body or b"null"), dict(self.headers)))
        if STATE["status"] != 200:
            return self.reply(STATE["status"], {"message": "nope"})
        if u.path.endswith("/login"):
            STATE["logins"] += 1
            return self.reply(200, {"token": "tok"})
        if u.path.endswith("/subtitles"):
            return self.reply(200, {"data": STATE["search"]})
        if u.path.endswith("/download"):
            return self.reply(200, {"link": STATE["base"] + "/file/%s" % json.loads(body)["file_id"]})
        if u.path.startswith("/file/"):
            return self.reply(200, ("1\n00:00:01,000 --> 00:00:02,000\nfile %s\n" % u.path[6:]).encode(), "text/plain")
        if u.path.endswith("/infos/user"):
            return self.reply(200, {"data": {"remaining_downloads": 17}})
        self.reply(404, {})

    do_GET = do_POST = answer


def sub(file_id, **a):
    a.setdefault("download_count", 0)
    return {"attributes": dict(a, files=[{"file_id": file_id}])}


def movie(path):
    return {"id": 1, "type": "movie", "title": "The Matrix", "year": 1999, "ids": {"imdb": "tt0133093"},
            "files": [{"path": path, "streams": [], "external_subtitles": []}]}


class PluginTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.srv = ThreadingHTTPServer(("127.0.0.1", 0), Fake)
        cls.srv.daemon_threads = True
        threading.Thread(target=cls.srv.serve_forever, daemon=True).start()
        STATE["base"] = "http://127.0.0.1:%d" % cls.srv.server_address[1]

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()
        cls.srv.server_close()

    def setUp(self):
        SEEN.clear()
        STATE.update(search=[], status=200, logins=0)
        self.dir = tempfile.mkdtemp()
        self.video = os.path.join(self.dir, "The Matrix (1999).mkv")
        with open(self.video, "wb") as f:
            f.write(b"\1" * 200000)
        self.patches = [mock.patch.object(main, "API", STATE["base"] + "/api/v1"),
                        mock.patch.object(main, "TOKEN_FILE", os.path.join(self.dir, "token.json")),
                        mock.patch.object(main, "LIMIT_FILE", os.path.join(self.dir, "limit"))]
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in self.patches:
            p.stop()
        shutil.rmtree(self.dir, ignore_errors=True)

    def run_plugin(self, event, data, **settings):
        s = {"api_key": "k", "languages": ["de"], "next_to_video": True}
        s.update(settings)
        out, err = io.StringIO(), io.StringIO()
        code = 0
        server = {"data_dir": os.path.join(self.dir, "data")}
        stdin = io.StringIO(json.dumps({"event": event, "data": data, "settings": s, "server": server}))
        with mock.patch("sys.stdin", stdin), redirect_stdout(out), redirect_stderr(err):
            try:
                main.main()
            except SystemExit as e:
                code = e.code or 0
        lines = out.getvalue().strip().splitlines()
        return code, json.loads(lines[-1]) if lines else None, err.getvalue()

    def searches(self):
        return [s[2] for s in SEEN if s[1].endswith("/subtitles")]

    def test_saves_the_best_subtitle_next_to_the_video(self):
        STATE["search"] = [sub(1, download_count=900, machine_translated=True), sub(2, download_count=50), sub(3, download_count=10, moviehash_match=True)]
        code, out, err = self.run_plugin("library.item.added", {"item": movie(self.video)})
        self.assertEqual(code, 0, err)
        target = os.path.join(self.dir, "The Matrix (1999).de.srt")
        self.assertEqual(out["saved"], [target])
        self.assertTrue(out["refresh_subtitles"])
        with open(target) as f:
            self.assertIn("file 3", f.read())  # the hash match beats more downloads
        q = self.searches()[0]
        self.assertEqual((q["imdb_id"], q["languages"], len(q["moviehash"])), ("133093", "de", 16))
        self.assertEqual(SEEN[0][4]["Api-Key"], "k")
        # A second run finds the file and asks nobody.
        SEEN.clear()
        self.run_plugin("library.item.added", {"item": movie(self.video)})
        self.assertEqual(SEEN, [])

    def test_by_default_it_saves_in_the_data_folder_below_the_videos_folder(self):
        STATE["search"] = [sub(1)]
        code, out, err = self.run_plugin("library.item.added", {"item": movie(self.video)}, next_to_video=False)
        self.assertEqual(code, 0, err)
        target = main.store_dir(os.path.join(self.dir, "data", "subtitles"), self.dir)
        target = os.path.join(target, "The Matrix (1999).de.srt")
        self.assertEqual(out["saved"], [target])
        self.assertTrue(os.path.isfile(target))
        self.assertFalse(os.path.exists(os.path.join(self.dir, "The Matrix (1999).de.srt")))
        # Found there the next time: nobody is asked.
        SEEN.clear()
        self.run_plugin("library.item.added", {"item": movie(self.video)}, next_to_video=False)
        self.assertEqual(SEEN, [])

    def test_the_store_mirrors_the_folder_like_the_server(self):
        self.assertEqual(main.store_dir("/data/subtitles", "/media/Movies/X").replace("\\", "/"), "/data/subtitles/media/Movies/X")
        if os.name == "nt":
            self.assertEqual(main.store_dir("S", r"C:\Movies\X"), os.path.join("S", "C", "Movies", "X"))

    def test_nothing_saved_asks_for_no_refresh(self):
        _, out, _ = self.run_plugin("library.item.added", {"item": movie(self.video)})
        self.assertFalse(out["refresh_subtitles"])

    def test_a_language_the_video_already_has_is_not_searched(self):
        item = movie(self.video)
        item["files"][0]["streams"] = [{"kind": "subtitle", "language": "ger"}]
        self.run_plugin("library.item.added", {"item": item}, languages=["de", "en"])
        self.assertEqual([q["languages"] for q in self.searches()], ["en"])
        SEEN.clear()
        item["files"][0]["streams"] = []
        item["files"][0]["external_subtitles"] = [os.path.join(self.dir, "The Matrix (1999).ger.srt")]
        self.run_plugin("library.item.added", {"item": item})
        self.assertEqual(SEEN, [])

    def test_forced_subtitles_do_not_count_as_having_the_language(self):
        self.assertFalse(main.has_language([{"streams": [{"kind": "subtitle", "language": "ger", "forced": True}]}], "de"))
        self.assertFalse(main.has_language([{"external_subtitles": ["/m/A.de.forced.srt"]}], "de"))
        self.assertTrue(main.has_language([{"external_subtitles": ["/m/A.de.sdh.srt"]}], "de"))

    def test_an_episode_is_searched_by_the_show_and_its_numbers(self):
        item = {"type": "episode", "title": "Pilot", "season": 1, "episode": 2, "ids": {},
                "show": {"title": "Dark", "ids": {"imdb": "tt5753856"}}, "files": [{"path": self.video}]}
        self.run_plugin("library.item.added", {"item": item})
        q = self.searches()[0]
        self.assertEqual((q["parent_imdb_id"], q["season_number"], q["episode_number"]), ("5753856", "1", "2"))

    def test_nothing_found_saves_nothing_and_is_no_error(self):
        code, out, err = self.run_plugin("library.item.added", {"item": movie(self.video)})
        self.assertEqual((code, out["saved"]), (0, []))
        self.assertIn("no de subtitles", err)

    def test_machine_translations_only_when_allowed(self):
        STATE["search"] = [sub(1, ai_translated=True)]
        self.assertEqual(self.run_plugin("library.item.added", {"item": movie(self.video)})[1]["saved"], [])
        _, out, _ = self.run_plugin("library.item.added", {"item": movie(self.video)}, allow_machine_translated=True)
        self.assertEqual(len(out["saved"]), 1)

    def test_the_daily_limit_stops_further_tries_until_tomorrow(self):
        STATE["status"] = 406
        code, _, err = self.run_plugin("library.item.added", {"item": movie(self.video)})
        self.assertEqual(code, 1)
        self.assertIn("limit", err)
        SEEN.clear()
        STATE["status"] = 200
        code, _, err = self.run_plugin("library.item.added", {"item": movie(self.video)})
        self.assertEqual((code, SEEN), (1, []))

    def test_without_a_key_it_says_so(self):
        code, _, err = self.run_plugin("library.item.added", {"item": movie(self.video)}, api_key="")
        self.assertEqual(code, 1)
        self.assertIn("api_key", err)

    def test_a_read_only_folder_is_reported(self):
        STATE["search"] = [sub(1)]
        os.chmod(self.dir, 0o500)
        try:
            real = open

            def no_write(path, mode="r", *a, **k):
                if "x" in mode:
                    raise PermissionError(13, "Permission denied")
                return real(path, mode, *a, **k)
            with mock.patch("builtins.open", no_write):
                code, _, err = self.run_plugin("library.item.added", {"item": movie(self.video)})
        finally:
            os.chmod(self.dir, 0o700)
        self.assertEqual(code, 1)
        self.assertIn("writable", err)

    def test_the_sign_in_is_reused_the_same_day(self):
        STATE["search"] = [sub(1)]
        for lang in ("de", "en"):
            self.run_plugin("library.item.added", {"item": movie(self.video)}, username="u", password="p", languages=[lang])
        self.assertEqual(STATE["logins"], 1)
        api = [s for s in SEEN if s[1].startswith("/api/") and not s[1].endswith("/login")]
        self.assertTrue(api and all(s[4].get("Authorization") == "Bearer tok" for s in api))

    def test_shows_and_seasons_are_left_alone(self):
        code, out, _ = self.run_plugin("library.item.added", {"item": {"type": "show", "title": "Dark", "files": []}})
        self.assertEqual((code, out["saved"], SEEN), (0, [], []))

    def test_only_an_admin_may_test_the_key(self):
        _, out, _ = self.run_plugin("api.call", {"action": "test", "admin": False})
        self.assertIn("error", out)
        _, out, _ = self.run_plugin("api.call", {"action": "test", "admin": True}, username="u", password="p")
        self.assertIn("17 downloads left", out["message"])
        STATE["status"] = 401
        os.remove(main.TOKEN_FILE)
        _, out, _ = self.run_plugin("api.call", {"action": "test", "admin": True}, username="u", password="p")
        self.assertIn("refused", out["error"])


if __name__ == "__main__":
    unittest.main()
