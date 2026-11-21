"""Tests for the Letterboxd import: python3 -I -m unittest discover -s plugins/letterboxd-import"""
import base64
import io
import os
import shutil
import sys
import tempfile
import unittest
import zipfile
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

WATCHED = """Date,Name,Year,Letterboxd URI
2020-01-05,The Godfather,1972,https://boxd.it/2aNK
2021-02-01,Parasite,2019,https://boxd.it/hTha
2022-03-01,Unknown Film,2001,https://boxd.it/zzzz
"""
DIARY = """Date,Name,Year,Letterboxd URI,Rating,Rewatch,Tags,Watched Date
2020-01-05,The Godfather,1972,https://boxd.it/entry1,5,,,2019-12-24
2023-06-01,The Godfather,1972,https://boxd.it/entry2,5,Yes,,2023-05-31
"""
RATINGS = """Date,Name,Year,Letterboxd URI,Rating
2020-01-05,The Godfather,1972,https://boxd.it/2aNK,5
2021-02-01,Parasite,2019,https://boxd.it/hTha,4.5
2021-02-01,Unknown Film,2001,https://boxd.it/zzzz,
"""
WATCHLIST = """Date,Name,Year,Letterboxd URI
2024-01-01,Up,2009,https://boxd.it/up01
2024-01-02,Parasite,2019,https://boxd.it/hTha
2024-01-03,Evil,2000,https://evil.example/x
"""
LIST = """Letterboxd list export v7
Date,Name,Tags,URL,Description
2023-01-01,Beste Filme,,https://letterboxd.com/anna/list/beste-filme/,"Meine, liebsten"

Position,Name,Year,URL,Description
1,The Godfather,1972,https://letterboxd.com/film/the-godfather/,
2,Up,2009,https://letterboxd.com/film/up/,
"""


def export(**files):
    b = io.BytesIO()
    with zipfile.ZipFile(b, "w") as z:
        for name, text in files.items():
            z.writestr(name, text)
    return b.getvalue()


FULL = export(**{"watched.csv": WATCHED, "diary.csv": DIARY, "ratings.csv": RATINGS, "watchlist.csv": WATCHLIST,
                 "lists/beste-filme.csv": LIST, "deleted/lists/old.csv": LIST, "profile.csv": "Username\nanna\n"})

LIBRARY = {"sections": [{"id": 1, "name": "Filme", "kind": "movie"}, {"id": 2, "name": "Serien", "kind": "show"}],
           "items": [{"id": 10, "type": "movie", "title": "Der Pate", "year": 1972, "ids": {"tmdb": 238}, "section_id": 1},
                     {"id": 11, "type": "movie", "title": "Der Pate", "year": 1972, "ids": {"tmdb": 238}, "section_id": 1, "version_of": 10},
                     {"id": 12, "type": "movie", "title": "Parasite", "year": 2019, "ids": {}, "section_id": 1},
                     {"id": 13, "type": "movie", "title": "Oben", "year": 2009, "ids": {"imdb": "tt1049413"}, "section_id": 1},
                     {"id": 14, "type": "episode", "title": "Up", "year": 2009, "ids": {}, "section_id": 2}]}

PAGES = {"https://boxd.it/2aNK": '<div data-tmdb-id="238"></div>', "https://letterboxd.com/film/the-godfather/": '<div data-tmdb-id="238"></div>',
         "https://boxd.it/up01": '<a href="https://www.imdb.com/title/tt1049413/maindetails">', "https://letterboxd.com/film/up/": '<a href="http://imdb.com/title/tt1049413/">',
         "https://boxd.it/zzzz": "<p>no ids</p>"}


class Reading(unittest.TestCase):
    def test_the_export_gives_watched_with_diary_dates_ratings_watchlist_and_lists(self):
        d = main.parse_export(FULL)
        g = "The Godfather\t1972"
        self.assertEqual(d["watched"][g], {"at": main.day("2019-12-24"), "plays": 2})
        self.assertEqual(d["watched"]["Parasite\t2019"], {"at": main.day("2021-02-01"), "plays": 1})
        self.assertEqual(d["ratings"], {g: 5.0, "Parasite\t2019": 4.5})
        self.assertEqual(d["watchlist"], ["Up\t2009", "Parasite\t2019", "Evil\t2000"])
        self.assertEqual(d["lists"], [{"name": "Beste Filme", "keys": [g, "Up\t2009"]}])
        # The film's own short link, not a diary entry's; addresses elsewhere are not kept.
        self.assertEqual(d["films"][g]["uri"], "https://boxd.it/2aNK")
        self.assertNotIn("uri", d["films"]["Evil\t2000"])
        self.assertEqual(main.day("2019-12-24") % 86400, 12 * 3600)

    def test_a_single_list_file_and_things_that_are_no_export(self):
        d = main.parse_export(LIST.encode(), "beste-filme.csv")
        self.assertEqual([l["name"] for l in d["lists"]], ["Beste Filme"])
        for blob in (b"hello", export(**{"readme.txt": "x"}), b"PK\x03\x04broken", "Name\n\xff".encode("latin-1")):
            with self.assertRaises(ValueError):
                main.parse_export(blob, "x.csv" if not blob.startswith(b"PK") else "x.zip")


class Panel(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.patches = [mock.patch.object(main, "USERS", os.path.join(self.dir, "users")), mock.patch.object(main, "CACHE", os.path.join(self.dir, "cache.json")),
                        mock.patch.object(main, "fetch_library", lambda server: LIBRARY), mock.patch.object(main, "PAUSE", 0)]
        self.looked = []

        def fake_get(url, *a):
            self.looked.append(url)
            return PAGES.get(url)
        self.patches.append(mock.patch.object(main, "get", fake_get))
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in self.patches:
            p.stop()
        shutil.rmtree(self.dir, ignore_errors=True)

    def call(self, action, user="anna", **body):
        body.setdefault("lang", "de")
        return main.answer_call({"action": action, "user": user, "body": body}, {"library": "x"})

    def test_upload_look_up_and_bring_in(self):
        self.assertIn("upload", self.call("panel"))
        self.assertIn("error", self.call("upload", file=base64.b64encode(b"nope").decode(), name="x.csv"))
        r = self.call("upload", file=base64.b64encode(FULL).decode(), name="letterboxd.zip")
        # Parasite is found by its title; the rest is looked up while the page asks again.
        self.assertEqual(r["poll"]["action"], "lookup")
        self.assertIn("1 von 4", r["wait"])
        r = self.call("lookup")
        self.assertNotIn("poll", r)
        self.assertEqual(r["message"], "Alle Filme nachgeschlagen.")
        self.assertEqual(sorted(self.looked), ["https://boxd.it/2aNK", "https://boxd.it/up01", "https://boxd.it/zzzz"])
        # Up: its short link from the watchlist comes first, so the list's own address needs no lookup.
        items = {i["title"]: i for i in r["panel"]["items"]}
        self.assertEqual(items["Gesehen"]["status"], "2 von 3 in den Mediatheken")
        self.assertEqual(items["Watchlist"]["status"], "2 von 3 in den Mediatheken")
        self.assertEqual(items["Beste Filme"]["status"], "2 von 2 in den Mediatheken")

        r = self.call("bring", args={"what": "watched+ratings"})
        self.assertEqual(r["mark_watched"], [{"id": 10, "watched_at": main.day("2019-12-24"), "plays": 2}, {"id": 12, "watched_at": main.day("2021-02-01"), "plays": 1}])
        self.assertEqual(r["set_ratings"], [{"id": 10, "rating": 10.0}, {"id": 12, "rating": 9.0}])
        self.assertTrue(r["panel"]["items"][0]["status"].startswith("2 von 3 in den Mediatheken · übernommen am "))
        r = self.call("bring", args={"what": "watchlist", "as": "watchlist"})
        self.assertEqual(r["add_to_watchlist"], [13, 12])
        r = self.call("bring", args={"what": "list:0", "as": "playlist"})
        self.assertEqual(r["save_playlist"], {"title": "Letterboxd: Beste Filme", "summary": "Von Letterboxd", "ids": [10, 13]})
        self.assertIn("error", self.call("bring", args={"what": "list:7"}))

        # Someone else has their own upload; forgetting leaves the cache.
        self.assertNotIn("items", self.call("panel", user="ben"))
        self.assertEqual(self.call("forget")["message"], "Vergessen.")
        self.assertNotIn("items", self.call("panel"))
        self.looked.clear()
        r = self.call("upload", file=base64.b64encode(FULL).decode(), name="letterboxd.zip")
        self.assertNotIn("poll", r)
        self.assertEqual(self.looked, [])

    def test_one_list_uploaded_later_joins_the_export(self):
        self.call("upload", file=base64.b64encode(export(**{"watchlist.csv": WATCHLIST})).decode(), name="a.zip")
        self.call("upload", file=base64.b64encode(LIST.encode()).decode(), name="beste-filme.csv")
        d = main.read_json(main.user_file("anna"), {})
        self.assertEqual(len(d["watchlist"]), 3)
        self.assertEqual([l["name"] for l in d["lists"]], ["Beste Filme"])

    def test_letterboxd_down_is_said_and_tried_again(self):
        def down(url, *a):
            raise OSError("unreachable")
        self.call("upload", file=base64.b64encode(FULL).decode(), name="a.zip")
        with mock.patch.object(main, "get", down):
            self.assertIn("nicht erreichbar", self.call("lookup")["error"])
        self.assertEqual(self.call("lookup")["message"], "Alle Filme nachgeschlagen.")


if __name__ == "__main__":
    unittest.main()
