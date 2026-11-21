import io
import json
import os
import sys
import tempfile
import time
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

AOT = {
    "id": 16498, "idMal": 16498, "siteUrl": "https://anilist.co/anime/16498", "format": "TV", "episodes": 25,
    "averageScore": 85, "popularity": 900000, "isAdult": False, "genres": ["Action", "Drama"],
    "title": {"romaji": "Shingeki no Kyojin", "english": "Attack on Titan", "native": "進撃の巨人"},
    "synonyms": ["SnK", "AoT"], "startDate": {"year": 2013},
    "description": "Several hundred years ago, humans were nearly exterminated.<br><br>\r\nEren &amp; <i>Mikasa</i>.\n\n(Source: Kodansha)",
    "studios": {"nodes": [{"name": "Pony Canyon", "isAnimationStudio": False}, {"name": "WIT STUDIO", "isAnimationStudio": True}]},
}
AOT_RECAP = dict(AOT, id=1, idMal=1, format="SPECIAL", popularity=10, startDate={"year": 2013})
AOT_FINAL = dict(AOT, id=110277, idMal=40028, popularity=500000, startDate={"year": 2020},
                 title={"romaji": "Shingeki no Kyojin: The Final Season", "english": "Attack on Titan Final Season", "native": "進撃の巨人 The Final Season"}, synonyms=[])

SHOW = {"kind": "show", "title": "Attack on Titan", "year": 2013, "section_name": "Serien", "summary": "Vor über 100 Jahren …",
        "genres": ["Animation", "Action & Adventure"],
        "extra": {"original_title": "進撃の巨人", "file_title": "Attack on Titan", "ids": {"tmdb": "1429", "tvdb": 267440},
                  "ratings": {"tmdb": {"score": "8.7/10"}}}}


class Wanted(unittest.TestCase):
    def test_a_japanese_animated_title_counts_without_libraries(self):
        self.assertTrue(main.wanted(SHOW, {}))

    def test_genres_in_other_languages_count(self):
        self.assertTrue(main.wanted(dict(SHOW, genres=["Animación"]), {}))

    def test_a_western_cartoon_or_a_japanese_live_action_movie_does_not(self):
        self.assertFalse(main.wanted(dict(SHOW, extra={"original_title": "Arcane"}), {}))
        self.assertFalse(main.wanted(dict(SHOW, genres=["Drama"]), {}))

    def test_named_libraries_take_every_title_there_and_nothing_else(self):
        bare = {"kind": "show", "title": "Frieren", "section_name": "Anime", "extra": {}}
        self.assertTrue(main.wanted(bare, {"libraries": ["anime"]}))
        self.assertFalse(main.wanted(SHOW, {"libraries": ["Anime"]}))

    def test_seasons_and_episodes_are_left_alone(self):
        self.assertFalse(main.wanted(dict(SHOW, kind="episode"), {}))
        self.assertFalse(main.wanted(dict(SHOW, kind="season"), {}))


class Matching(unittest.TestCase):
    def test_the_same_name_in_any_spelling_with_the_year_matches(self):
        self.assertEqual(main.best_match([AOT_FINAL, AOT], ["進撃の巨人"], 2013, "show")["id"], 16498)
        self.assertEqual(main.best_match([AOT], ["Attack on Titan!"], 2014, "show")["id"], 16498)

    def test_a_recap_special_of_the_same_name_loses_to_the_show(self):
        self.assertEqual(main.best_match([AOT_RECAP, AOT], ["Attack on Titan"], 2013, "show")["id"], 16498)

    def test_a_year_far_off_or_the_wrong_kind_does_not_match(self):
        self.assertIsNone(main.best_match([AOT], ["Attack on Titan"], 2020, "show"))
        self.assertIsNone(main.best_match([AOT], ["Attack on Titan"], 2013, "movie"))

    def test_a_nearly_same_name_needs_the_same_year(self):
        self.assertEqual(main.best_match([AOT], ["Attack on Titans"], 2013, "show")["id"], 16498)
        self.assertIsNone(main.best_match([AOT], ["Attack on Titans"], None, "show"))
        self.assertIsNone(main.best_match([AOT], ["Attack of the Giants"], 2013, "show"))


class Answer(unittest.TestCase):
    def test_ids_score_and_studio_are_added_and_the_rest_kept(self):
        meta = main.answer(SHOW, AOT, {})["metadata"]
        self.assertEqual(meta["extra"]["ids"], {"tmdb": "1429", "tvdb": 267440, "anilist": 16498, "mal": 16498})
        self.assertEqual(meta["extra"]["ratings"]["tmdb"], {"score": "8.7/10"})
        self.assertEqual(meta["extra"]["ratings"]["anilist"]["score"], "85%")
        self.assertEqual(meta["studio"], "WIT STUDIO")
        self.assertNotIn("title", meta)
        self.assertNotIn("genres", meta)
        self.assertNotIn("summary", meta)  # the show has one already

    def test_title_summary_and_genres_as_the_settings_say(self):
        meta = main.answer(dict(SHOW, summary=""), AOT, {"title": "romaji", "genres": True, "studio": False})["metadata"]
        self.assertEqual(meta["title"], "Shingeki no Kyojin")
        self.assertEqual(meta["genres"], ["Action", "Drama"])
        self.assertNotIn("studio", meta)
        self.assertEqual(meta["summary"], "Several hundred years ago, humans were nearly exterminated.\n\nEren & Mikasa.")
        self.assertIn("summary", main.answer(SHOW, AOT, {"summary": "always"})["metadata"])
        self.assertNotIn("summary", main.answer(dict(SHOW, summary=""), AOT, {"summary": "never"})["metadata"])


class Run(unittest.TestCase):
    def setUp(self):
        d = tempfile.mkdtemp()
        self.cache = os.path.join(d, "cache.json")
        self.p = mock.patch.object(main, "CACHE", self.cache)
        self.p.start()

    def tearDown(self):
        self.p.stop()

    def run_main(self, data, settings=None):
        stdin = io.TextIOWrapper(io.BytesIO(json.dumps({"event": "metadata.lookup", "data": data, "settings": settings or {}}).encode()))
        out = io.StringIO()
        with mock.patch.object(sys, "stdin", stdin), mock.patch.object(sys, "stdout", out):
            main.main()
        return json.loads(out.getvalue().strip().splitlines()[-1])

    def test_a_search_is_asked_once_and_then_kept(self):
        calls = []

        def fake(q, v):
            calls.append(v["search"])
            return {"data": {"Page": {"media": [AOT]}}}
        with mock.patch.object(main, "post", fake):
            a = self.run_main(SHOW)
            b = self.run_main(SHOW)
        self.assertEqual(a, b)
        self.assertEqual(a["metadata"]["extra"]["ids"]["anilist"], 16498)
        self.assertEqual(calls, ["進撃の巨人"])

    def test_other_names_are_tried_when_the_first_finds_nothing(self):
        calls = []

        def fake(q, v):
            calls.append(v["search"])
            return {"data": {"Page": {"media": [AOT] if v["search"] == "Attack on Titan" else []}}}
        with mock.patch.object(main, "post", fake):
            a = self.run_main(dict(SHOW, extra=dict(SHOW["extra"], file_title="Shingeki (2013) [1080p]")))
        self.assertEqual(calls, ["進撃の巨人", "Shingeki (2013) [1080p]", "Attack on Titan"])
        self.assertIn("metadata", a)

    def test_a_miss_is_kept_a_week_and_no_internet_says_so(self):
        with mock.patch.object(main, "post", lambda q, v: {"data": {"Page": {"media": []}}}):
            self.assertEqual(self.run_main(SHOW), {"found": False})
        with open(self.cache, encoding="utf-8") as f:
            old = json.load(f)
        for v in old.values():
            v["at"] = time.time() - 8 * 86400
        with open(self.cache, "w", encoding="utf-8") as f:
            json.dump(old, f)

        def offline(q, v):
            raise main.urllib.error.URLError("no network")
        with mock.patch.object(main, "post", offline):
            self.assertIn("error", self.run_main(SHOW))

    def test_a_title_that_is_no_anime_asks_nothing(self):
        with mock.patch.object(main, "post", lambda q, v: self.fail("asked AniList")):
            self.assertEqual(self.run_main(dict(SHOW, genres=["Drama"])), {"skipped": True})


if __name__ == "__main__":
    unittest.main()
