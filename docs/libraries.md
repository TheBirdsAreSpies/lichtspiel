# Libraries: scanning, Plex sync and import

How new files are found, how to keep in step with a running Plex server, and how to bring everything over from the old server. Back to the [README](../README.md).

## New files are found by themselves

With **Settings → Scan my library automatically** on (the default here), the server looks at
the library folders every 60 seconds. When files are added, renamed or removed it waits until
they stop changing (so a file that is still being copied isn't read half-way) and then scans
that library; only new or changed files are read. Files added while the server was off are
found right after it starts.

## Keep in step with a running Plex server

On the **Libraries** page, under **Sync with Plex**, enter the Plex server's address
(e.g. `http://192.168.1.10:32400`) and one X-Plex-Token per person (Plex Web: ⋯ on a movie →
Get info → View XML; the address ends in `X-Plex-Token=…`). Every few minutes (or with
**Sync now**) watched state, resume points and ratings of movies and episodes go both ways.
Videos are matched by their file path; when Plex sees the folders under other paths,
**Test connection** suggests a folder translation. A change on one side goes to the other;
when both sides changed, the side watched more recently wins. The first sync loses nothing:
watched on either side stays watched and a rating set on either side is kept.
Under **Direction** you can make one side the master instead: *Plex → here only* or
*Here → Plex only* copy the master's state as it is and never send anything the other way.

## Import everything from your old server

On the **Libraries** page, under **Import from your old server**, point the server at
the old server's library database (`com.plexapp.plugins.library.db`, usually in
`Plug-in Support/Databases`). It is found by itself in the usual places on Windows and in
the Docker folder `/oldserver` (on a Synology set `OLD_SERVER` to the old data folder in a `.env`
file next to `docker-compose.yml`, see there; or set `LICHTSPIEL_OLD_SERVER`).
The old database is only read, from a copy; nothing of the old server is changed.

1. Press **Look inside**: you see the old libraries with their folders and the old accounts.
2. If the files are now at another place (e.g. `D:\Movies` on the old PC, `/media/Movies`
   in Docker), type the new folder next to each old one.
3. Choose which of your users each old account becomes (same names are matched by
   themselves; the old owner is you).
4. Press **Import**. The top bar shows the progress; a report is shown when it's done.

What comes over: the **libraries** (created if missing, then scanned), **watched state and
how often, resume points and your ratings** for every chosen account, **credits markers**,
**details** (title, original title, plot, tagline, studio, age rating, release date, genres;
these are then kept as if you had edited them), **actors with their roles, directors and
writers** (with the actor photos the old server had) and **normal playlists**. Smart playlists are
listed in the report but not imported (their filters are written differently). Running the
import again is safe: playlists with the same name are updated, not doubled.

