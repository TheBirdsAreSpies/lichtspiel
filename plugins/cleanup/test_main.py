"""Tests for the clean-up suggestions: python3 -I -m unittest discover -s plugins/cleanup"""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

NOW = 1_800_000_000
DAY = 86400
GB = 1_000_000_000
SETTINGS = {"unwatched_months": 12, "people": [], "skip_libraries": []}


def movie(id, title, size=GB, minutes=120, added=NOW - 800 * DAY, last=0, watched=(), started=(), section=1, **kw):
    it = {"id": id, "type": "movie", "title": title, "year": 2000, "section_id": section, "duration_ms": minutes * 60_000,
          "added_at": added, "last_played": last, "watched_by": list(watched), "in_progress_by": list(started),
          "files": [{"path": "/media/Movies/%s.mkv" % title, "size": size}]}
    it.update(kw)
    return it


def episode(id, show_id, number, size=GB // 2, added=NOW - 800 * DAY, last=0, watched=()):
    return {"id": id, "type": "episode", "title": "Folge %s" % number, "season": 1, "episode": number, "section_id": 2,
            "show": {"id": show_id, "title": "Dark", "year": 2017}, "duration_ms": 50 * 60_000, "added_at": added,
            "last_played": last, "watched_by": list(watched), "in_progress_by": [],
            "files": [{"path": "/media/Shows/Dark/S01E%02d.mkv" % number, "size": size}]}


def library(*items, users=("anna", "ben")):
    return {"sections": [{"id": 1, "name": "Movies"}, {"id": 2, "name": "Shows"}, {"id": 3, "name": "Kids"}],
            "users": list(users), "items": list(items)}


def ids(found, c):
    return [e["id"] for e in found[c]]


class Checks(unittest.TestCase):
    def test_long_unwatched_titles_are_found_biggest_first(self):
        found, total = main.check(library(
            movie(1, "Old", size=2 * GB),
            movie(2, "Bigger", size=9 * GB, last=NOW - 400 * DAY, watched=["anna"]),
            movie(3, "Seen lately", last=NOW - 10 * DAY, watched=["anna"]),
            movie(4, "New", added=NOW - 20 * DAY),
            movie(5, "Started long ago", started=["ben"], last=NOW - 500 * DAY),
        ), SETTINGS, now=NOW)
        self.assertEqual(total, 5)
        self.assertEqual(ids(found, "unwatched"), [2, 1, 5])

    def test_a_show_is_one_title_and_a_new_episode_keeps_it(self):
        old = [episode(10, 50, 1), episode(11, 50, 2)]
        found, total = main.check(library(*old), SETTINGS, now=NOW)
        self.assertEqual(total, 1)
        self.assertEqual(found["unwatched"][0]["id"], 50)
        self.assertEqual(found["unwatched"][0]["episodes"], 2)
        self.assertEqual(found["unwatched"][0]["size"], GB)
        found, _ = main.check(library(*old, episode(12, 50, 3, added=NOW - 5 * DAY)), SETTINGS, now=NOW)
        self.assertEqual(found["unwatched"], [])

    def test_everyone_seen_needs_every_person_and_a_rest(self):
        found, _ = main.check(library(
            movie(1, "Both", watched=["anna", "ben"], last=NOW - 60 * DAY),
            movie(2, "Only Anna", watched=["anna"], last=NOW - 60 * DAY),
            movie(3, "Both lately", watched=["anna", "ben"], last=NOW - 5 * DAY),
            movie(4, "Both, Ben again", watched=["anna", "ben"], started=["ben"], last=NOW - 60 * DAY),
        ), SETTINGS, now=NOW)
        self.assertEqual(ids(found, "everyone_seen"), [1])
        # Only the people named count.
        found, _ = main.check(library(movie(2, "Only Anna", watched=["anna"], last=NOW - 60 * DAY)),
                              dict(SETTINGS, people=["anna"]), now=NOW)
        self.assertEqual(ids(found, "everyone_seen"), [2])

    def test_a_show_is_seen_by_someone_only_with_every_episode(self):
        found, _ = main.check(library(
            episode(10, 50, 1, watched=["anna", "ben"], last=NOW - 90 * DAY),
            episode(11, 50, 2, watched=["anna"], last=NOW - 90 * DAY),
        ), SETTINGS, now=NOW)
        self.assertEqual(found["everyone_seen"], [])

    def test_biggest_for_running_time(self):
        found, _ = main.check(library(
            movie(1, "Remux", size=40 * GB, minutes=120, added=NOW),
            movie(2, "Small", size=2 * GB, minutes=120, added=NOW),
            movie(3, "Long", size=30 * GB, minutes=240, added=NOW),
            movie(4, "No length", size=50 * GB, minutes=0, added=NOW),
        ), SETTINGS, now=NOW)
        self.assertEqual(ids(found, "biggest"), [1, 3, 2])
        self.assertEqual(found["biggest"][0]["rate"], 20 * GB)

    def test_versions_and_skipped_libraries_are_left_out(self):
        found, total = main.check(library(
            movie(1, "Heat"), movie(2, "Heat 4K", version_of=1), movie(3, "Kids film", section=3),
        ), dict(SETTINGS, skip_libraries=["kids"]), now=NOW)
        self.assertEqual(total, 1)
        self.assertEqual(ids(found, "unwatched"), [1])


class Panel(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.saved = (main.REPORT, main.KEPT, main.LOCK)
        main.REPORT, main.KEPT, main.LOCK = (os.path.join(self.dir, n) for n in ("report.json", "kept.json", ".lock"))
        found, total = main.check(library(movie(1, "Old", size=3 * GB), movie(2, "Older", size=GB)), SETTINGS, now=NOW)
        main.write_json(main.REPORT, {"at": NOW, "total": total, "found": found})

    def tearDown(self):
        main.REPORT, main.KEPT, main.LOCK = self.saved

    def call(self, action, args=None, admin=True, may_delete=True, lang="en"):
        return main.answer_call({"action": action, "admin": admin, "may_delete": may_delete,
                                 "body": {"lang": lang, "args": args or {}}}, SETTINGS, {})

    def test_only_admins(self):
        self.assertIn("error", self.call("panel", admin=False))

    def test_the_overview_counts_each_list(self):
        p = self.call("panel", lang="de")
        self.assertEqual(p["title"], "Aufräum-Vorschläge")
        self.assertEqual(p["items"][0]["status"], "2 Titel · 4,0 GB")

    def test_delete_is_offered_only_when_allowed(self):
        shown = self.call("show", {"check": "unwatched"})["panel"]
        self.assertEqual([b["action"] for b in shown["items"][0]["buttons"]], ["keep", "delete"])
        self.assertEqual(shown["items"][0]["href"], "#/item/1")
        shown = self.call("show", {"check": "unwatched"}, may_delete=False)["panel"]
        self.assertEqual([b["action"] for b in shown["items"][0]["buttons"]], ["keep"])
        self.assertIn("error", self.call("delete", {"id": 1, "check": "unwatched"}, may_delete=False))

    def test_delete_asks_the_server_and_takes_it_off_every_list(self):
        r = self.call("delete", {"id": 1, "check": "unwatched"})
        self.assertEqual(r["delete_media"], [1])
        self.assertEqual([i["title"] for i in r["panel"]["items"]], ["Older (2000)"])
        report = main.read_json(main.REPORT, {})
        self.assertFalse(any(e["id"] == 1 for v in report["found"].values() for e in v))

    def test_a_kept_title_stays_off_until_shown_again(self):
        self.call("keep", {"key": "movie:1", "check": "unwatched"})
        self.assertEqual(self.call("panel")["items"][0]["status"], "1 titles · 1.0 GB")
        self.call("unkeep")
        self.assertEqual(self.call("panel")["items"][0]["status"], "2 titles · 4.0 GB")


if __name__ == "__main__":
    unittest.main()
