"""Tests for the library doctor: python3 -I -m unittest discover -s plugins/library-doctor"""
import json
import os
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

SETTINGS = {"languages": ["de"], "min_width": 1280, "skip_libraries": []}


def audio(lang):
    return {"kind": "audio", "codec": "ac3", "language": lang, "channels": 6}


def video(w, h):
    return {"kind": "video", "codec": "h264", "width": w, "height": h}


def movie(id, title, year=2000, ids=None, streams=None, path=None, section=1, subs=None, **kw):
    path = path or "/media/Movies/%s (%s)/%s (%s).mkv" % (title, year, title, year)
    it = {"id": id, "type": "movie", "title": title, "year": year, "ids": {"tmdb": 1000 + id} if ids is None else ids,
          "section_id": section,
          "files": [{"path": path, "streams": [video(1920, 1080), audio("ger")] if streams is None else streams,
                     "external_subtitles": subs or []}]}
    it.update(kw)
    return it


def episode(id, show_id, season, number, ids=None, streams=None):
    return {"id": id, "type": "episode", "title": "Folge %s" % number, "season": season, "episode": number, "ids": {},
            "show": {"id": show_id, "title": "Dark", "year": 2017, "ids": {"tvdb": 334824} if ids is None else ids},
            "section_id": 2,
            "files": [{"path": "/media/Shows/Dark/S%02dE%02d.mkv" % (season or 0, number or 0),
                       "streams": [video(1920, 1080), audio("deu")] if streams is None else streams, "external_subtitles": []}]}


def library(*items):
    return {"sections": [{"id": 1, "name": "Movies", "kind": "movie"}, {"id": 2, "name": "Shows", "kind": "show"},
                         {"id": 3, "name": "Anime", "kind": "show"}], "items": list(items)}


def keys(found, check):
    return sorted(e["id"] for e in found[check])


class Checks(unittest.TestCase):
    def test_a_good_library_has_nothing_to_report(self):
        found, total, hint = main.check(library(movie(1, "Heat", 1995), episode(2, 50, 1, 1)), SETTINGS)
        self.assertEqual(total, 2)
        self.assertFalse(hint)
        self.assertEqual({c: v for c, v in found.items() if v}, {})

    def test_the_same_movie_twice_is_found_but_a_version_is_not(self):
        found, _, _ = main.check(library(
            movie(1, "Heat", 1995, ids={"tmdb": 949}),
            movie(2, "Heat", 1995, ids={"tmdb": 949}, path="/media/Movies/Heat.mkv"),
            movie(3, "Alien", 1979, ids={"tmdb": 348}),
            movie(4, "Alien", 1979, ids={"tmdb": 348}, path="/media/Movies/Alien 4K.mkv", version_of=3),
        ), SETTINGS)
        self.assertEqual(len(found["duplicate"]), 1)
        self.assertEqual(found["duplicate"][0]["key"], "duplicate:1-2")
        self.assertIn("/media/Movies/Heat.mkv", found["duplicate"][0]["detail"])

    def test_an_episode_twice_is_found(self):
        found, _, _ = main.check(library(episode(1, 50, 1, 1), episode(2, 50, 1, 1), episode(3, 50, 1, 2)), SETTINGS)
        self.assertEqual([e["key"] for e in found["duplicate"]], ["duplicate:1-2"])

    def test_low_resolution_counts_the_best_version(self):
        found, _, _ = main.check(library(
            movie(1, "Old", streams=[video(720, 576), audio("ger")]),
            movie(2, "Fine", streams=[video(720, 576), audio("ger")]),
            movie(3, "Fine", streams=[video(3840, 1600), audio("ger")], version_of=2, path="/x/Fine 4K.mkv"),
            movie(4, "Scope", streams=[video(1920, 800), audio("ger")]),
        ), SETTINGS)
        self.assertEqual(keys(found, "low_resolution"), [1])
        self.assertEqual(found["low_resolution"][0]["detail"], "720×576")
        found, _, _ = main.check(library(movie(1, "Old", streams=[video(720, 576), audio("ger")])), dict(SETTINGS, min_width=0))
        self.assertEqual(found["low_resolution"], [])

    def test_no_audio_in_the_chosen_language_notes_the_subtitles(self):
        found, _, _ = main.check(library(
            movie(1, "Amelie", streams=[video(1920, 1080), audio("fre")], subs=["/media/Movies/Amelie (2000)/Amelie (2000).de.srt"]),
            movie(2, "Oldboy", streams=[video(1920, 1080), audio("kor"), {"kind": "subtitle", "language": "eng"}]),
            movie(3, "Mixed", streams=[video(1920, 1080), audio("eng"), audio("de")]),
            movie(4, "Untagged", streams=[video(1920, 1080), audio("und")]),
            movie(5, "Silent", streams=[video(1920, 1080)]),
        ), SETTINGS)
        self.assertEqual(keys(found, "no_language"), [1, 2])
        amelie = json.loads(found["no_language"][0]["detail"])
        self.assertEqual(amelie, {"audio": ["fr"], "subs": ["de"]})
        self.assertEqual(json.loads(found["no_language"][1]["detail"])["subs"], [])
        self.assertEqual(keys(found, "untagged_audio"), [4])

    def test_any_of_several_languages_will_do(self):
        found, _, _ = main.check(library(movie(1, "Heat", streams=[video(1920, 1080), audio("eng")])), dict(SETTINGS, languages="de, en"))
        self.assertEqual(found["no_language"], [])

    def test_files_that_could_not_be_read(self):
        found, _, hint = main.check(library(movie(1, "Heat"), movie(2, "Broken", streams=[])), SETTINGS)
        self.assertEqual(keys(found, "unreadable"), [2])
        self.assertFalse(hint)

    def test_no_streams_anywhere_gives_one_hint_instead_of_a_list(self):
        found, _, hint = main.check(library(movie(1, "A", streams=[]), movie(2, "B", streams=[])), SETTINGS)
        self.assertTrue(hint)
        self.assertEqual(found["unreadable"], [])

    def test_names_the_scanner_could_not_read(self):
        found, _, _ = main.check(library(
            movie(1, "Heat", year=None),
            movie(2, "Some.Movie.x264", ids={}),
            episode(3, 50, None, 4),
            episode(4, 60, 1, 1, ids={}),
            episode(5, 60, 1, 2, ids={}),
        ), SETTINGS)
        self.assertEqual(keys(found, "no_year"), [1])
        self.assertEqual(keys(found, "no_numbers"), [3])
        # The show without details is one entry, with its episodes counted.
        self.assertEqual(sorted(e["key"] for e in found["no_details"]), ["no_details:2", "no_details:show:60"])
        show = next(e for e in found["no_details"] if e["key"].endswith("show:60"))
        self.assertEqual(json.loads(show["detail"]), {"episodes": 2})

    def test_without_any_details_online_lookups_are_off_and_not_listed(self):
        found, _, _ = main.check(library(movie(1, "A", ids={}), movie(2, "B", ids={})), SETTINGS)
        self.assertEqual(found["no_details"], [])

    def test_a_library_left_out_by_name(self):
        lib = library(movie(1, "Akira", streams=[video(1920, 1080), audio("jpn")], section=3))
        self.assertEqual(len(main.check(lib, SETTINGS)[0]["no_language"]), 1)
        found, total, _ = main.check(lib, dict(SETTINGS, skip_libraries=["anime"]))
        self.assertEqual((found["no_language"], total), ([], 0))

    def test_subtitle_names_give_their_language(self):
        v = "/m/Heat (1995).mkv"
        self.assertEqual(main.subtitle_languages("/m/Heat (1995).de.srt", v), {"de"})
        self.assertEqual(main.subtitle_languages("/m/Heat (1995).German.forced.srt", v), {"de"})
        self.assertEqual(main.subtitle_languages("/m/Heat (1995).srt", v), set())
        self.assertEqual(main.lang_code("ger"), "de")
        self.assertEqual(main.lang_code("pt-BR"), "pt")
        self.assertEqual(main.lang_code("und"), "")


class Listing(BaseHTTPRequestHandler):
    body = {}
    asked = []

    def do_GET(self):
        Listing.asked.append(self.path)
        data = json.dumps(Listing.body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *a):
        pass


class Panel(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.old = (main.REPORT, main.IGNORED, main.LOCK)
        main.REPORT = os.path.join(self.dir, "report.json")
        main.IGNORED = os.path.join(self.dir, "ignored.json")
        main.LOCK = os.path.join(self.dir, ".lock")
        self.http = ThreadingHTTPServer(("127.0.0.1", 0), Listing)
        threading.Thread(target=self.http.serve_forever, daemon=True).start()
        self.server = {"library": "http://127.0.0.1:%d/api/plugins/library-doctor/library?key=k" % self.http.server_port}
        Listing.body = library(movie(1, "Old", streams=[video(720, 576), audio("ger")]), movie(2, "Heat", 1995),
                               movie(3, "Amelie", streams=[video(1920, 1080), audio("fre")]))
        Listing.asked = []

    def tearDown(self):
        self.http.shutdown()
        self.http.server_close()
        main.REPORT, main.IGNORED, main.LOCK = self.old

    def call(self, action, args=None, admin=True, lang="de"):
        body = {"lang": lang}
        if args:
            body["args"] = args
        return main.answer_call({"action": action, "body": body, "user": "a", "admin": admin}, SETTINGS, self.server)

    def test_only_admins_see_it(self):
        self.assertIn("error", self.call("panel", admin=False))

    def test_check_show_and_ignore(self):
        first = self.call("panel")
        self.assertIn("noch nicht geprüft", first["text"])
        p = self.call("check")["panel"]
        self.assertEqual(Listing.asked, ["/api/plugins/library-doctor/library?key=k"])
        self.assertIn("3 Titel", p["text"])
        self.assertEqual([(i["title"], i["status"]) for i in p["items"]],
                         [("Niedrige Auflösung", "1 gefunden"), ("Kein Ton auf Deutsch", "1 gefunden")])
        shown = self.call("show", {"check": "no_language"})["panel"]
        self.assertEqual(shown["items"][0]["title"], "Amelie (2000)")
        self.assertEqual(shown["items"][0]["href"], "#/item/3")
        self.assertIn("Ton: Französisch", shown["items"][0]["subtitle"])
        ignored = self.call("ignore", shown["items"][0]["buttons"][0]["args"])
        self.assertEqual(ignored["panel"]["text"].split(" ")[0:3], ["Hier", "ist", "nichts"])
        p = self.call("panel")
        self.assertEqual(len(p["items"]), 1)
        self.assertIn("Ignorierte wieder zeigen (1)", [b["label"] for b in p["buttons"]])
        self.assertEqual(len(self.call("unignore")["panel"]["items"]), 2)

    def test_a_scan_that_changed_nothing_does_not_check_again(self):
        main.run_check(self.server, SETTINGS)
        Listing.asked = []
        out = self.run_main({"event": "library.scan.finished", "data": {"added": 0, "updated": 0, "removed": 0}})
        self.assertEqual((out, Listing.asked), ({"skipped": "nothing changed"}, []))
        out = self.run_main({"event": "library.scan.finished", "data": {"added": 1, "updated": 0, "removed": 0}})
        self.assertEqual(out["found"], {"low_resolution": 1, "no_language": 1})

    def test_a_server_without_a_library_address(self):
        self.server = {}
        self.assertIn("Server", self.call("check")["error"])

    def run_main(self, msg):
        import io
        msg = dict(msg, settings=SETTINGS, server=self.server)
        old_in, old_out = sys.stdin, sys.stdout
        sys.stdin = io.TextIOWrapper(io.BytesIO(json.dumps(msg).encode()), encoding="utf-8")
        sys.stdout = io.StringIO()
        try:
            main.main()
            return json.loads(sys.stdout.getvalue().strip().splitlines()[-1])
        finally:
            sys.stdin, sys.stdout = old_in, old_out


if __name__ == "__main__":
    unittest.main()
