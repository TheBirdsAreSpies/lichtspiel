# Watching

When a video counts as watched, credits and intro detection, theme music and deep analysis. Back to the [README](../README.md).

## When something counts as watched

A video counts as watched at **whichever comes first: 90 % or the start of
the end credits**. The credits are found in two ways:

1. a **chapter** in the file called "Credits", "End Credits", "Abspann", "Générique" …;
2. otherwise the server **looks at the picture** of the last part of the video (the last 30 %,
   1 to 15 minutes): end credits are a dark screen with a little bright text, for at least
   20 seconds. Only key frames are read, so a file takes seconds, not minutes. (Credits shown over moving
   pictures can be missed.)

Credits detection runs by itself: with **Settings → Generate credits video markers** on "as a
scheduled task and when media is added" (the default) new files are checked within a minute;
on "as a scheduled task" once a day. To run it now, press **Detect credits** next to a library
on the **Libraries** page, or **Credits… → Detect credits again** on a movie or episode (API:
`POST /library/sections/{id}/credits`, `POST /library/metadata/{id}/credits`). A file that fails more than 3 times is not retried automatically, and libraries with
"Enable credits detection" off are skipped. You can always type the time yourself under
**Credits…** (e.g. `1:52:30`). Without a credits time, 90 % is used. **Settings → Mark videos
as watched** offers other choices.

## Theme music

A movie's or show's page plays its theme music quietly, once, and fades it out when you leave.
The music comes from a `theme.mp3` in its folder (also `theme-1.mp3`, or .m4a, .flac, .ogg, .wav,
.aac) next to the video, or for a show also one folder above a season folder. A show without such a
file gets its music from its own episodes: inside the maintenance window the server cuts the intro of
one episode (the first with a found intro of 10 s to 3 min, specials last) and keeps it as
`data/themes/{show}.m4a`. It is cut again only when that intro changes. Switch this off under
**Settings → Scheduled tasks → Theme music from show intros**. Turn the playing off under
**Settings → Web → Play theme music**. Browsers may need one click on the page first.

## Skip intro (TV shows)

The server finds a show's intro by **listening**: it reads the sound of the first 10 minutes
of every episode in a season and compares the episodes with each other. The stretch of sound
(longer than 20 seconds) that most episodes share is the intro, even when it starts at a
different time in each episode because of a cold open. While it plays, the player shows
**Skip intro**, which jumps to its end; the seek bar marks it in blue.

It needs at least two episodes of a season, and runs by itself like credits detection
(**Settings → Generate intro video markers**, default "as a scheduled task and when media is
added"). To run it now, press **Detect intros** next to a TV library on the **Libraries** page
(API: `POST /library/sections/{id}/intros`, `POST /library/metadata/{id}/intros` for one show,
season or episode). Each TV library has **Enable intro detection** to switch it off. When fewer
than half of a season's episodes share an intro, the season gets none. An
intro can also be set by hand with `PUT /library/metadata/{id}/markers` and
`{"intro": [start ms, end ms]}`.

## Subtitle timing

A subtitle file made for another release is often a few seconds off, or drifts because it was made
for 25 instead of 23.976 frames per second. The server fixes that by **listening**: it finds the
stretches where someone speaks and moves (and if needed stretches) the subtitle's lines onto them.
It only applies a result it is sure of; the file itself is never changed, the server sends the
lines moved, to the web app and the app alike.

- **By itself:** every subtitle file next to a video (or in `<data>/subtitles/`) is listened to
  once, one video per minute inside the maintenance window, and a file the **Subtitles** plugin
  downloads right away. A file less than 0.2 s off is left alone, and so is one whose timing was
  set before. Switch it off under **Settings → Scheduled tasks → Synchronize subtitle files with
  the sound**. The log says which files were moved and by how much.
- **By hand (admins):** in the player's subtitle menu, *Synchronize with the sound*, *Earlier /
  Later by 0.5 s* and *Reset timing* (API: `POST /library/parts/{id}/subtitles/{n}/sync`,
  `PUT …/timing`).

A timing belongs to its file: when another subtitle file is added or one taken away, it stays with
the file it was set for.

## Deep analysis

The bitrate of a file is only its average: a busy scene can need far more for a while, and a
player with only a little video stored ahead of it then stops to wait. **Deep analysis**
(*Ausführliche Analyse*) reads every packet of the file, without decoding anything, and plays them
back on paper. For eight connection speeds, from 5 to 500 Mbit/s, it finds the lowest speed at which
the player's store never runs empty. The player's store holds 7.2 seconds of the connection, and
playback starts after 5 seconds of filling it.

It only matters when a stream has a speed limit (a quality chosen in the player, or the limit for
people outside the home network). A video is sent as it is when the measured speed plus 5 % fits the
limit. A video that was not analyzed is taken to need twice its bitrate, so it is converted sooner.
On the home network without a limit it changes nothing.

Run it with **Deep analysis** in the menu of a video or a library, or turn on **Settings → Scheduled
tasks → Run deep analysis** (off by default): one video at a time inside the
maintenance window, the latest changed first, and again when a file changes. It reads every file
completely, so a large library keeps the disks busy for a long time. **Media info** shows the result
under *Connection speed needed*. Plain **Analyze** only reads the file's header, which takes
seconds.

