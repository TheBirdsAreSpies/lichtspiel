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

    def test_wikidata_gives_each_film_once_with_its_title_and_year_and_an_old_answer_is_asked_again(self):
        answer = '{"results": {"bindings": [{"film": {"value": "Q1"}, "imdb": {"value": "tt0068646"}, "tmdb": {"value": "238"}, ' \
                 '"title": {"value": "The Godfather"}, "date": {"value": "1972-03-15T00:00:00Z"}}, {"film": {"value": "Q2"}}]}}'
        with mock.patch.object(main, "get", lambda url, accept: answer):
            films = main.ask_wikidata("Q102427")
        self.assertEqual(films, [{"imdb": "tt0068646", "tmdb": 238, "title": "The Godfather", "year": 1972},
                                 {"imdb": None, "tmdb": None, "title": None, "year": None}])
        # Kept from before there were titles: asked again although it is not a week old.
        cache = {"wikidata:Q102427": {"at": int(main.time.time()), "films": [{"imdb": "tt0068646", "tmdb": 238}]}}
        with mock.patch.object(main, "ask_wikidata", lambda award: films):
            self.assertEqual(main.wikidata_award("Q102427", cache), films)
        self.assertEqual(cache["wikidata:Q102427"]["v"], 2)

    def test_a_missing_film_is_shown_with_a_link_to_its_page(self):
        self.assertEqual(main.described({"slug": "the-godfather", "title": "The Godfather", "year": 1972}),
                         {"title": "The Godfather", "year": 1972, "link": "https://letterboxd.com/film/the-godfather/"})
        self.assertEqual(main.described({"imdb": "tt0068646"}), {"title": "tt0068646", "year": None, "link": "https://www.imdb.com/title/tt0068646/"})
        self.assertEqual(main.described({"tmdb": 238, "title": "Der Pate"})["link"], "https://www.themoviedb.org/movie/238")

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
        # A boxd.it share link is followed to the list it leads to.
        with mock.patch.object(main, "letterboxd_films", lambda url, c, b: [{"url": url}]):
            with mock.patch.object(main, "unshorten", lambda u: "https://letterboxd.com/a/list/b/" if "boxd.it" in u else None):
                self.assertEqual(main.films_of("https://boxd.it/bAS2M", {}, {}, [0]), [{"url": "https://letterboxd.com/a/list/b/"}])
            with mock.patch.object(main, "unshorten", lambda u: "https://evil.example/x" if "boxd.it" in u else None):
                with self.assertRaisesRegex(ValueError, "unknown source"):
                    main.films_of("https://boxd.it/abc", {}, {}, [0])
        self.assertIsNone(main.unshorten("https://letterboxd.com/a/list/b/"))
        for bad in ("https://evil.example/list/", "http://letterboxd.com/a/list/b/", "imdb"):
            with self.assertRaisesRegex(ValueError, "unknown source"):
                main.films_of(bad, {}, {}, [0])

    def test_tv_shows_are_found_through_their_episodes_and_kept_apart_from_movies(self):
        def ep(id, show, section=3, **kw):
            return {"id": id, "type": "episode", "title": "Pilot", "section_id": section, "ids": {"tmdb": 9000 + id}, "show": show, **kw}
        bb = {"id": 20, "title": "Breaking Bad", "year": 2008, "ids": {"tmdb": 1396, "imdb": "tt0903747"}}
        dark = {"id": 21, "title": "Dark", "year": 2017, "ids": {}}
        same_number = {"id": 22, "title": "Some Show", "year": 2001, "ids": {"tmdb": 238}}
        lib = dict(LIBRARY, items=LIBRARY["items"] + [ep(30, bb), ep(31, bb), ep(32, dark), ep(33, same_number), ep(34, bb, version_of=30),
                                                     ep(35, {"id": 23, "title": "Elsewhere", "year": 2000, "ids": {"tmdb": 5}}, section=1)])
        index = main.library_index(lib, {})
        shows = [{"kind": "show", "tmdb": 1396}, {"imdb": "tt0903747"}, {"kind": "show", "title": "Dark", "year": 2017},
                 {"kind": "show", "tmdb": 238}, {"kind": "show", "tmdb": 5}, {"title": "Dark", "year": 2017}]
        # A show once (not each episode); TMDB's movie 238 is not its show 238; an episode in a movie library is not a show;
        # a title with no kind is a movie.
        self.assertEqual([main.titles_of(f, index) for f in shows], [[20], [20], [21], [22], [], []])
        self.assertEqual(main.titles_of({"tmdb": 238}, index), [1])
        # Only the libraries named.
        self.assertEqual(main.match(shows, main.library_index(lib, {"libraries": ["Filme"]})), [])
        self.assertEqual(main.match(shows, main.library_index(lib, {"libraries": ["serien"]})), [20, 21, 22])
        self.assertEqual(main.described({"kind": "show", "tmdb": 1396, "title": "Breaking Bad"})["link"], "https://www.themoviedb.org/tv/1396")

    def test_tmdb_charts_for_tv_and_a_tmdb_list_with_movies_and_shows(self):
        asked = []

        def fake_get(url, accept="text/html", headers=None):
            asked.append(url)
            if "/tv/popular" in url:
                return '{"results": [{"id": 1396, "name": "Breaking Bad", "first_air_date": "2008-01-20"}]}' if "page=1&" in url else '{"results": []}'
            if "/list/42" in url:
                page = int(main.re.search(r"page=(\d+)", url).group(1))
                return ('{"total_pages": 2, "items": [{"media_type": "movie", "id": 238, "title": "The Godfather", "release_date": "1972-03-14"}, '
                        '{"media_type": "person", "id": 7}]}' if page == 1 else
                        '{"total_pages": 2, "items": [{"media_type": "tv", "id": 1396, "name": "Breaking Bad", "first_air_date": ""}]}')
            return None
        with mock.patch.object(main, "get", fake_get):
            self.assertEqual(main.films_of("tmdb-tv-popular", {"tmdb_api_key": "k"}, {}, [0]),
                             [{"tmdb": 1396, "title": "Breaking Bad", "year": 2008, "kind": "show"}])
            self.assertEqual(len(asked), 5)
            self.assertEqual(main.films_of("https://www.themoviedb.org/list/42-my-favourites", {"tmdb_api_key": "k"}, {}, [0]),
                             [{"tmdb": 238, "title": "The Godfather", "year": 1972}, {"tmdb": 1396, "title": "Breaking Bad", "year": None, "kind": "show"}])
            self.assertEqual(len(asked), 7)
        with self.assertRaisesRegex(ValueError, "TMDB key"):
            main.films_of("https://www.themoviedb.org/list/42", {}, {}, [0])
        with self.assertRaisesRegex(ValueError, "unknown source"):
            main.films_of("https://www.themoviedb.org/movie/42", {"tmdb_api_key": "k"}, {}, [0])


class Runs(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.patches = [mock.patch.object(main, n, os.path.join(self.dir, f)) for n, f in (("STATE", "state.json"), ("CACHE", "cache.json"), ("LOCK", ".lock"),
                                                                                          ("PROGRESS", "progress.json"), ("PENDING", "pending.json"),
                                                                                          ("CHOICES", "choices.json"))]
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
            # A film you have twice (in two libraries) is one film of the list, in both titles.
            twice = dict(LIBRARY, items=LIBRARY["items"] + [movie(7, "Der Pate", 1972, tmdb=238, section=2)])
            with mock.patch.object(main, "fetch_library", lambda server: twice):
                out, report = main.run({}, {"lists": [{"name": "Oscars", "source": "oscar-best-picture"}]})
            self.assertEqual(out["collections"]["Oscars"], [1, 7])
            self.assertEqual((report[0]["found"], report[0]["total"]), (1, 2))
            # The playlist plays it once.
            self.assertEqual(report[0]["playlist"], [1])
            out, _ = main.run({}, {"lists": [{"name": "Oscars", "source": "oscar-best-picture"}]})
            self.assertEqual(out["collections"], {"Oscars": [1]})

    def test_a_missing_title_can_be_given_your_title_of_it_or_hidden_for_every_list(self):
        # The finale of a show, in cinemas as a film, that you have as the show's episode.
        finale = {"tmdb": 1500000, "title": "Stranger Things 5: The Finale", "year": 2025}
        st = {"id": 40, "title": "Stranger Things", "year": 2016, "ids": {"tmdb": 66732}}
        lib = dict(LIBRARY, items=LIBRARY["items"] + [
            {"id": 41, "type": "episode", "title": "Kapitel acht: Zurück auf der richtigen Seite", "season": 5, "episode": 8, "section_id": 3, "ids": {}, "show": st},
            {"id": 42, "type": "episode", "title": "Kapitel eins", "season": 1, "episode": 1, "section_id": 3, "ids": {}, "show": st}])
        lists = {"Popular": [{"tmdb": 238}, finale, {"tmdb": 77}], "Best": [finale, {"tmdb": 77}, {"tmdb": 14160}]}
        settings = {"lists": [{"name": n, "source": n} for n in lists]}
        server = {"library": "x"}

        def call(action, args=None, **body):
            return main.answer_call({"action": action, "admin": True, "body": dict(body, lang="de", args=args or {})}, settings, server)
        with mock.patch.object(main, "fetch_library", lambda server: lib), \
                mock.patch.object(main, "films_of", lambda source, *a: [dict(f) for f in lists[source]]):
            main.run(server, settings)
            m = call("missing", {"name": "Popular"})["panel"]
            self.assertEqual([i["title"] for i in m["items"]], ["Stranger Things 5: The Finale (2025)", "?"])
            pick = m["items"][0]["buttons"][0]
            about = {"name": "Popular", "key": "tmdb:movie:1500000", "title": "Stranger Things 5: The Finale (2025)"}
            self.assertEqual(pick["args"], about)
            # The search starts with the list's title; the show and its episodes are found by their names.
            p = call("pick", pick["args"])["panel"]
            self.assertEqual((p["input"]["value"], p["input"]["args"], p["items"]), ("Stranger Things 5: The Finale", about, []))
            self.assertIn("Nichts gefunden.", p["text"])
            p = call("find", pick["args"], query="stranger things s05e08")["panel"]
            self.assertEqual([(i["title"], i["subtitle"]) for i in p["items"]],
                             [("Stranger Things – S05E08 – Kapitel acht: Zurück auf der richtigen Seite", "Episode")])
            self.assertEqual([i["title"] for i in call("find", pick["args"], query="stranger")["panel"]["items"]][:2],
                             ["Stranger Things (2016)", "Stranger Things – S01E01 – Kapitel eins"])
            # Picked: it counts as had in every list and plays in its place, but an episode is no title of a collection.
            r = call("assign", p["items"][0]["buttons"][0]["args"])
            self.assertEqual(r["message"], "„Stranger Things 5: The Finale (2025)“ ist jetzt Stranger Things – S05E08 – Kapitel acht: Zurück auf der richtigen Seite.")
            self.assertEqual(r["collections"], {"Popular": [1], "Best": [5]})
            state = {x["name"]: x for x in main.read_json(main.STATE, {})["lists"]}
            self.assertEqual((state["Popular"]["playlist"], state["Popular"]["found"], state["Popular"]["total"]), ([1, 41], 2, 3))
            self.assertEqual(state["Best"]["playlist"], [41, 5])
            self.assertEqual([i.get("status") for i in r["panel"]["items"]],
                             [None, "ist Stranger Things – S05E08 – Kapitel acht: Zurück auf der richtigen Seite"])
            # It holds for the next run too; one you do not want is hidden (and no longer counted).
            main.run(server, settings)
            self.assertEqual(main.read_json(main.STATE, {})["lists"][0]["playlist"], [1, 41])
            hide = r["panel"]["items"][0]["buttons"][1]
            self.assertEqual(hide["action"], "hide")
            r = call("hide", hide["args"])
            state = {x["name"]: x for x in main.read_json(main.STATE, {})["lists"]}
            self.assertEqual((state["Popular"]["found"], state["Popular"]["total"], state["Popular"]["missing"]), (2, 2, []))
            self.assertEqual(r["panel"]["items"][-1]["status"], "ausgeblendet")
            self.assertIn("Fehlende zeigen", [b["label"] for b in call("panel")["items"][0]["buttons"]])
            # Undone: missing again.
            undo = r["panel"]["items"][0]["buttons"][0]
            self.assertEqual(undo["action"], "forget")
            r = call("forget", undo["args"])
            self.assertEqual(r["message"], "„Stranger Things 5: The Finale (2025)“ zählt wieder als fehlend.")
            self.assertEqual(r["collections"], {"Popular": [1], "Best": [5]})
            self.assertEqual(main.read_json(main.STATE, {})["lists"][1]["playlist"], [5])
            # While an update runs, the choice is kept and counts from the next one on.
            main.write_json(main.PROGRESS, {"at": int(main.time.time()), "i": 1, "n": 2, "name": "Popular"})
            r = call("assign", dict(about, id=41))
            self.assertNotIn("collections", r)
            self.assertTrue(r["message"].endswith("Eine Aktualisierung läuft: das gilt ab der nächsten."))
            self.assertEqual(call("assign", dict(about, id=999))["error"], "Dieser Titel ist nicht mehr in deinen Mediatheken.")

    def test_a_click_updates_in_the_background_and_the_panel_asks_until_it_is_done(self):
        settings = {"lists": [{"name": "Oscars", "source": "oscar-best-picture"}]}
        call = lambda action, admin=True: main.answer_call({"action": action, "admin": admin, "body": {"lang": "de"}}, settings, {"library": "x"})
        self.assertIn("error", call("panel", admin=False))
        self.assertEqual(call("panel")["items"][0]["status"], "Noch nicht aktualisiert.")
        started = []
        with mock.patch.object(main.subprocess, "Popen") as popen:
            r = call("update")
            started.append(popen.call_args)
            # Clicked again while it runs: no second run, only its progress.
            self.assertEqual(call("update")["poll"], {"action": "progress", "seconds": 5})
            self.assertEqual(popen.call_count, 1)
        self.assertEqual(r, {"wait": "Die Sammlungen werden aktualisiert …", "poll": {"action": "progress", "seconds": 5}})
        self.assertEqual(started[0].args[0][-1], "--update")
        self.assertTrue(call("panel")["text"].startswith("Eine Aktualisierung läuft."))
        # The run itself (what the started process does), stopped half way to see its progress.
        seen = []
        real_run = main.run

        def run_and_look(server, s, progress=None):
            def told(i, n, name):
                progress(i, n, name)
                seen.append(call("progress")["wait"])
            return real_run(server, s, told)
        with mock.patch.object(main, "films_of", self.films), mock.patch.object(main, "run", run_and_look):
            main.update_in_background(settings, {"library": "x"})
        self.assertEqual(seen, ["Wird aktualisiert: Liste 1 von 1 (Oscars) …"])
        # Done: the next call hands the collections to the server, once.
        r = call("progress")
        self.assertEqual(r["collections"], {"Oscars": [1]})
        self.assertEqual(r["message"], "Aktualisiert: 1 Sammlungen.")
        self.assertEqual(r["panel"]["items"][0]["status"], "1 von 2 in deinen Mediatheken")
        self.assertNotIn("poll", r)
        self.assertNotIn("collections", call("panel"))
        # Which films of the list you do not have: a button on the list shows them.
        item = call("panel")["items"][0]
        self.assertEqual(item["buttons"], [{"label": "Fehlende zeigen", "action": "missing", "args": {"name": "Oscars"}},
                                           {"label": "Als Playlist speichern", "action": "playlist", "args": {"name": "Oscars"}, "working": "Wird gespeichert …"}])
        # The list as a playlist: the titles you have, in the list's order.
        pl = main.answer_call({"action": "playlist", "admin": True, "body": {"lang": "de", "args": {"name": "Oscars"}}}, settings, {})
        self.assertEqual(pl["save_playlist"], {"title": "Smarte Sammlung: Oscars", "summary": "Aus den smarten Sammlungen", "ids": [1]})
        self.assertEqual(main.answer_call({"action": "playlist", "admin": True, "body": {"args": {"name": "Gibt es nicht"}}}, settings, {})["error"],
                         "Update the collections first.")
        m = main.answer_call({"action": "missing", "admin": True, "body": {"lang": "de", "args": {"name": "Oscars"}}}, settings, {})["panel"]
        self.assertEqual(m["title"], "Oscars: fehlt dir")
        self.assertEqual(m["text"], "1 Titel dieser Liste sind nicht in deinen Mediatheken.")
        about = {"name": "Oscars", "key": "imdb:tt9999999", "title": "tt9999999"}
        self.assertEqual(m["items"], [{"title": "tt9999999", "href": "https://www.imdb.com/title/tt9999999/",
                                       "buttons": [{"label": "Habe ich …", "action": "pick", "args": about},
                                                   {"label": "Ausblenden", "action": "hide", "args": about}]}])
        self.assertEqual(m["buttons"], [{"label": "Zurück", "action": "panel"}])
        # The page was closed in between: opening the panel hands them over.
        with mock.patch.object(main, "films_of", self.films):
            main.update_in_background(settings, {"library": "x"})
        p = call("panel")
        self.assertEqual(p["collections"], {"Oscars": [1]})
        self.assertTrue(p["text"].startswith("Aktualisiert: 1 Sammlungen."))
        # The first call after a run is a playlist: it hands the collections over too.
        with mock.patch.object(main, "films_of", self.films):
            main.update_in_background(settings, {"library": "x"})
        pl = main.answer_call({"action": "playlist", "admin": True, "body": {"lang": "de", "args": {"name": "Oscars"}}}, settings, {})
        self.assertEqual((pl["collections"], pl["message"]), ({"Oscars": [1]}, "Aktualisiert: 1 Sammlungen."))
        self.assertEqual(pl["save_playlist"]["ids"], [1])
        # A list none of whose films you have, or one that could not be read, says so.
        lists = main.read_json(main.STATE, {})
        lists["lists"] = [{"name": "Leer", "found": 0, "total": 3, "ids": [], "playlist": []}, {"name": "Down", "error": "x"}]
        main.write_json(main.STATE, lists)
        said = lambda name: main.answer_call({"action": "playlist", "admin": True, "body": {"lang": "de", "args": {"name": name}}}, settings, {})["error"]
        self.assertEqual(said("Leer"), "Keiner der Titel dieser Liste ist in deinen Mediatheken.")
        self.assertEqual(said("Down"), "Diese Liste konnte bei der letzten Aktualisierung nicht gelesen werden.")
        # A run that failed says why.
        with mock.patch.object(main, "fetch_library", mock.Mock(side_effect=LookupError())):
            main.update_in_background(settings, {})
        self.assertEqual(call("progress")["error"], "Dieses Plugin braucht einen Server, der Plugins die Mediathek lesen lässt.")
        # A run not heard from for 20 minutes has stopped.
        main.write_json(main.PROGRESS, {"at": 0, "i": 1, "n": 1, "name": "Oscars"})
        self.assertIsNone(main.running())
        self.assertEqual(main.answer_call({"action": "panel", "admin": True, "body": {"lang": "de"}}, {"lists": []}, {})["buttons"], [])


if __name__ == "__main__":
    unittest.main()
