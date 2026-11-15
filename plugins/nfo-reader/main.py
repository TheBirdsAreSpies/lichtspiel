"""NFO reader plugin.

The server starts this program for every new library item and sends one
JSON object on standard input:
  {"event": "metadata.lookup", "data": {...item...}, "settings": {...}, "server": {...}}
It answers with {"metadata": {...}} on standard output (or nothing).
"""
import json
import os
import sys
import xml.etree.ElementTree as ET


def candidates(item):
    f = (item.get("file") or {}).get("path")
    if not f:
        return []
    folder = os.path.dirname(f)
    stem = os.path.splitext(os.path.basename(f))[0]
    names = [stem + ".nfo", "movie.nfo"]
    out = [os.path.join(folder, n) for n in names]
    if item.get("kind") == "episode":
        # tvshow.nfo lives in the show folder: this one (no season folders) or the one above
        out.append(os.path.join(folder, "tvshow.nfo"))
        out.append(os.path.join(os.path.dirname(folder), "tvshow.nfo"))
    return out


def read_nfo(path):
    try:
        with open(path, "rb") as f:  # bytes, so the file's own encoding declaration still counts
            s = f.read()
        try:
            root = ET.fromstring(s)
        except ET.ParseError:
            # Kodi allows a TMDB/IMDb link after the XML, so cut at the last closing tag.
            root = ET.fromstring(s[:s.rfind(b">") + 1])
    except Exception:
        return None
    text = lambda tag: (root.findtext(tag) or "").strip()
    meta = {}
    if text("title"):
        meta["title"] = text("title")
    if text("plot") or text("outline"):
        meta["summary"] = text("plot") or text("outline")
    if text("tagline"):
        meta["tagline"] = text("tagline")
    year = text("year") or text("premiered")[:4]
    if year.isdigit():
        meta["year"] = int(year)
    genres = [g.text.strip() for g in root.findall("genre") if g.text and g.text.strip()]
    if genres:
        meta["genres"] = genres
    rating = text("rating") or (root.findtext("ratings/rating/value") or "").strip()
    try:
        if rating:
            meta["rating"] = round(float(rating), 1)
    except ValueError:
        pass
    if text("mpaa"):
        meta["content_rating"] = text("mpaa").replace("Rated ", "")
    if text("studio"):
        meta["studio"] = text("studio")
    return meta


def main():
    msg = json.load(sys.stdin)
    if msg.get("event") != "metadata.lookup":
        return
    item = msg.get("data") or {}
    settings = msg.get("settings") or {}
    for path in candidates(item):
        if os.path.exists(path):
            meta = read_nfo(path)
            if meta:
                is_show_file = path.endswith("tvshow.nfo")
                if is_show_file or not settings.get("overwrite_title"):
                    meta.pop("title", None)
                if is_show_file:
                    # Only borrow genres and rating from the show.
                    meta = {k: v for k, v in meta.items() if k in ("genres", "content_rating", "studio")}
                print(json.dumps({"metadata": meta}))
                return


if __name__ == "__main__":
    main()
