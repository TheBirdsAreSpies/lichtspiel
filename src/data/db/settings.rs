//! Server settings and plugin settings, with their default values and the choices offered for them.

use super::*;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PluginState {
    pub enabled: bool,
    #[serde(default)]
    pub settings: serde_json::Map<String, Value>,
    /// Where an admin put the plugin's panel: "settings", "sidebar" or "both"; empty = where the plugin suggests.
    #[serde(default)]
    pub placement: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    /// Empty = the computer's name (see `display_name`).
    pub server_name: String,
    /// "none", "vaapi", "qsv" or "nvenc".
    pub hw_accel: String,
    /// Which graphics device to use: for VAAPI the render node (empty = /dev/dri/renderD128), for NVENC the card's number (empty = 0).
    #[serde(default)]
    pub hw_device: String,
    pub ffmpeg: String,
    pub ffprobe: String,
    pub scan_interval_minutes: u64,
    /// Percent of a video that counts as watched.
    pub watched_threshold_percent: u32,
    pub hls_segment_seconds: u32,
    pub max_transcodes: usize,
    pub invite_default_days: u64,
    /// Address shown in invite links (empty = use the request's Host).
    #[serde(default)]
    pub public_url: String,
    /// The mail server for invites, "Forgot password" and plugins that send mail (empty = no e-mail).
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "smtp_port")]
    pub smtp_port: u16,
    /// "starttls", "ssl" (encrypted from the start, usually port 465) or "none".
    #[serde(default = "starttls")]
    pub smtp_security: String,
    #[serde(default)]
    pub smtp_user: String,
    #[serde(default)]
    pub smtp_password: String,
    /// "kino@example.com" or "Kino <kino@example.com>"; empty = the user name.
    #[serde(default)]
    pub smtp_from: String,
    /// "automatic", "speed", "quality" or "max".
    #[serde(default = "auto_quality")]
    pub transcoder_quality: String,
    /// Media not watched in this many weeks leaves Continue Watching.
    #[serde(default = "cw_weeks")]
    pub continue_watching_weeks: u64,
    #[serde(default = "cw_limit")]
    pub continue_watching_limit: usize,
    /// How long an HLS request waits for the transcoder to write data.
    #[serde(default = "seg_timeout")]
    pub segmented_transcoder_timeout_seconds: u64,
    /// "Scan my library periodically" — off by default.
    #[serde(default)]
    pub scheduled_scans_enabled: bool,
    /// x264 preset used for every software transcode ("veryfast" by default).
    #[serde(default = "h264_preset")]
    pub h264_preset: String,
    /// Lowest (best) x264 CRF the quality formula may reach (16 by default).
    #[serde(default = "min_crf")]
    pub h264_minimum_crf: u32,
    /// Season premieres always appear in Continue Watching.
    #[serde(default = "yes")]
    pub continue_watching_include_premieres: bool,
    /// Look up plot, cast, posters … online for new items.
    #[serde(default = "yes")]
    pub metadata_enabled: bool,
    /// Language for online details, e.g. "en" or "de".
    #[serde(default = "lang")]
    pub metadata_language: String,
    /// Optional free key from themoviedb.org; without it Wikipedia/Wikidata and TVmaze are used.
    #[serde(default)]
    pub tmdb_api_key: String,
    /// Optional free key from omdbapi.com; with it movie and show plots are IMDb's.
    #[serde(default)]
    pub omdb_api_key: String,
    /// When a video counts as watched:
    /// 0 at the threshold, 1 at the final credits marker, 2 at the first credits
    /// marker, 3 at whichever comes first of threshold and first credits marker.
    #[serde(default = "played_at")]
    pub played_at_behaviour: u32,
    /// Hardware encoders use constant quality (-qp / -q from the quality formula)
    /// instead of a bitrate. A choice (off = bitrate).
    #[serde(default)]
    pub hw_constant_quality: bool,
    /// HDR video that is converted for a browser is tone mapped to normal (SDR) colours, else it looks
    /// grey and washed out. Needs an ffmpeg with the zscale and tonemap filters.
    #[serde(default = "yes")]
    pub hdr_tone_mapping: bool,
    /// "Generate credits video markers": "never", "scheduled" or "asap" (default:
    /// as a scheduled task and when media is added).
    #[serde(default = "asap")]
    pub credits_markers_behaviour: String,
    /// "Generate intro video markers": "never", "scheduled" or "asap" (default "asap").
    #[serde(default = "asap")]
    pub intro_markers_behaviour: String,
    /// How many actor photos are saved per movie or show.
    #[serde(default = "cast_photos")]
    pub cast_photos_limit: usize,
    /// Port the server listens on when no --port / LICHTSPIEL_PORT is given
    /// (used after a restart).
    #[serde(default = "port")]
    pub port: u16,
    /// Notice new, changed and removed files in library folders and scan them
    /// ("Scan my library automatically"; on by default).
    #[serde(default = "yes")]
    pub watch_folders: bool,
    /// When a file has no "Credits" chapter, look at the picture for end credits.
    #[serde(default = "yes")]
    pub credits_from_picture: bool,
    /// Fill in IMDb ratings from IMDb's own ratings file (about 9 MB, weekly).
    #[serde(default = "yes")]
    pub imdb_ratings_file: bool,
    /// "Empty trash automatically after every scan": files gone from disk leave
    /// the library at once. Off: they stay as unavailable until "Empty trash".
    #[serde(default = "yes")]
    pub empty_trash_automatically: bool,
    /// "Allow media deletion": the owner (and people allowed to) may delete
    /// files from disk from the "…" menu.
    #[serde(default = "yes")]
    pub allow_media_deletion: bool,
    /// Converted downloads are deleted this many days after they were made (0 = kept).
    #[serde(default = "keep_days")]
    pub download_keep_days: u64,
    /// Home profiles can be picked on the sign-in screen (in the local network).
    #[serde(default = "yes")]
    pub home_profiles: bool,
    /// Reverse proxies whose X-Forwarded-For / X-Real-IP / Forwarded header is believed:
    /// addresses or ranges (192.168.1.5, 172.16.0.0/12, ::1), separated by commas or spaces.
    #[serde(default)]
    pub trusted_proxies: String,
    /// Further addresses or ranges that count as the home network (a VPN, a routed subnet), besides the private ranges.
    #[serde(default)]
    pub lan_networks: String,
    /// The network card's address to listen on (empty = all). Needs a restart.
    #[serde(default)]
    pub bind_address: String,
    /// HTTPS: the certificate (PEM file, with the chain) and its private key (PEM file). Both set =
    /// the server speaks HTTPS on its port; both empty = plain HTTP. Needs a restart.
    #[serde(default)]
    pub tls_cert: String,
    #[serde(default)]
    pub tls_key: String,
    /// With HTTPS: a second port that answers plain http:// with a redirect to https:// (0 = none, e.g. 80).
    /// Needs a restart.
    #[serde(default)]
    pub http_redirect_port: u16,
    /// Relay: address ("host:port") of a computer on the internet running `lichtspiel --relay`, and the
    /// secret shared with it. Callers from outside reach this server through it, without a port forward.
    #[serde(default)]
    pub relay_server: String,
    #[serde(default)]
    pub relay_secret: String,
    /// Answer apps that search the home network for servers (GDM).
    #[serde(default = "yes")]
    pub gdm_enabled: bool,
    /// Maintenance window (local hours): scheduled tasks only start between these.
    #[serde(default = "maint_start")]
    pub maintenance_start_hour: u32,
    #[serde(default = "maint_end")]
    pub maintenance_end_hour: u32,
    /// The browser's offset from UTC in minutes (sent when settings are saved), so hours are local.
    #[serde(default)]
    pub utc_offset_minutes: i64,
    /// A copy of the library file every 3 days, the newest few kept.
    #[serde(default = "yes")]
    pub backup_enabled: bool,
    /// Empty = the "backups" folder in the data folder.
    #[serde(default)]
    pub backup_dir: String,
    #[serde(default = "backups_keep")]
    pub backups_keep: u32,
    /// Memory for the library file's page cache, in MB.
    #[serde(default = "db_cache_mb")]
    pub db_cache_mb: u32,
    #[serde(default = "yes")]
    pub optimize_weekly: bool,
    #[serde(default = "yes")]
    pub cleanup_weekly: bool,
    /// Every three days, inside the maintenance window, all libraries are read again.
    #[serde(default = "yes")]
    pub refresh_metadata: bool,
    /// Show online trailers (from TMDB / Wikidata) on movie and show pages.
    #[serde(default = "yes")]
    pub trailers_online: bool,
    /// What of the online details is loaded (all on by default): people, pictures, seasons and episodes, and the
    /// biography and filmography on a person's page.
    #[serde(default = "yes")]
    pub meta_cast: bool,
    #[serde(default = "yes")]
    pub meta_artwork: bool,
    #[serde(default = "yes")]
    pub meta_episodes: bool,
    #[serde(default = "yes")]
    pub meta_ratings: bool,
    #[serde(default = "yes")]
    pub meta_person_details: bool,
    /// Also look for English trailers when the library's language has none.
    #[serde(default = "yes")]
    pub trailers_english: bool,
    /// Allow red band (restricted) trailers.
    #[serde(default)]
    pub trailers_red_band: bool,
    /// On the home page, new episodes are shown as their show (once per show).
    #[serde(default)]
    pub group_recent_episodes: bool,
    /// Measure how loud each video is (during maintenance) so the player can level the volume.
    #[serde(default)]
    pub analyze_loudness: bool,
    /// "Deep analysis" as a scheduled task (off by default).
    #[serde(default)]
    pub deep_analysis: bool,
    #[serde(default)]
    pub generate_thumbnails: bool,
    /// Theme music for shows cut from the intro of one episode (during maintenance, once per show).
    #[serde(default = "yes")]
    pub theme_from_intros: bool,
    /// Subtitle files next to a video are synchronized with its sound once each (during maintenance,
    /// and right after a plugin saved one).
    #[serde(default = "yes")]
    pub sync_subtitles: bool,
    /// Surround sound converted to stereo: lift the dialogue (the centre channel) so voices stay clear.
    #[serde(default = "yes")]
    pub boost_dialogue: bool,
    /// Streams to people outside the home network are held to this many Mbit/s (0 = no limit).
    #[serde(default)]
    pub remote_max_mbps: u32,
    /// Off: the server never converts video (only files the player can play as they are).
    #[serde(default = "yes")]
    pub transcoding_enabled: bool,
    /// Where live conversions are written (empty = the "transcode" folder in the data folder).
    #[serde(default)]
    pub transcode_dir: String,
    /// Where converted downloads are kept (empty = the "downloads" folder in the data folder).
    #[serde(default)]
    pub downloads_dir: String,
    /// People outside the home network may play this many different videos at once (0 = no limit).
    #[serde(default)]
    pub max_remote_plays: u32,
    /// A play that stays paused this many minutes is ended (0 = never).
    #[serde(default)]
    pub paused_stop_minutes: u32,
    /// Addresses and networks that need no sign-in: they act as the first admin. Only believed
    /// addresses count (see trusted_proxies). Leave empty unless you know why you need it.
    #[serde(default)]
    pub no_auth_networks: String,
    /// Most conversions at once while the graphics chip converts (0 = no limit).
    #[serde(default)]
    pub max_gpu_transcodes: usize,
    /// Most conversions at once while the processor converts (0 = no limit).
    #[serde(default)]
    pub max_cpu_transcodes: usize,
    /// Video files played before a movie (one is picked each time): paths separated by new lines or
    /// semicolons. Files the browser can play (MP4, WebM) work best.
    #[serde(default)]
    pub preroll_paths: String,
    /// Leave articles off when sorting titles ("The Matrix" under M).
    #[serde(default = "yes")]
    pub sort_ignore_articles: bool,
    /// The articles, separated by commas.
    #[serde(default = "articles_default")]
    pub sort_articles: String,
    /// From this many movies of one film series a collection is made (2 = every series).
    #[serde(default = "collection_min_default")]
    pub collection_min_items: u32,
    /// "random" (one at random), "all" (all of them, in the order listed), "first" (always the first).
    #[serde(default = "preroll_mode_default")]
    pub preroll_mode: String,
    /// When each scheduled task last ran (unix seconds).
    #[serde(default)]
    pub maintenance_last: std::collections::BTreeMap<String, u64>,
}

fn maint_start() -> u32 { 2 }
fn maint_end() -> u32 { 5 }
fn backups_keep() -> u32 { 5 }
fn db_cache_mb() -> u32 { 64 }

fn keep_days() -> u64 {
    7
}

fn port() -> u16 {
    crate::server::DEFAULT_PORT
}

fn smtp_port() -> u16 {
    587
}

fn starttls() -> String {
    "starttls".into()
}

fn cast_photos() -> usize {
    20
}

fn asap() -> String {
    "asap".into()
}

/// The choices for "Generate credits video markers".
pub const CREDITS_MARKER_CHOICES: &[(&str, &str)] = &[
    ("never", "never"),
    ("scheduled", "as a scheduled task"),
    ("asap", "as a scheduled task and when media is added"),
];

fn played_at() -> u32 {
    3
}

/// The "Video play completion behaviour" choices.
pub const PLAYED_AT_CHOICES: &[(u32, &str)] = &[
    (0, "at selected threshold percentage"),
    (1, "at final credits marker position"),
    (2, "at first credits marker position"),
    (3, "earliest between threshold percent and first credits marker"),
];

/// Position (ms) at which a video of `duration` ms counts as watched, given its
/// credits markers (start times, ms). Without markers the threshold is used.
pub fn watched_at_ms(behaviour: u32, threshold_percent: u32, duration: u64, credits: &[u64]) -> u64 {
    let threshold = duration * threshold_percent.clamp(1, 100) as u64 / 100;
    let first = credits.iter().copied().filter(|&c| c > 0 && c < duration).min();
    let last = credits.iter().copied().filter(|&c| c > 0 && c < duration).max();
    match (behaviour, first, last) {
        (1, _, Some(l)) => l,
        (2, Some(f), _) => f,
        (3, Some(f), _) => f.min(threshold),
        _ => threshold,
    }
}



fn auto_quality() -> String {
    "automatic".into()
}
fn h264_preset() -> String {
    "veryfast".into()
}
fn min_crf() -> u32 {
    16
}
fn lang() -> String {
    "en".into()
}

/// The "Video played threshold" choices (percent).
pub const PLAYED_THRESHOLDS: &[u32] = &[70, 75, 80, 85, 90, 95];
fn cw_weeks() -> u64 {
    16
}
fn cw_limit() -> usize {
    40
}
fn seg_timeout() -> u64 {
    20
}

/// The scan interval choices, in minutes (15 minutes … 1 day).
pub const SCAN_INTERVALS: &[(u64, &str)] = &[
    (15, "every 15 minutes"),
    (30, "every 30 minutes"),
    (60, "hourly"),
    (120, "every 2 hours"),
    (360, "every 6 hours"),
    (720, "every 12 hours"),
    (1440, "daily"),
];

fn articles_default() -> String {
    DEFAULT_ARTICLES.into()
}

fn collection_min_default() -> u32 {
    2
}

fn preroll_mode_default() -> String {
    "random".into()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            server_name: crate::server::APP_NAME.into(),
            // Use the hardware when it works.
            hw_accel: "auto".into(),
            hw_device: String::new(),
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            scan_interval_minutes: 60,
            watched_threshold_percent: 90,
            hls_segment_seconds: 6,
            // 0 = no limit.
            max_transcodes: 0,
            invite_default_days: 7,
            public_url: String::new(),
            smtp_host: String::new(),
            smtp_port: smtp_port(),
            smtp_security: starttls(),
            smtp_user: String::new(),
            smtp_password: String::new(),
            smtp_from: String::new(),
            transcoder_quality: auto_quality(),
            continue_watching_weeks: cw_weeks(),
            continue_watching_limit: cw_limit(),
            segmented_transcoder_timeout_seconds: seg_timeout(),
            scheduled_scans_enabled: false,
            h264_preset: h264_preset(),
            h264_minimum_crf: min_crf(),
            continue_watching_include_premieres: true,
            metadata_enabled: true,
            metadata_language: lang(),
            tmdb_api_key: String::new(),
            omdb_api_key: String::new(),
            // Whichever comes first: the threshold or the first credits marker.
            played_at_behaviour: played_at(),
            hw_constant_quality: false,
            hdr_tone_mapping: true,
            credits_markers_behaviour: asap(),
            intro_markers_behaviour: asap(),
            cast_photos_limit: cast_photos(),
            port: port(),
            watch_folders: true,
            credits_from_picture: true,
            imdb_ratings_file: true,
            empty_trash_automatically: true,
            allow_media_deletion: true,
            download_keep_days: keep_days(),
            home_profiles: true,
            trusted_proxies: String::new(),
            lan_networks: String::new(),
            bind_address: String::new(),
            tls_cert: String::new(),
            tls_key: String::new(),
            http_redirect_port: 0,
            relay_server: String::new(),
            relay_secret: String::new(),
            gdm_enabled: true,
            maintenance_start_hour: maint_start(),
            maintenance_end_hour: maint_end(),
            utc_offset_minutes: 0,
            backup_enabled: true,
            backup_dir: String::new(),
            backups_keep: backups_keep(),
            db_cache_mb: db_cache_mb(),
            optimize_weekly: true,
            cleanup_weekly: true,
            refresh_metadata: true,
            trailers_online: true,
            meta_cast: true,
            meta_artwork: true,
            meta_episodes: true,
            meta_ratings: true,
            meta_person_details: true,
            trailers_english: true,
            theme_from_intros: true,
            sync_subtitles: true,
            trailers_red_band: false,
            group_recent_episodes: false,
            analyze_loudness: false,
            deep_analysis: false,
            generate_thumbnails: false,
            boost_dialogue: true,
            remote_max_mbps: 0,
            preroll_paths: String::new(),
            sort_ignore_articles: true,
            sort_articles: DEFAULT_ARTICLES.into(),
            collection_min_items: 2,
            preroll_mode: preroll_mode_default(),
            max_gpu_transcodes: 0,
            max_cpu_transcodes: 0,
            max_remote_plays: 0,
            paused_stop_minutes: 0,
            no_auth_networks: String::new(),
            transcoding_enabled: true,
            transcode_dir: String::new(),
            downloads_dir: String::new(),
            maintenance_last: Default::default(),
        }
    }
}
