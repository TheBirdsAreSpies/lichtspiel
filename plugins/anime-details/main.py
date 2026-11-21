"""Anime details plugin.

The server asks it about every new title, and again after the online details
were looked up ("metadata.lookup"). For a movie or show that is an anime it
finds the title on AniList (no key needed) and answers with what AniList knows
better: the AniList and MyAnimeList ids, the AniList score, the animation
studio and, if the settings say so, the title and the summary.

Which titles count as anime: every title in the libraries named in
`libraries`; with no libraries named, titles whose original title is Japanese
and that are animated (as the online details say).

Searches are kept in cache.json next to this file for a month, so AniList
(30 requests a minute) is asked once per title.
"""
import difflib
import html
import json
import os
import re
import sys
import time
import unicodedata
import urllib.error
import urllib.request

API = "https://graphql.anilist.co"
HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, "cache.json")
KEEP = 30 * 86400        # a search found something: asked again after a month (scores change)
KEEP_MISS = 7 * 86400    # found nothing: asked again after a week
UA = "Lichtspiel anime-details plugin"

QUERY = """query ($search: String) {
  Page(perPage: 10) {
    media(search: $search, type: ANIME, sort: SEARCH_MATCH) {
      id idMal siteUrl format episodes averageScore popularity isAdult genres
      title { romaji english native }
      synonyms
      startDate { year }
      description(asHtml: false)
      studios(isMain: true) { nodes { name isAnimationStudio } }
    }
  }
}"""

SHOW_FORMATS = {"TV", "TV_SHORT", "ONA", "OVA", "SPECIAL"}
MOVIE_FORMATS = {"MOVIE", "ONA", "OVA", "SPECIAL"}


# ---------------------------------------------------------------- what counts as anime

def japanese(s):
    """Hiragana, katakana or kanji in the text."""
    return any("぀" <= c <= "ヿ" or "一" <= c <= "鿿" for c in s or "")


def looks_like_anime(item):
    x = item.get("extra") or {}
    # TMDB's genre names are in the library's language: Animation, Animación, Animação, アニメーション …
    animated = any("anim" in g.lower() or "アニメ" in g for g in item.get("genres") or [])
    return animated and japanese(x.get("original_title") or "")


def wanted(item, settings):
    if item.get("kind") not in ("movie", "show"):
        return False
    libs = [str(l).strip().lower() for l in settings.get("libraries") or [] if str(l).strip()]
    if libs:
        return (item.get("section_name") or "").strip().lower() in libs
    return looks_like_anime(item)


# ---------------------------------------------------------------- matching

def norm(s):
    s = unicodedata.normalize("NFKC", s or "").lower().replace("&", "and")
    return "".join(c for c in s if c.isalnum())


def names_of(m):
    t = m.get("title") or {}
    return [n for n in [t.get("romaji"), t.get("english"), t.get("native")] + list(m.get("synonyms") or []) if n]


def year_ok(want, got):
    return not want or not got or abs(int(want) - int(got)) <= 1


def best_match(results, titles, year, kind):
    """The search result that is the title for sure, or None.

    A name that is the same (ignoring case, spaces and punctuation) and a year
    no more than one off; else a name that is nearly the same (90 %) with the
    year known on both sides and the same. Among several, the year that fits
    best, then the one more people know.
    """
    formats = SHOW_FORMATS if kind == "show" else MOVIE_FORMATS
    wants = [norm(t) for t in titles if norm(t)]
    exact, close = [], []
    for m in results:
        if m.get("format") not in formats:
            continue
        got_year = (m.get("startDate") or {}).get("year")
        names = [norm(n) for n in names_of(m)]
        if not year_ok(year, got_year):
            continue
        if any(w in names for w in wants):
            exact.append(m)
        elif year and got_year and int(year) == int(got_year) and any(
                difflib.SequenceMatcher(None, w, n).ratio() >= 0.9 for w in wants for n in names if n):
            close.append(m)
    pool = exact or close
    if not pool:
        return None

    def rank(m):
        got_year = (m.get("startDate") or {}).get("year")
        off = abs(int(year) - int(got_year)) if year and got_year else 0
        # A show's own first season, not a recap special of the same name.
        main = 0 if m.get("format") in ("TV", "MOVIE") else 1
        return (off, main, -(m.get("popularity") or 0))
    return sorted(pool, key=rank)[0]


# ---------------------------------------------------------------- AniList, with a cache

def load_cache():
    try:
        with open(CACHE, encoding="utf-8") as f:
            c = json.load(f)
        return c if isinstance(c, dict) else {}
    except (OSError, ValueError):
        return {}


def save_cache(c):
    now = time.time()
    c = {k: v for k, v in c.items() if now - v.get("at", 0) < KEEP}
    tmp = CACHE + ".%d.tmp" % os.getpid()
    try:
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(c, f)
        os.replace(tmp, CACHE)
    except OSError:
        try:
            os.remove(tmp)
        except OSError:
            pass


def post(query, variables):
    body = json.dumps({"query": query, "variables": variables}).encode("utf-8")
    req = urllib.request.Request(API, data=body, headers={
        "Content-Type": "application/json", "Accept": "application/json", "User-Agent": UA})
    for attempt in range(2):
        try:
            with urllib.request.urlopen(req, timeout=15) as r:
                return json.loads(r.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            # Too many requests: wait as long as AniList says, once, if that is not long.
            wait = int(e.headers.get("Retry-After") or 0) if e.code == 429 else 0
            if attempt == 0 and 0 < wait <= 20:
                time.sleep(wait)
                continue
            raise
    return None


def search(text, cache):
    """AniList's results for a name (from the cache when asked lately)."""
    key = "search:" + text.strip().lower()
    hit = cache.get(key)
    if hit and time.time() - hit.get("at", 0) < (KEEP if hit.get("results") else KEEP_MISS):
        return hit["results"]
    v = post(QUERY, {"search": text.strip()})
    results = (((v or {}).get("data") or {}).get("Page") or {}).get("media") or []
    cache[key] = {"at": time.time(), "results": results}
    return results


def find(item, cache):
    x = item.get("extra") or {}
    titles = [t for t in [x.get("original_title"), x.get("file_title"), item.get("title")] if isinstance(t, str) and t.strip()]
    seen = set()
    for t in titles:
        if t.strip().lower() in seen:
            continue
        seen.add(t.strip().lower())
        m = best_match(search(t, cache), titles, item.get("year"), item.get("kind"))
        if m:
            return m
    return None


# ---------------------------------------------------------------- the answer

def clean_text(s):
    """AniList's description as plain text: no <br>, <i> …, no "(Source: …)" line."""
    s = re.sub(r"<br\s*/?>", "\n", s or "", flags=re.I)
    s = html.unescape(re.sub(r"<[^>]+>", "", s))
    s = re.sub(r"\n?\s*\(Source:[^)]*\)\s*$", "", s.strip(), flags=re.I)
    s = re.sub(r"[ \t]+\n", "\n", s.replace("\r", ""))
    return re.sub(r"\n{3,}", "\n\n", s).strip()


def answer(item, m, settings):
    x = item.get("extra") or {}
    ids = dict(x.get("ids") or {}) if isinstance(x.get("ids"), dict) else {}
    ids["anilist"] = m["id"]
    if m.get("idMal"):
        ids["mal"] = m["idMal"]
    extra = {"ids": ids}
    if m.get("averageScore"):
        ratings = dict(x.get("ratings") or {}) if isinstance(x.get("ratings"), dict) else {}
        ratings["anilist"] = {"score": "%d%%" % m["averageScore"], "source": "AniList",
                              "url": m.get("siteUrl") or "https://anilist.co/anime/%d" % m["id"],
                              "as_of": time.strftime("%Y-%m-%d")}
        extra["ratings"] = ratings
    meta = {"extra": extra}

    which = str(settings.get("title") or "").strip().lower()
    t = (m.get("title") or {}).get(which) if which in ("romaji", "english", "native") else None
    if t:
        meta["title"] = t.strip()

    how = str(settings.get("summary") or "missing").strip().lower()
    text = clean_text(m.get("description"))
    if text and (how == "always" or (how == "missing" and not (item.get("summary") or "").strip())):
        meta["summary"] = text

    if settings.get("studio", True):
        nodes = ((m.get("studios") or {}).get("nodes") or [])
        studio = next((n["name"] for n in nodes if n.get("isAnimationStudio") and n.get("name")), None)
        if studio:
            meta["studio"] = studio

    if settings.get("genres") and m.get("genres"):
        meta["genres"] = m["genres"]
    return {"metadata": meta}


def main():
    msg = json.loads(sys.stdin.buffer.read() or b"{}")
    if msg.get("event") != "metadata.lookup":
        return
    item = msg.get("data") or {}
    settings = msg.get("settings") or {}
    if not wanted(item, settings):
        print(json.dumps({"skipped": True}))
        return
    cache = load_cache()
    try:
        m = find(item, cache)
    except (urllib.error.URLError, OSError, ValueError) as e:
        save_cache(cache)
        print(json.dumps({"error": "AniList: %s" % e}))
        return
    save_cache(cache)
    if not m:
        print(json.dumps({"found": False}))
        return
    print(json.dumps(answer(item, m, settings)))


if __name__ == "__main__":
    main()
