"""Tests for smart collections: python3 -I -m unittest discover -s plugins/smart-collections"""
import os
import shutil
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402

PAGE = """
<div class="react-component" data-component-class="LazyPoster" data-item-name="The Godfather (1972)" data-item-slug="the-godfather" data-item-link="/film/the-godfather/"></div>
<div class="react-component" data-component-class="LazyPoster" data-item-name="Am&#039;lie (2001)" data-item-slug="amelie" data-item-link="/film/amelie/"></div>
<div class="react-component" data-item-name="The Godfather (1972)" data-item-slug="the-godfather"></div>
<div class="react-component" data-item-name="Untitled" data-item-slug="untitled-2"></div>
"""


def movie(id, title, year, tmdb=None, imdb=None, section=1, **kw):
    ids = {}
    if tmdb:
        ids["tmdb"] = tmdb
    if imdb:
        ids["imdb"] = imdb
    it = {"id": id, "type": "movie", "title": title, "year": year, "ids": ids, "section_id": section}
    it.update(kw)
    return it


LIBRARY = {"sections": [{"id": 1, "name": "Filme", "kind": "movie"}, {"id": 2, "name": "Kinder", "kind": "movie"}, {"id": 3, "name": "Serien", "kind": "show"}],
           "items": [movie(1, "Der Pate", 1972, tmdb=238), movie(2, "Die fabelhafte Welt der Amélie", 2001, imdb="tt0211915"),
                     movie(3, "Parasite", 2019), movie(4, "Der Pate (4K)", 1972, tmdb=238, version_of=1),
                     movie(5, "Oben", 2009, tmdb=14160, section=2), {"id": 6, "type": "episode", "title": "Pilot", "section_id": 3, "ids": {"tmdb": 1}}]}


class Lists(unittest.TestCase):
    def test_a_letterboxd_page_gives_its_films_once_in_order(self):
        self.assertEqual(main.parse_letterboxd_page(PAGE), [
            {"slug": "the-godfather", "title": "The Godfather", "year": 1972},
            {"slug": "amelie", "title": "Am'lie", "year": 2001},
            {"slug": "untitled-2", "title": "Untitled", "year": None}])

    def test_films_are_found_by_tmdb_imdb_or_title_and_year(self):
        index = main.library_index(LIBRARY, {})
        films = [{"tmdb": 238, "title": "The Godfather"}, {"imdb": "tt0211915"}, {"title": "Parasite", "year": 2019},
                 {"title": "Parasite", "year": 2020}, {"tmdb": 14160}, {"tmdb": 1}]
        # A version is not a title of its own; an episode is no movie.
        self.assertEqual(main.match(films, index), [1, 2, 3, 5])
        # Only the libraries named.
        self.assertEqual(main.match(films, main.library_index(LIBRARY, {"libraries": ["filme"]})), [1, 2, 3])

    def test_letterboxd_films_are_looked_up_once_and_then_cached(self):
        pages = {"https://letterboxd.com/x/list/y/": PAGE}
        looked = []

        def fake_get(url, *a, **k):
            if "/film/" in url:
                looked.append(url)
                return '<div data-tmdb-id="238"></div> <a href="https://www.imdb.com/title/tt0068646/">' if "godfather" in url else ""
            return pages.get(url)
        cache = {"amelie": {"tmdb": 194, "imdb": "tt0211915"}}
        with mock.patch.object(main, "get", fake_get), mock.patch.object(main, "PAUSE", 0):
            films = main.films_of("https://letterboxd.com/x/list/y", {}, cache, [1])
        self.assertEqual([(f["slug"], f.get("tmdb")) for f in films], [("the-godfather", 238), ("amelie", 194), ("untitled-2", None)])
        # One new lookup allowed: the third waits for the next run.
        self.assertEqual(len(looked), 1)
        self.assertEqual(cache["the-godfather"], {"tmdb": 238, "imdb": "tt0068646"})
        self.assertNotIn("untitled-2", cache)

    def test_wikidata_is_asked_once_a_week_and_its_last_answer_used_while_it_does_not_answer(self):
        asked = []

        def fake_ask(award):
            asked.append(award)
            if len(asked) > 1:
                raise main.urllib.error.HTTPError("https://query.wikidata.org/sparql", 429, "Too Many Requests", {}, None)
            return [{"imdb": "tt0068646", "tmdb": 238}]
        cache = {}
        with mock.patch.object(main, "ask_wikidata", fake_ask):
            self.assertEqual(main.wikidata_award("Q102427", cache), [{"imdb": "tt0068646", "tmdb": 238}])
            self.assertEqual(main.wikidata_award("Q102427", cache), [{"imdb": "tt0068646", "tmdb": 238}])
            self.assertEqual(len(asked), 1)
            cache["wikidata:Q102427"]["at"] -= 8 * 86400
            # Older than a week: asked again; Wikidata says 429, so the last answer is used.
            self.assertEqual(main.wikidata_award("Q102427", cache), [{"imdb": "tt0068646", "tmdb": 238}])
            self.assertEqual(len(asked), 2)
            # Nothing kept yet: the error is shown.
            with self.assertRaises(main.urllib.error.HTTPError):
                main.wikidata_award("Q19020", {})

    def test_a_site_that_says_how_long_to_wait_is_waited_for_at_most_two_minutes(self):
        def err(after):
            return main.urllib.error.HTTPError("u", 429, "Too Many Requests", {"Retry-After": after} if after else {}, None)
        self.assertEqual(main.wait_for(err(None), 0), 5)
        self.assertEqual(main.wait_for(err("60"), 0), 60)
        self.assertEqual(main.wait_for(err("3600"), 1), 120)
        self.assertEqual(main.wait_for(err("Wed, 21 Oct 2026 07:28:00 GMT"), 1), 10)

    def test_presets_and_unknown_sources(self):
        with mock.patch.object(main, "letterboxd_films", lambda url, c, b: [{"url": url}]), mock.patch.object(main, "wikidata_award", lambda a, c: [{"award": a}]):
            self.assertEqual(main.films_of("imdb-top-250", {}, {}, [0]), [{"url": "https://letterboxd.com/dave/list/imdb-top-250/"}])
            self.assertEqual(main.films_of("oscar-best-picture", {}, {}, [0]), [{"award": "Q102427"}])
            self.assertEqual(main.films_of("https://letterboxd.com/anna/watchlist/", {}, {}, [0]), [{"url": "https://letterboxd.com/anna/watchlist/"}])
        with self.assertRaisesRegex(ValueError, "TMDB key"):
            main.films_of("tmdb-popular", {}, {}, [0])
        for bad in ("https://evil.example/list/", "http://letterboxd.com/a/list/b/", "imdb"):
            with self.assertRaisesRegex(ValueError, "unknown source"):
                main.films_of(bad, {}, {}, [0])


class Runs(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.patches = [mock.patch.object(main, n, os.path.join(self.dir, f)) for n, f in (("STATE", "state.json"), ("CACHE", "cache.json"), ("LOCK", ".lock"))]
        self.patches.append(mock.patch.object(main, "fetch_library", lambda server: LIBRARY))
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in self.patches:
            p.stop()
        shutil.rmtree(self.dir, ignore_errors=True)

    def films(self, source, settings, cache, budget):
        if source == "oscar-best-picture":
            return [{"tmdb": 238}, {"imdb": "tt9999999"}]
        if source == "imdb-top-250":
            return [{"tmdb": 238}, {"imdb": "tt0211915"}, {"tmdb": 496243}]
        raise OSError("letterboxd.com could not be reached")

    def test_each_list_becomes_a_collection_and_a_list_taken_out_is_emptied_once(self):
        settings = {"lists": [{"name": "IMDb Top 250", "source": "imdb-top-250"}, {"name": "Oscars", "source": "oscar-best-picture"},
                              {"name": "Down", "source": "https://letterboxd.com/a/list/b/"}, {"name": "", "source": "x"}]}
        with mock.patch.object(main, "films_of", self.films):
            out, report = main.run({}, settings)
            # A list that could not be read leaves its collection alone.
            self.assertEqual(out, {"collections": {"IMDb Top 250": [1, 2], "Oscars": [1]}})
            self.assertEqual(report[2]["error"], "letterboxd.com could not be reached")
            self.assertEqual(main.read_json(main.STATE, {})["managed"], ["Down", "IMDb Top 250", "Oscars"])
            out, _ = main.run({}, {"lists": [{"name": "Oscars", "source": "oscar-best-picture"}]})
            self.assertEqual(out["collections"], {"Oscars": [1], "IMDb Top 250": [], "Down": []})
            out, _ = main.run({}, {"lists": [{"name": "Oscars", "source": "oscar-best-picture"}]})
            self.assertEqual(out["collections"], {"Oscars": [1]})

    def test_the_panel_shows_each_list_and_updates_on_a_click(self):
        settings = {"lists": [{"name": "Oscars", "source": "oscar-best-picture"}]}
        call = lambda action, admin=True: main.answer_call({"action": action, "admin": admin, "body": {"lang": "de"}}, settings, {})
        self.assertIn("error", call("panel", admin=False))
        self.assertEqual(call("panel")["items"][0]["status"], "Noch nicht aktualisiert.")
        with mock.patch.object(main, "films_of", self.films):
            r = call("update")
        self.assertEqual(r["collections"], {"Oscars": [1]})
        self.assertEqual(r["panel"]["items"][0]["status"], "1 von 2 in deinen Mediatheken")
        self.assertTrue(r["panel"]["text"].startswith("Aktualisiert: 1 Sammlungen."))
        self.assertEqual(main.answer_call({"action": "panel", "admin": True, "body": {"lang": "de"}}, {"lists": []}, {})["buttons"], [])


if __name__ == "__main__":
    unittest.main()
