# Movie and show details

Where titles, plots, pictures and ratings come from, and how to name files so they are found. Back to the [README](../README.md).

New movies and TV shows get their details from the internet automatically: **plot, cast
(with roles and photos), directors, writers, genres, rating, age rating, studio, poster and
background picture**, plus **season posters and every episode's title, plot, picture and air
date**. The movie or show page shows all of it, with links to IMDb, TMDB, TVmaze or
Wikipedia.

- Without any setup, movies come from **Wikidata + Wikipedia** (the plot is the article's
  plot section) and TV shows from **TVmaze**.
- For better matches, cast photos for movies and plots in your language, get a free API
  key at themoviedb.org (Settings → API) and paste it under **Settings → Movie and show
  details → TMDB API key**.
- **Plots from IMDb:** get a free key at omdbapi.com and paste it under **Settings → Movie
  and show details → OMDb API key**. From then on movie and show plots are IMDb's (in
  English); your movies get them in the background right after you save the key. (IMDb has
  no free service of its own and doesn't allow reading its pages, so OMDb is the way to get
  its plots.) Without the key the plot comes from Wikipedia or TMDB as before.
- **Language:** choose the language of titles, plots and episode names under **Settings →
  Movie and show details**; each library can have its own on the **Libraries** page ("Same as
  server" by default). Changing it downloads the details again in the background. Show
  plots come from Wikipedia in your language; episode names are only translated with a TMDB
  key.
- **Progress:** while details are being downloaded (or credits detected, or ratings
  updated) the top bar shows what is happening and how much is left, and open pages update
  themselves when new details arrive.
- **Ratings:** the page shows scores from **IMDb**, **Rotten Tomatoes** (Tomatometer),
  **Metacritic** and **TMDB**, each with the site's mark (drawn by the web app; Rotten
  Tomatoes shows the tomato from 60 % and the splat below, Metacritic's square takes the
  colour of the score). Rotten Tomatoes and Metacritic come from
  Wikidata; IMDb from IMDb's free ratings file (about 9 MB, downloaded once a week; can be
  switched off in Settings); TMDB needs a TMDB key. Your own rating is given with **five
  stars, half stars included**: click the left half of a star for a half star (each half star
  is 1 point on the 0–10 scale used by the API, so 3½ stars = 7).
- If something was matched wrongly, or not at all, open it and press **Fix match…**: search
  by title and year and click the right entry. **Refresh details** downloads them again.
- All pictures (posters, backgrounds, actor photos, episode pictures) are saved in the data
  folder under `metadata`, so pages load them from your own server. When no background is
  found online, a still from the video is used.
- **Actors are shared:** each actor's photo is saved once (`metadata/people`) and used by every
  movie and show they appear in. Under each actor you see how many items of the same library
  they are in; click them to see everything of theirs in your libraries (API:
  `/library/people/{key}/media`).
- **Only real photos are used as posters.** Some Wikipedia articles show the film's logo (an
  SVG drawing) instead of a poster; such pictures, title cards and landscape images are
  skipped. Broken pictures saved by an older version are removed when the server starts and
  downloaded again.
- Details you change yourself with **Edit details** are never overwritten.
- For best results name files like `Movies/The Matrix (1999)/The Matrix (1999).mkv` and
  `TV Shows/Twin Peaks/Season 1/Twin Peaks - S01E02.mkv`.

## Naming files (the Plex way)

Movies are read the way Plex's naming guide describes:

| What | Example |
|---|---|
| Movie in its own folder, or on its own | `Avatar (2009)/Avatar (2009).mkv`, `Avatar (2009).mkv` |
| Exact match by id (folder or file name; `{tvdb-…}` for shows, in the show's folder) | `Batman Begins (2005) {imdb-tt0372784}`, `{tmdb-272}` |
| Edition | `Blade Runner (1982) {edition-Final Cut}.mkv` |
| Split movie (up to 8 parts, played one after the other) | `The Dark Knight (2008) - pt1.mp4`, `- pt2.mp4` (also cd, disc, disk, dvd, part) |
| Extras in a subfolder of the movie's folder | `Avatar (2009)/Trailers/Teaser.mkv` — also `Behind The Scenes`, `Deleted Scenes`, `Featurettes`, `Interviews`, `Scenes`, `Shorts`, `Other`, `Extras` |
| Extras next to the movie | `Avatar (2009)-trailer.mkv` — also `-behindthescenes`, `-deleted`, `-featurette`, `-interview`, `-scene`, `-short`, `-other` |
| A trailer file on its own | `Avatar (2009)/trailer.mkv` or `movie-trailer.mkv` |
| Subtitles next to the video, with their language (`de`, `ger`, `German`) and `forced` or `sdh` / `cc` | `Avatar (2009).de.srt`, `Avatar (2009).en.forced.srt`, `Avatar (2009).en.sdh.srt` |

**Trailers:** the **▶ Trailer** button on a movie or show page plays a trailer file when there
is one. Otherwise it plays the first online trailer found while matching (from TMDB, or from
Wikidata when no TMDB key is set). Online trailers play in the page from YouTube, nothing is
downloaded, and they also appear in the **Extras** row marked "online". Titles matched before
this version get their online trailers after **Refresh metadata**.

Extras are shown on the movie's page, not as movies of their own; an extras folder without a
movie next to it is read as ordinary movies. When a file name finds nothing online (a release
name like `funxd.mkv`), the name of the movie's own folder is tried. Extras in TV libraries are
left out.

**Leaving files out:** a `.lichtspielignore` file in any library folder lists what the scan
skips in that folder and below, one pattern per line (`#` starts a comment):

```
Samples/
*.sample.mkv
Extras/2019/*
```

A pattern without `/` matches a file or folder name at any depth; one with `/` matches the
path from the folder the file is in. `*` stands for any characters except `/`, `**` for any
characters, `?` for one. Upper and lower case count the same.

