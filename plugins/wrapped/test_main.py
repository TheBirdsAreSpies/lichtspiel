"""Tests for the Wrapped plugin: python3 -I -m unittest discover -s plugins/wrapped"""
import datetime
import io
import json
import os
import shutil
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stdout
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

YEAR = datetime.date.today().year


def at(month, day, hour, minute=0, year=YEAR):
    return int(time.mktime(datetime.datetime(year, month, day, hour, minute).timetuple()))


def movie(i, title, genres, minutes=120):
    return {"id": i, "type": "movie", "title": title, "year": 1995, "duration_ms": minutes * 60000, "genres": genres}


def episode(i, show, sid, season, ep, minutes=45):
    return {"id": i, "type": "episode", "title": "x", "season": season, "episode": ep, "duration_ms": minutes * 60000, "genres": [],
            "show": {"id": sid, "title": show, "genres": ["Drama", "Mystery"]}}


class WrappedTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir, True)
        for name, value in (("STATE", os.path.join(self.dir, "state.json")), ("LOCK", os.path.join(self.dir, ".lock"))):
            p = mock.patch.object(main, name, value)
            p.start()
            self.addCleanup(p.stop)

    def watch(self, item, when, user="anna", **extra):
        return main.note(dict({"user": user, "item": item}, **extra), now=when)

    def panel(self, lang="en", user="anna", year=None, action="panel"):
        body = {"lang": lang}
        if year:
            body["args"] = {"year": year}
        stdin = io.TextIOWrapper(io.BytesIO(json.dumps({"event": "api.call", "settings": {"binge_gap_hours": 3},
                                                        "data": {"action": action, "user": user, "body": body}}).encode()))
        out = io.StringIO()
        with mock.patch.object(sys, "stdin", stdin), redirect_stdout(out):
            main.main()
        return json.loads(out.getvalue().strip().splitlines()[-1])

    def year(self):
        self.watch(movie(1, "Heat", ["Crime", "Drama"]), at(1, 3, 20))
        self.watch(movie(2, "Up", ["Animation"], 96), at(2, 10, 18))
        # A binge: 5 episodes of Lost on one evening, then one more the next week.
        for k in range(5):
            self.watch(episode(10 + k, "Lost", 7, 1, k + 1), at(3, 7, 18) + k * 50 * 60)
        self.watch(episode(15, "Lost", 7, 1, 6), at(3, 14, 20))
        # Dark: 3 over two evenings (more than 3 hours apart: two runs).
        self.watch(episode(20, "Dark", 8, 1, 1), at(4, 1, 20))
        self.watch(episode(21, "Dark", 8, 1, 2), at(4, 1, 21))
        self.watch(episode(22, "Dark", 8, 1, 3), at(4, 2, 21))

    def test_the_year(self):
        self.year()
        p = self.panel()
        self.assertEqual(p["title"], "Your %d" % YEAR)
        items = {i["title"]: i for i in p["items"]}
        first = p["items"][0]
        # 120 + 96 + 9 × 45 = 621 minutes ≈ 10 hours
        self.assertEqual(first["title"], "10 hours")
        self.assertEqual(first["subtitle"], "watched · 2 movies, 9 episodes of 2 shows")
        self.assertEqual(items["Favourite genres"]["subtitle"], "Drama, Mystery, Crime")
        self.assertEqual(items["Most watched show"]["subtitle"], "Lost · 6 episodes")
        self.assertEqual((items["Most watched show"]["image"], items["Most watched show"]["href"]), ("/library/metadata/7/thumb", "#/item/7"))
        self.assertEqual(items["Also a lot"]["subtitle"], "Dark (3)")
        self.assertEqual(items["Longest binge"]["subtitle"], "Lost · 5 episodes in a row on 7 March")
        self.assertEqual(items["Busiest month"]["subtitle"], "March · 5 hours")
        self.assertEqual(items["First of the year"]["subtitle"], "Heat (1995) · 3 January")
        self.assertEqual(items["Latest"]["subtitle"], "Dark · S01E03 · 2 April")

    def test_in_german(self):
        self.year()
        items = {i["title"]: i for i in self.panel("de")["items"]}
        self.assertEqual(items["Längster Marathon"]["subtitle"], "Lost · 5 Folgen am Stück am 7. März")
        self.assertEqual(items["Stärkster Monat"]["subtitle"], "März · 5 Stunden")

    def test_what_does_not_count(self):
        self.assertFalse(self.watch(movie(1, "Heat", []), at(1, 3, 20), manual=True))
        self.assertFalse(self.watch({"id": 3, "type": "track", "title": "Song"}, at(1, 3, 20)))
        # The same title reported twice by two players counts once.
        self.assertTrue(self.watch(movie(1, "Heat", []), at(1, 3, 20)))
        self.watch(movie(1, "Heat", []), at(1, 3, 20, 2))
        self.assertEqual(len(main.load()["people"]["anna"]["seen"]), 1)
        # Others see only their own year.
        p = self.panel(user="bob")
        self.assertIn("Nothing finished", p["text"])
        self.assertNotIn("items", p)

    def test_earlier_years(self):
        self.watch(movie(1, "Heat", ["Crime"]), at(6, 1, 20, year=YEAR - 1))
        self.watch(movie(2, "Up", ["Animation"]), at(1, 2, 20))
        p = self.panel()
        self.assertEqual([b["label"] for b in p["buttons"]], [str(YEAR), str(YEAR - 1)])
        r = self.panel(year=YEAR - 1, action="year")["panel"]
        self.assertEqual(r["title"], "Your %d" % (YEAR - 1))
        self.assertEqual(r["items"][0]["subtitle"], "watched · 1 movie")
        # Years long gone are dropped.
        self.watch(movie(3, "Old", []), at(1, 1, 20, year=YEAR - 10))
        self.watch(movie(4, "New", []), at(1, 5, 20))
        self.assertNotIn("Old", [e["title"] for e in main.load()["people"]["anna"]["seen"]])


if __name__ == "__main__":
    unittest.main()
