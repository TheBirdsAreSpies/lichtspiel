// The server's settings (owner), as data: each tab's rows, drawn by ServerSettingsView.vue. The texts are
// English keys for t() (their German in web/lang/de.js, shared with the classic app); an option or a help
// with numbers in it says them in `params`.

/** GET /api/settings, with what the rows need besides the settings. */
export interface SettingsReply {
  settings: Record<string, unknown>, port_in_use: number, lan_url: string, data_dir: string,
  your_address?: string, your_address_trusted?: boolean, you_are_local?: boolean,
  scan_intervals: { minutes: number, label: string }[], played_thresholds: number[], credits_marker_choices: { value: string, label: string }[],
  played_at_choices: { value: string, label: string }[], languages: { code: string, name: string }[], h264_presets: string[],
}
/** GET /api/transcoder */
export interface Transcoder {
  ffmpeg_found: boolean, ffmpeg_version?: string, hardware_encoders: string[], hardware_working?: string[], render_device?: boolean,
  hw_in_use: string, devices?: { value: string, name: string, backend: string }[],
}

export type Opt = [string | number, string, Record<string, string | number>?];
export type Row =
  | { k: string, label: string, kind: 'text' | 'number' | 'password' | 'check' | 'area', help?: string, params?: (r: SettingsReply) => Record<string, string | number> }
  | { k: string, label: string, kind: 'select', opts: Opt[] | ((r: SettingsReply, tc: Transcoder) => Opt[]), help?: string, params?: (r: SettingsReply) => Record<string, string | number> }
  | { kind: 'head', label: string }
  | { kind: 'note', label: string }
  | { kind: 'info', what: 'lan' | 'you' | 'transcoder' }
  | { kind: 'button', what: 'mail' | 'imdb', label: string, help?: string };

const n = (list: number[], none: string, label?: string): Opt[] => [[0, none], ...list.map(x => [x, label || String(x), label ? { n: x } : undefined] as Opt)];
const hours: Opt[] = Array.from({ length: 24 }, (_, i) => [i, String(i).padStart(2, '0') + ':00']);
// Without ffmpeg these do nothing: they are switched off and say why.
export const NEEDS_FFMPEG = ['credits_from_picture', 'analyze_loudness', 'deep_analysis', 'generate_thumbnails', 'theme_from_intros', 'sync_subtitles', 'boost_dialogue', 'transcoder_quality',
  'hw_accel', 'hw_device', 'hw_constant_quality', 'hdr_tone_mapping', 'h264_preset', 'h264_minimum_crf', 'max_transcodes', 'max_gpu_transcodes', 'max_cpu_transcodes', 'hls_segment_seconds', 'segmented_transcoder_timeout_seconds'];

export const TABS: [string, string][] = [['general', 'Server'], ['network', 'Network settings'], ['library', 'Library settings'], ['metadata', 'Movie and show details'],
  ['transcoder', 'Transcoder'], ['tasks', 'Scheduled tasks'], ['extras', 'Extras'], ['maintenance', 'Maintenance']];

export const ROWS: Record<string, Row[]> = {
  general: [
    { k: 'server_name', label: 'Server name', kind: 'text' },
    { k: 'port', label: 'Port', kind: 'number', help: 'Default 8484. Takes effect after a restart; right now the server listens on {port}. With Docker, change the port mapping in docker-compose.yml instead.', params: r => ({ port: r.port_in_use }) },
    { k: 'public_url', label: 'Address used in invite links', kind: 'text', help: 'e.g. http://my-nas.example.com:8484 — leave empty to use the address you opened this page with' },
    { k: 'invite_default_days', label: 'Invite links last (days)', kind: 'number' },
    { k: 'home_profiles', label: 'Profiles on the sign-in screen', kind: 'check', help: 'Home profiles (set under Users & invites) can be picked with a tap and their PIN — only from your home network.' },
    { kind: 'head', label: 'E-mail' },
    { kind: 'note', label: 'With a mail server the server sends invites itself, people can ask for a new password when they forgot theirs, and plugins such as “New on Lichtspiel” send their mail through it.' },
    { k: 'smtp_host', label: 'Mail server (SMTP)', kind: 'text', help: 'e.g. smtp.gmail.com, smtp.office365.com or mail.gmx.net. Empty = no e-mail.' },
    { k: 'smtp_port', label: 'Port', kind: 'number', help: '587 with STARTTLS, 465 with SSL/TLS.' },
    { k: 'smtp_security', label: 'Encryption', kind: 'select', opts: [['starttls', 'STARTTLS'], ['ssl', 'SSL/TLS'], ['none', 'None']] },
    { k: 'smtp_user', label: 'User name for the mail server', kind: 'text', help: 'Usually your e-mail address. Empty = no sign-in.' },
    { k: 'smtp_password', label: 'Password for the mail server', kind: 'password', help: 'With Gmail, Outlook and two-step sign-in: an app password.' },
    { k: 'smtp_from', label: 'Sender', kind: 'text', help: 'e.g. kino@example.com or Kino <kino@example.com>. Empty = the user name.' },
    { kind: 'button', what: 'mail', label: 'Save and send a test mail' },
  ],
  network: [
    { k: 'max_remote_plays', label: 'Videos per person outside the home network', kind: 'select', opts: n([1, 2, 3, 4, 6], 'No limit'), help: 'How many different videos one person may play at once from outside. Admins are not limited.' },
    { k: 'paused_stop_minutes', label: 'End paused playback after', kind: 'select', opts: n([5, 10, 15, 30, 60, 120], 'Never', '{n} minutes'), help: 'Frees the converter when someone leaves a video paused.' },
    { kind: 'info', what: 'lan' },
    { k: 'no_auth_networks', label: 'Networks that need no sign-in', kind: 'text', help: 'Addresses or ranges separated by commas, e.g. 192.168.1.0/24. People coming from there are signed in as the first admin without a password — only add networks you fully trust. Needs a believed address (see trusted proxies).' },
    { k: 'remote_max_mbps', label: 'Limit streams over the internet', kind: 'select', opts: n([1, 2, 3, 4, 8, 10, 12, 20], 'No limit', '{n} Mbit/s'), help: 'Videos watched from outside your home network are converted to stay below this. Home network viewers are not limited.' },
    { k: 'lan_networks', label: 'More networks counted as home network', kind: 'text', help: 'Addresses or ranges separated by commas, e.g. 100.64.0.0/10 for a VPN. People there are not limited like people on the internet.' },
    { k: 'bind_address', label: 'Network card address', kind: 'text', help: 'Empty = listen on every network card. Enter one address such as 192.168.1.10 to listen only there. Takes effect after a restart.' },
    { k: 'tls_cert', label: 'HTTPS certificate file', kind: 'text', help: 'Path of the certificate file (PEM, with its chain), e.g. /config/fullchain.pem. With the key below the server answers on https:// instead of http://. Takes effect after a restart.' },
    { k: 'tls_key', label: 'HTTPS key file', kind: 'text', help: 'Path of the private key (PEM) that belongs to the certificate, e.g. /config/privkey.pem. Empty again = plain http://. Both files are checked when you save.' },
    { k: 'http_redirect_port', label: 'Pass http:// on to https:// from port', kind: 'number', help: 'With HTTPS: this extra port (e.g. 80) answers plain http:// by sending visitors to the https:// address. 0 = off. Takes effect after a restart.' },
    { k: 'relay_server', label: 'Relay (address)', kind: 'text', help: 'To be reachable from outside without a port forward: the address of a computer on the internet that runs "lichtspiel --relay", e.g. relay.example.com:8485. Empty = off. Use HTTPS here (certificate for the relay\'s name), else the traffic is not private.' },
    { k: 'relay_secret', label: 'Relay (secret)', kind: 'password', help: 'The secret you gave the relay with --relay-secret (at least 8 characters).' },
    { k: 'gdm_enabled', label: 'Let apps find this server on the home network', kind: 'check', help: 'Answers searches from apps on the same network (GDM), so they list the server without typing an address.' },
    { k: 'trusted_proxies', label: 'Trusted reverse proxies', kind: 'text', help: 'Addresses or ranges of your reverse proxy, e.g. 127.0.0.1 or 172.16.0.0/12 for Docker. Only then is the caller’s address it passes on (X-Forwarded-For) believed; requests passed on by any other machine never count as your home network.' },
    { kind: 'info', what: 'you' },
  ],
  library: [
    { k: 'group_recent_episodes', label: 'Show new episodes on the home page as their show', kind: 'check' },
    { k: 'sort_ignore_articles', label: 'Ignore articles when sorting titles', kind: 'check', help: '“The Matrix” is then sorted under M.' },
    { k: 'sort_articles', label: 'Articles to ignore', kind: 'text', help: 'Separated by commas, e.g. the, a, an, der, die, das. Sorting is updated when you save.' },
    { k: 'collection_min_items', label: 'Make a collection from … movies', kind: 'number', help: 'A film series becomes a collection once this many of its movies are in the library (2 = every series).' },
    { k: 'scheduled_scans_enabled', label: 'Scan my library periodically', kind: 'check', help: 'Off by default. Turn it on so new files show up without pressing "Scan".' },
    { k: 'scan_interval_minutes', label: 'Library scan interval', kind: 'select', opts: r => r.scan_intervals.map(x => [x.minutes, x.label] as Opt), help: 'Each scan only looks at folder listings and file dates, not inside the files; videos that did not change are not read again. Disks still wake up, so choose a long interval for a NAS that should sleep.' },
    { k: 'watched_threshold_percent', label: 'Video played threshold', kind: 'select', opts: r => r.played_thresholds.map(p => [p, '{n}%', { n: p }] as Opt), help: 'Set the progress percentage for video playback at which point the video will be marked as played.' },
    { k: 'watch_folders', label: 'Scan my library automatically', kind: 'check', help: 'Your library will be updated automatically when files are added to, changed in or removed from library folders (checked every 60 seconds; a file is added once copying has finished).' },
    { k: 'credits_markers_behaviour', label: 'Generate credits video markers', kind: 'select', opts: r => r.credits_marker_choices.map(c => [c.value, c.label] as Opt), help: 'Detects movie and episode end credits: from chapters named "Credits" in the file, otherwise by looking at the end of the video. Can also be switched off per library; the Libraries page has a "Detect credits" button to run it now.' },
    { k: 'intro_markers_behaviour', label: 'Generate intro video markers', kind: 'select', opts: r => r.credits_marker_choices.map(c => [c.value, c.label] as Opt), help: 'Detects show intros, showing the "Skip intro" button in the player: the first 10 minutes of sound of every episode in a season are compared, and the part most episodes share is the intro. Can also be switched off per TV library; the Libraries page has a "Detect intros" button to run it now. Needs ffmpeg.' },
    { k: 'credits_from_picture', label: 'Look at the picture to find credits', kind: 'check', help: 'When a file has no "Credits" chapter, the last part of the video is checked for the typical credits look (dark screen with text). Needs ffmpeg.' },
    { k: 'played_at_behaviour', label: 'Mark videos as watched', kind: 'select', opts: r => r.played_at_choices.map(c => [c.value, c.label] as Opt), help: 'Credits markers come from chapters named "Credits" / "Abspann" in the file, or are set by hand on the movie or episode page. Without markers the threshold is used.' },
    { k: 'continue_watching_weeks', label: 'Weeks to consider for Continue Watching', kind: 'number' },
    { k: 'continue_watching_limit', label: 'Maximum Continue Watching items', kind: 'number' },
    { k: 'continue_watching_include_premieres', label: 'Include season premieres in Continue Watching', kind: 'check', help: 'New season premieres always appear no matter how many weeks have passed since watching.' },
    { k: 'empty_trash_automatically', label: 'Empty trash automatically after every scan', kind: 'check', help: 'Files gone from disk leave the library right away. Off: they stay as unavailable (with their watched state and details) until you choose "Empty trash" in the library’s menu. A library folder that cannot be read never empties anything.' },
    { k: 'allow_media_deletion', label: 'Allow media deletion', kind: 'check', help: 'Admins, and people you allow under Users & invites, may delete movie and episode files from disk with "Delete" in the "⋯" menu.' },
    { k: 'disk_warn_percent', label: 'Warn when a disk is nearly full (percent free)', kind: 'number', help: 'Checked every 30 minutes for the disks of the libraries and of the data folder. Below this much free space plugins are told once (the Telegram / Pushover plugin sends a message), and the dashboard shows it red. 0 = never.' },
    { k: 'download_keep_days', label: 'Keep converted downloads (days)', kind: 'number', help: 'Copies converted for “Download…” are kept in the server’s data folder (downloads) and deleted this many days after they were made. 0 = until they are removed from the Downloads page.' },
  ],
  metadata: [
    { k: 'metadata_enabled', label: 'Download plot, cast, posters and episode titles from the internet', kind: 'check' },
    { kind: 'head', label: 'What is downloaded' },
    { k: 'meta_cast', label: 'Cast and crew', kind: 'check', help: 'Actors with their roles and photos, directors, writers and producers.' },
    { k: 'cast_photos_limit', label: 'Actor photos per movie or show', kind: 'select', opts: n([10, 20, 40, 100], 'None'), help: 'The cast list itself is kept; only this many photos are saved on the server.' },
    { k: 'meta_person_details', label: 'Details about people', kind: 'check', help: 'Biography, birth, more photos and the filmography on an actor’s page. Needs a TMDB key; loaded when the page is opened.' },
    { k: 'meta_artwork', label: 'Posters and backgrounds', kind: 'check', help: 'Pictures of movies, shows and seasons. Off: only pictures from your folders are used.' },
    { k: 'meta_episodes', label: 'Seasons and episodes', kind: 'check', help: 'Episode names, plots, air dates and stills.' },
    { k: 'meta_ratings', label: 'Ratings', kind: 'check', help: 'Scores from the online source (IMDb ratings from the ratings file are set below).' },
    { k: 'metadata_language', label: 'Language of titles, plots and episode names', kind: 'select', opts: r => r.languages.map(l => [l.code, l.name] as Opt), help: 'Used by every library that is set to "Same as server" on the Libraries page (each library can have its own). Changing it downloads the details again in the background — watch the progress at the top of the page. Show episode names are only translated with a TMDB key.' },
    { k: 'imdb_ratings_file', label: 'Show IMDb ratings from IMDb’s ratings file', kind: 'check', help: 'Downloads IMDb’s free ratings list (about 9 MB) once a week. Rotten Tomatoes and Metacritic scores come from Wikidata; the TMDB score needs a TMDB key.' },
    { kind: 'button', what: 'imdb', label: 'Update IMDb ratings now', help: 'Downloads the ratings file again right away and updates every movie and show that has an IMDb id.' },
    { k: 'tmdb_api_key', label: 'TMDB API key (optional)', kind: 'text', help: 'Without a key, movies come from Wikipedia/Wikidata and TV shows from TVmaze. A free key from themoviedb.org gives better matches, cast photos and translated plots.' },
    { k: 'omdb_api_key', label: 'OMDb API key for IMDb plots (optional)', kind: 'text', help: 'With a free key from omdbapi.com, movie and show plots are taken from IMDb (in English) instead of Wikipedia. Press Refresh details on a movie, or change the language, to load them for movies you already have.' },
  ],
  extras: [
    { k: 'trailers_online', label: 'Show online trailers', kind: 'check', help: 'Trailers and other videos from TMDB or Wikidata, played from YouTube. Nothing is downloaded.' },
    { k: 'trailers_english', label: 'Also look for English trailers', kind: 'check', help: 'When there is none in the library language. Applies to titles when their details are loaded again.' },
    { k: 'trailers_red_band', label: 'Allow red band trailers', kind: 'check', help: 'Trailers marked for adults only. Applies to titles when their details are loaded again.' },
    { k: 'preroll_mode', label: 'Pre-roll: which to play', kind: 'select', opts: [['random', 'One, picked at random'], ['all', 'All of them, in the order listed'], ['first', 'Always the first']], help: 'Used when several pre-roll videos are listed below. Pre-roll videos are not the online trailers: those appear on the movie’s page.' },
    { k: 'preroll_paths', label: 'Pre-roll videos', kind: 'area', help: 'Played before every movie. One path per line (or separated by semicolons). MP4 or WebM files that the browser plays directly work best; the video can be skipped.' },
  ],
  tasks: [
    { k: 'maintenance_start_hour', label: 'Maintenance starts at', kind: 'select', opts: hours, help: 'Scheduled tasks only start between these two times (your local time).' },
    { k: 'maintenance_end_hour', label: 'Maintenance ends at', kind: 'select', opts: hours },
    { k: 'backup_enabled', label: 'Back up the library file every 3 days', kind: 'check', help: 'Back up now and restore a backup: Maintenance.' },
    { k: 'backup_dir', label: 'Backup folder', kind: 'text', help: 'Empty = the “backups” folder in the data folder.' },
    { k: 'backups_keep', label: 'Backups to keep', kind: 'number' },
    { k: 'db_cache_mb', label: 'Database cache (MB)', kind: 'number', help: 'Memory the library file may keep ready. More makes big libraries faster. Applies right away.' },
    { k: 'optimize_weekly', label: 'Optimize the library file every week', kind: 'check' },
    { k: 'cleanup_weekly', label: 'Remove old cache files and leftover conversions every week', kind: 'check' },
    { k: 'analyze_loudness', label: 'Measure the loudness of videos', kind: 'check', help: 'Looks at 5 videos per minute inside the maintenance window until all are measured. Needs ffmpeg.' },
    { k: 'deep_analysis', label: 'Run deep analysis', kind: 'check', help: 'The bitrate of a file is only its average. Deep analysis reads the whole file to find out how fast a connection really has to be for it to play without stopping, also in its busiest scenes; with a speed limit, this decides whether a video is sent as it is or converted. One video at a time inside the maintenance window, and it reads every file completely, so disks stay busy for a long time. Needs ffprobe.' },
    { k: 'theme_from_intros', label: 'Theme music from show intros', kind: 'check', help: 'Cuts the intro of one episode per show once, 3 shows per minute inside the maintenance window, and plays it as the show’s theme music. Needs found intros; a theme.mp3 in the show’s folder comes first. Needs ffmpeg.' },
    { k: 'sync_subtitles', label: 'Synchronize subtitle files with the sound', kind: 'check', help: 'Listens to the sound of a video once and moves the lines of each subtitle file next to it onto the speech, also when the file was made for another frame rate. Only moves a file when it is sure, never one whose timing was set by hand. One video per minute inside the maintenance window; files a plugin downloads are done right away. Needs ffmpeg.' },
    { k: 'generate_thumbnails', label: 'Create preview pictures', kind: 'check', help: 'Makes the pictures on the seek bar and the chapter pictures ahead of time, 2 videos per minute inside the maintenance window. Needs ffmpeg.' },
    { k: 'refresh_metadata', label: 'Read all libraries again every 3 days', kind: 'check' },
  ],
  transcoder: [
    { kind: 'info', what: 'transcoder' },
    { k: 'transcoder_quality', label: 'Transcoder quality', kind: 'select', opts: [['automatic', 'Automatic'], ['speed', 'Prefer higher speed encoding'], ['quality', 'Prefer higher quality encoding'], ['max', 'Make my CPU hurt']], help: 'Applies to Intel Quick Sync and NVIDIA hardware encoding.' },
    { k: 'boost_dialogue', label: 'Lift dialogue when mixing surround sound down to stereo', kind: 'check', help: 'Voices stay clear when 5.1 or 7.1 sound is converted for browsers and phones, also in optimized versions and downloads made from now on. Takes effect with the next video you start.' },
    { k: 'transcoding_enabled', label: 'Allow video conversion', kind: 'check', help: 'Off: the server never converts video; only files the player can play as they are will play.' },
    { k: 'transcode_dir', label: 'Folder for live conversions', kind: 'text', help: 'Empty = the “transcode” folder in the data folder. Pick a fast disk.' },
    { k: 'downloads_dir', label: 'Folder for converted downloads', kind: 'text', help: 'Empty = the “downloads” folder in the data folder. Copies already made stay where they are.' },
    { k: 'hw_accel', label: 'Hardware acceleration', kind: 'select', opts: [['auto', 'Automatic (use the graphics chip when it works)'], ['none', 'Off (software)'], ['vaapi', 'Intel/AMD VAAPI (most Synology Plus models)'], ['qsv', 'Intel Quick Sync'], ['nvenc', 'NVIDIA']] },
    // The devices found on this computer to pick from, or a field to type one into.
    { k: 'hw_device', label: 'Graphics device', kind: 'select', opts: (r, tc) => [['', 'Automatic (the first one)'], ...(tc.devices || []).map(d => [d.value, (d.backend === 'nvenc' ? 'NVIDIA ' : 'VAAPI ') + d.name] as Opt),
      ...(r.settings.hw_device && !(tc.devices || []).some(d => d.value === r.settings.hw_device) ? [[String(r.settings.hw_device), String(r.settings.hw_device)] as Opt] : [])], help: 'Found on this computer. Pick one if you have more than one graphics chip.' },
    { k: 'hdr_tone_mapping', label: 'Convert HDR video to normal colours', kind: 'check', help: 'When HDR video has to be converted for a browser it is tone mapped to normal (SDR) colours, else it looks grey and washed out. Costs processor time. Needs an ffmpeg with the zscale and tonemap filters.' },
    { k: 'hw_constant_quality', label: 'Graphics chip: convert by quality instead of bitrate', kind: 'check', help: 'Off: the graphics chip gets a fixed bitrate (safe on every model). On: it gets a quality number from 15 (best) to 25, like the software encoder does. Try it if hardware-converted video looks blocky.' },
    { k: 'h264_preset', label: 'Software encoder speed (x264 preset)', kind: 'select', opts: r => r.h264_presets.map(p => [p, p] as Opt) },
    { k: 'h264_minimum_crf', label: 'Best allowed x264 quality (minimum CRF)', kind: 'number', help: 'Lower = better picture and bigger stream. The quality the player asks for is mapped between this and 24.' },
    { k: 'max_transcodes', label: 'Maximum simultaneous video transcodes (0 = no limit)', kind: 'number' },
    { k: 'max_gpu_transcodes', label: 'Maximum simultaneous video transcodes with the graphics chip (0 = no limit)', kind: 'number' },
    { k: 'max_cpu_transcodes', label: 'Maximum simultaneous video transcodes with the processor (0 = no limit)', kind: 'number' },
    { k: 'hls_segment_seconds', label: 'HLS segment length (seconds)', kind: 'number' },
    { k: 'segmented_transcoder_timeout_seconds', label: 'Segmented transcoder timeout (seconds)', kind: 'number' },
    { k: 'ffmpeg', label: 'ffmpeg program', kind: 'text' },
    { k: 'ffprobe', label: 'ffprobe program', kind: 'text' },
  ],
};
