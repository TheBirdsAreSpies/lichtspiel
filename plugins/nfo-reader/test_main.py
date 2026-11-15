"""Tests for the NFO reader: python3 -I -m unittest discover -s plugins/nfo-reader"""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main  # noqa: E402


class NfoTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, rel, text):
        path = os.path.join(self.dir, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        return path

    def test_kodi_nfo_with_a_link_after_the_xml_is_read(self):
        path = self.write("movie.nfo", "<movie><title>Kodi</title><plot>good plot</plot><year>2001</year></movie>\n"
                                       "https://www.themoviedb.org/movie/603\n")
        self.assertEqual(main.read_nfo(path), {"title": "Kodi", "summary": "good plot", "year": 2001})

    def test_plain_nfo_still_works(self):
        path = self.write("movie.nfo", "<?xml version='1.0'?>\n<movie><title>Plain</title></movie>\n")
        self.assertEqual(main.read_nfo(path), {"title": "Plain"})

    def test_declared_encoding_is_used(self):
        path = os.path.join(self.dir, "movie.nfo")
        with open(path, "wb") as f:
            f.write("<?xml version='1.0' encoding='ISO-8859-1'?>\n<movie><title>Müller</title></movie>\n".encode("latin-1"))
        self.assertEqual(main.read_nfo(path), {"title": "Müller"})

    def test_only_a_link_gives_nothing(self):
        self.assertIsNone(main.read_nfo(self.write("movie.nfo", "https://www.imdb.com/title/tt0133093/\n")))

    def test_episode_without_season_folder_looks_in_its_own_show_folder_first(self):
        ep = os.path.join(self.dir, "Show", "Show S01E01.mkv")
        got = main.candidates({"kind": "episode", "file": {"path": ep}})
        show = os.path.join(self.dir, "Show", "tvshow.nfo")
        root = os.path.join(self.dir, "tvshow.nfo")
        self.assertIn(show, got)
        self.assertLess(got.index(show), got.index(root) if root in got else len(got))

    def test_episode_in_season_folder_finds_tvshow_nfo_one_level_up(self):
        ep = os.path.join(self.dir, "Show", "Season 01", "Show S01E01.mkv")
        got = main.candidates({"kind": "episode", "file": {"path": ep}})
        self.assertIn(os.path.join(self.dir, "Show", "tvshow.nfo"), got)


if __name__ == "__main__":
    unittest.main()
