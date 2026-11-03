//! Playback decisions (direct play, remux, transcode) and the ffmpeg
//! command lines behind them. The browser profile: HLS in MPEG-TS with H.264 + AAC/MP3, progressive
//! MP4 with H.264 + AAC, at most 8-bit video and 6 audio channels for
//! direct play, and multi-channel audio mixed down to stereo when
//! transcoding.

use crate::db::{Item, MediaFile, Settings};
use serde::Serialize;
use serde_json::{json, Value};

/// Quality choices offered to players: (name, max height, video kbit/s).
/// 0 = keep the original.
pub const QUALITIES: &[(&str, u32, u32)] = &[
    ("original", 0, 0),
    ("1080p-20", 1080, 20000),
    ("1080p-12", 1080, 12000),
    ("1080p-8", 1080, 8000),
    ("720p-4", 720, 4000),
    ("720p-3", 720, 3000),
    ("720p-2", 720, 2000),
    ("480p-1.5", 480, 1500),
    ("320p-0.7", 320, 720),
];

/// Containers, video and audio codecs a web browser plays without help.
pub const BROWSER_CONTAINERS: &[&str] = &["mp4", "m4v", "mov", "webm"];
pub const BROWSER_VIDEO: &[&str] = &["h264", "vp8", "vp9", "av1"];
pub const BROWSER_AUDIO: &[&str] = &["aac", "mp3", "opus", "vorbis", "flac"];
pub const MAX_DIRECT_BIT_DEPTH: u32 = 8;
pub const MAX_DIRECT_AUDIO_CHANNELS: u32 = 6;
pub const TRANSCODE_AUDIO_CHANNELS: u32 = 2;
pub const TRANSCODE_AUDIO_KBPS: u32 = 160;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// The file is sent as it is.
    Direct,
    /// Picture copied, only the container (and audio) changed.
    Remux,
    /// Picture re-encoded.
    Transcode,
}

#[derive(Serialize, Clone, Debug)]
pub struct Decision {
    pub method: Method,
    pub reasons: Vec<String>,
    pub height: u32,
    pub bitrate_kbps: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub max_height: u32,
    pub max_kbps: u32,
    pub force_transcode: bool,
    pub audio_stream: Option<u32>,
    /// Client says which codecs it can play beyond the browser defaults.
    pub extra_video: Vec<String>,
    pub extra_containers: Vec<String>,
    /// The player's videoQuality, 0 … 99 (None = best, 99).
    pub video_quality: Option<u32>,
    /// H.264 level the player asks for, ×10 (41 = 4.1); None = let the encoder pick.
    pub h264_level: Option<u32>,
    /// Burn this picture-based subtitle (its number among the file's subtitle streams) into the video.
    pub burn_subtitle: Option<u32>,
}

/// H.264 level limits (level×10, max frame size in macroblocks, max bitrate kbit/s, max buffer kbit).
pub const H264_LEVELS: &[(u32, u32, u32, u32)] = &[
    (10, 396, 64, 175), (11, 900, 192, 500), (12, 2376, 384, 1000), (13, 2376, 768, 2000),
    (20, 2376, 2000, 2000), (21, 4752, 4000, 4000), (22, 8100, 4000, 4000), (30, 8100, 10000, 10000),
    (31, 18000, 14000, 14000), (32, 20480, 20000, 20000), (40, 32768, 20000, 25000), (41, 32768, 50000, 62500),
    (42, 34816, 50000, 62500), (50, 110400, 135000, 135000), (51, 184320, 240000, 240000), (52, 184320, 240000, 240000),
];

/// -bufsize is 2 × bitrate, but for an H.264 level from the table
/// at most 1.25 × that level's buffer (integer maths: max_cpb*5/4).
pub fn bufsize_kbps(kb: u32, level: Option<u32>) -> u32 {
    let two = kb * 2;
    match level.and_then(|l| H264_LEVELS.iter().find(|x| x.0 == l)) {
        Some(&(_, _, _, cpb)) => two.min(cpb * 5 / 4),
        None => two,
    }
}

/// "level=41" or "level=4.1" → 41.
pub fn parse_level(s: &str) -> Option<u32> {
    let s = s.trim();
    let v = if let Some((a, b)) = s.split_once('.') { a.parse::<u32>().ok()? * 10 + b.chars().next()?.to_digit(10)? } else { s.parse().ok()? };
    (9..=62).contains(&v).then_some(v)
}

pub fn quality(name: &str) -> (u32, u32) {
    QUALITIES.iter().find(|q| q.0 == name).map(|q| (q.1, q.2)).unwrap_or((0, 0))
}

pub fn decide(file: &MediaFile, req: &Request) -> Decision {
    let mut reasons = vec![];
    let container_ok = BROWSER_CONTAINERS.contains(&file.container.as_str())
        || req.extra_containers.iter().any(|c| c == &file.container);
    if !container_ok {
        reasons.push(format!("container {} does not play in the browser", file.container));
    }
    let v = file.video();
    let a = file.audio();
    let vcodec = v.map(|v| v.codec.as_str()).unwrap_or("");
    let video_ok = !file.probed
        || v.is_none()
        || BROWSER_VIDEO.contains(&vcodec)
        || req.extra_video.iter().any(|c| c == vcodec);
    if !video_ok {
        reasons.push(format!("video codec {vcodec} needs converting"));
    }
    let depth_ok = v.is_none_or(|v| v.bit_depth <= MAX_DIRECT_BIT_DEPTH);
    if !depth_ok {
        reasons.push(format!("{}-bit video needs converting to 8-bit", v.map(|v| v.bit_depth).unwrap_or(0)));
    }
    let audio_ok = a.is_none_or(|a| BROWSER_AUDIO.contains(&a.codec.as_str()) && a.channels <= MAX_DIRECT_AUDIO_CHANNELS);
    if !audio_ok {
        if let Some(a) = a {
            reasons.push(format!("audio {} with {} channels needs converting", a.codec, a.channels));
        }
    }
    let src_h = v.map(|v| v.height).unwrap_or(0);
    let src_kbps = (file.bitrate / 1000) as u32;
    let too_big = (req.max_height > 0 && src_h > req.max_height)
        || (req.max_kbps > 0 && src_kbps > req.max_kbps + TRANSCODE_AUDIO_KBPS);
    if too_big {
        reasons.push("lower quality was asked for".into());
    }
    if req.force_transcode {
        reasons.push("transcoding was asked for".into());
    }
    let audio_track_switch = req.audio_stream.is_some();
    let method = if req.force_transcode || too_big || !video_ok || !depth_ok {
        Method::Transcode
    } else if !container_ok || !audio_ok || audio_track_switch {
        if vcodec == "h264" || vcodec.is_empty() || !file.probed {
            Method::Remux
        } else {
            // vp9 / av1 inside mkv etc.: re-encode so the mp4 stream always works.
            Method::Transcode
        }
    } else {
        Method::Direct
    };
    let height = if req.max_height > 0 { req.max_height.min(src_h.max(1)) } else { src_h };
    let height = if req.max_height > 0 && src_h == 0 { req.max_height } else { height };
    let bitrate_kbps = if req.max_kbps > 0 { req.max_kbps } else { default_kbps(height) };
    Decision { method, reasons, height, bitrate_kbps }
}

fn default_kbps(height: u32) -> u32 {
    match height {
        0 => 8000,
        h if h > 1080 => 20000,
        h if h > 720 => 10000,
        h if h > 480 => 4000,
        _ => 1500,
    }
}

/// Playback decision codes and their messages (table built in
///). 1xxx = fine, 2xxx = cannot play, 3xxx = direct
/// play refused, 4xxx = conversion problems.
pub const DECISIONS: &[(u32, &str)] = &[
    (1000, "Direct play OK."),
    (1001, "Direct play not available; Conversion OK."),
    (2000, "Neither direct play nor conversion is available."),
    (2001, "Not enough bandwidth for any playback of this item."),
    (2002, "Number of allowed streams has been reached. Stop a playback or ask admin for more permissions."),
    (2003, "File is unplayable."),
    (2004, "Streaming Session doesn't exist or timed out."),
    (2005, "Client stopped playback."),
    (2006, "Admin Terminated Playback."),
    (2007, "You have too many active streams."),
    (2008, "Playback has been paused for too long."),
    (2011, "Downloads not allowed"),
    (2012, "File is missing"),
    (3000, "This app does not allow this item to be direct played. Check your settings and try again."),
    (3001, "Not enough bandwidth for direct play of this item."),
    (4005, "This app does not allow item to be converted for playback. Check your settings and try again."),
    (4006, "Conversion failed. The transcoder exited due to an error."),
    (4007, "Conversion encoder or decoder not found."),
    (4008, "Conversion failed. The transcoder process crashed."),
    (4009, "Conversion failed. The transcoder failed to start up."),
    (4010, "The server has reached its configured limit for maximum simultaneous video transcodes."),
];

pub fn decision_text(code: u32) -> &'static str {
    DECISIONS.iter().find(|d| d.0 == code).map(|d| d.1).unwrap_or("")
}

/// Code for a playback method: 1000 direct, 1001 converted (remux or transcode).
pub fn decision_code(m: &Method) -> u32 {
    if *m == Method::Direct { 1000 } else { 1001 }
}

/// The x264 presets that can be picked in the settings.
pub const X264_PRESETS: &[&str] = &["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];

/// Quality → number mapping:
/// x = (hi − lo) × quality / 99, rounded half up; result = hi − x.
/// `quality` is the player's videoQuality, 0 … 99 (99 = best).
pub fn quality_scale(quality: u32, lo: u32, hi: u32) -> u32 {
    let x = (hi as f64 - lo as f64) * (quality.min(99) as f64 / 99.0);
    let r = (x + 0.5).floor().max(0.0) as u32;
    hi.saturating_sub(r)
}

/// x264 CRF for a quality: between the minimum CRF (16) and 24.
pub fn x264_crf(settings: &Settings, quality: u32) -> u32 {
    let lo = settings.h264_minimum_crf.min(24);
    quality_scale(quality, lo, 24)
}

/// "auto": use VAAPI when the
/// render device exists and a one-frame test encode works, else software.
/// Tested once per ffmpeg path.
pub fn effective_hw(settings: &Settings) -> String {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, String>>> = OnceLock::new();
    if settings.hw_accel != "auto" {
        return settings.hw_accel.clone();
    }
    let cache = CACHE.get_or_init(|| Mutex::new(Default::default()));
    if let Some(v) = cache.lock().unwrap().get(&settings.ffmpeg) {
        return v.clone();
    }
    let dev = "/dev/dri/renderD128";
    let works = std::path::Path::new(dev).exists()
        && std::process::Command::new(&settings.ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-vaapi_device", dev, "-f", "lavfi", "-i", "testsrc=size=320x240:rate=1",
                "-frames:v", "1", "-vf", "format=nv12,hwupload", "-c:v", "h264_vaapi", "-f", "null", "-"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    let pick = if works { "vaapi" } else { "none" }.to_string();
    cache.lock().unwrap().insert(settings.ffmpeg.clone(), pick.clone());
    pick
}

/// Hardware constant-quality number: calls(q, 15, 25),
/// so 99 → 15 (best) … 0 → 25.
pub fn hw_qp(quality: u32) -> u32 {
    quality_scale(quality, 15, 25)
}

/// Input options and the picture part of an ffmpeg command line.
/// Follows the encoder setup:
/// libx264 gets `-crf` from the quality formula plus `-maxrate` = bitrate and
/// `-bufsize` = 2 × bitrate and `-preset` from the x264 preset setting; "Transcoder
/// quality" only picks presets for Quick Sync (veryfast / slower / veryslow) and
/// NVENC (hp / hq); keyframes are forced at every segment boundary.
fn video_args(settings: &Settings, d: &Decision, method: &Method, quality: u32, level: Option<u32>) -> (Vec<String>, Vec<String>) {
    let s = |x: &str| x.to_string();
    let mut pre = vec![];
    let mut out = vec![];
    if *method == Method::Remux {
        out.extend([s("-c:v"), s("copy")]);
        return (pre, out);
    }
    let kb = d.bitrate_kbps.max(300);
    let scale_h = if d.height > 0 { d.height } else { 0 };
    let tq = settings.transcoder_quality.as_str();
    // Hardware encoders: a bitrate (-b:v), or with constant quality on, a
    // second formula: VAAPI gets -qp, others -q, = hw_qp(quality).
    let rate = |flag: &str| -> Vec<String> {
        if settings.hw_constant_quality { vec![s(flag), hw_qp(quality).to_string()] } else { vec![s("-b:v"), format!("{kb}k")] }
    };
    match effective_hw(settings).as_str() {
        "vaapi" => {
            pre.extend([s("-vaapi_device"), s("/dev/dri/renderD128")]);
            let scale = if scale_h > 0 { format!(",scale_vaapi=w=-2:h={scale_h}") } else { String::new() };
            out.extend([s("-vf"), format!("format=nv12,hwupload{scale}"), s("-c:v"), s("h264_vaapi")]);
            out.extend(rate("-qp"));
        }
        "qsv" => {
            let scale = if scale_h > 0 { format!("scale=-2:{scale_h},") } else { String::new() };
            out.extend([s("-vf"), format!("{scale}format=nv12"), s("-c:v"), s("h264_qsv")]);
            out.extend(rate("-q:v"));
            match tq {
                "speed" => out.extend([s("-preset"), s("veryfast")]),
                "quality" => out.extend([s("-preset"), s("slower")]),
                "max" => out.extend([s("-preset"), s("veryslow")]),
                _ => {}
            }
            out.extend([s("-forced_idr"), s("1")]);
        }
        "nvenc" => {
            let scale = if scale_h > 0 { format!("scale=-2:{scale_h},") } else { String::new() };
            out.extend([s("-vf"), format!("{scale}format=yuv420p"), s("-c:v"), s("h264_nvenc")]);
            out.extend(rate("-q:v"));
            match tq {
                "quality" | "max" => out.extend([s("-preset"), s("hq")]),
                "speed" => out.extend([s("-preset"), s("hp")]),
                _ => {}
            }
            out.extend([s("-forced-idr"), s("1")]);
        }
        _ => {
            let scale = if scale_h > 0 { format!("scale=-2:{scale_h},") } else { String::new() };
            out.extend([
                s("-vf"), format!("{scale}format=yuv420p"),
                s("-c:v"), s("libx264"), s("-crf"), x264_crf(settings, quality).to_string(),
                s("-preset"), settings.h264_preset.clone(), s("-profile:v"), s("high"),
            ]);
            // The asked-for level goes to libx264 when it is below 5.3.
            if let Some(l) = level.filter(|l| *l < 53) {
                out.extend([s("-level"), format!("{}.{}", l / 10, l % 10)]);
            }
        }
    }
    let seg = settings.hls_segment_seconds.max(2);
    out.extend([
        s("-maxrate"), format!("{kb}k"),
        s("-bufsize"), format!("{}k", bufsize_kbps(kb, level)),
        s("-force_key_frames"), format!("expr:gte(t,n_forced*{seg})"),
    ]);
    (pre, out)
}

/// Subtitles made of pictures (Blu-ray PGS, DVD, DVB): the browser can't show them as text.
pub fn is_picture_subtitle(codec: &str) -> bool {
    let c = codec.to_lowercase();
    c.contains("pgs") || c.contains("dvd_sub") || c.contains("dvb_sub") || c.contains("xsub") || c.contains("vobsub") || c.contains("hdmv")
}

/// Burning a picture subtitle in needs the software converter and an overlay: the video filter chain
/// (`-vf X`) becomes `-filter_complex "[0:v:0][0:s:N]overlay[ov];[ov]X[v]"`, and the video comes from `[v]`.
/// Returns the settings to convert with (software when burning).
fn burn_settings(settings: &Settings, file: &MediaFile, req: &Request) -> Option<(Settings, u32)> {
    let n = req.burn_subtitle?;
    let codec = file.streams.iter().filter(|s| s.kind == "subtitle").nth(n as usize).map(|s| s.codec.clone())?;
    if !is_picture_subtitle(&codec) { return None; }
    let mut st = settings.clone();
    st.hw_accel = "none".into();
    Some((st, n))
}

fn apply_burn(video: &mut Vec<String>, audio: &mut [String], n: u32) {
    let Some(i) = video.iter().position(|a| a == "-vf") else { return };
    let chain = video.get(i + 1).cloned().unwrap_or_default();
    video.drain(i..=(i + 1).min(video.len() - 1));
    video.extend(["-filter_complex".to_string(), format!("[0:v:0][0:s:{n}]overlay=eof_action=pass[ov];[ov]{chain}[v]")]);
    for a in audio.iter_mut() {
        if a == "0:v:0?" { *a = "[v]".into(); }
    }
}

fn audio_args(file: &MediaFile, req: &Request) -> Vec<String> {
    let s = |x: &str| x.to_string();
    let mut v = vec![];
    // Choose the audio track: asked-for one, else the default.
    let audio_streams: Vec<_> = file.streams.iter().filter(|s| s.kind == "audio").collect();
    let pick = req
        .audio_stream
        .and_then(|i| audio_streams.iter().position(|s| s.index == i))
        .or_else(|| audio_streams.iter().position(|s| s.default))
        .unwrap_or(0);
    v.extend([s("-map"), s("0:v:0?"), s("-map"), format!("0:a:{pick}?")]);
    v.extend([
        s("-c:a"), s("aac"),
        s("-ac"), TRANSCODE_AUDIO_CHANNELS.to_string(),
        s("-b:a"), format!("{TRANSCODE_AUDIO_KBPS}k"),
    ]);
    v
}

/// ffmpeg arguments for a progressive, seekable-by-restart MP4 stream on stdout.
pub fn progressive_args(settings: &Settings, item: &Item, file: &MediaFile, d: &Decision, req: &Request, offset_s: f64) -> Vec<String> {
    let _ = item;
    let burn = burn_settings(settings, file, req);
    let method = if burn.is_some() { Method::Transcode } else if d.method == Method::Direct { Method::Remux } else { d.method.clone() };
    let (pre, mut video) = video_args(burn.as_ref().map(|b| &b.0).unwrap_or(settings), d, &method, req.video_quality.unwrap_or(99), req.h264_level);
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into()];
    a.extend(pre);
    if offset_s > 0.0 {
        a.extend(["-ss".into(), format!("{offset_s:.3}")]);
    }
    a.extend(["-i".into(), file.path.clone()]);
    let mut audio = audio_args(file, req);
    if let Some((_, n)) = &burn { apply_burn(&mut video, &mut audio, *n); }
    a.extend(audio);
    a.extend(video);
    a.extend([
        "-sn".into(), "-dn".into(),
        "-movflags".into(), "frag_keyframe+empty_moov+default_base_moof".into(),
        "-f".into(), "mp4".into(), "pipe:1".into(),
    ]);
    a
}

/// "Optimize": versions saved as a file that every browser plays directly
/// (MP4, H.264 + stereo AAC, moov at the front): (name, max height, video kbit/s).
/// 0 = the source's height; its bitrate is kept up to 20 Mbit/s.
pub const OPTIMIZE: &[(&str, u32, u32)] = &[("original", 0, 0), ("tv", 1080, 8000), ("mobile", 720, 2000)];

/// ffmpeg arguments that write an optimized version of `file` to `out`
/// (a whole file; `-progress pipe:1` reports how far it got).
pub fn optimize_args(settings: &Settings, file: &MediaFile, preset: &str, out: &str) -> Option<Vec<String>> {
    let &(_, h, kb) = OPTIMIZE.iter().find(|p| p.0 == preset)?;
    let src_h = file.video().map(|v| v.height).unwrap_or(0);
    let height = if h > 0 && src_h > h { h } else { 0 };
    let kb = if kb > 0 { kb } else { ((file.bitrate / 1000) as u32).clamp(2000, 20000) };
    Some(file_args(settings, file, &["-i".into(), file.path.clone()], height, kb, out))
}

/// ffmpeg arguments that write a whole MP4 file (H.264 + stereo AAC, moov at
/// the front) at most `height` high (0 = keep) at `kb` kbit/s; `input` is
/// the input part ("-i", path, or a concat list for a movie in parts).
pub fn file_args(settings: &Settings, file: &MediaFile, input: &[String], height: u32, kb: u32, out: &str) -> Vec<String> {
    let d = Decision { method: Method::Transcode, reasons: vec![], height, bitrate_kbps: kb };
    let (pre, video) = video_args(settings, &d, &Method::Transcode, 99, Some(41));
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-y".into()];
    a.extend(pre);
    a.extend(input.iter().cloned());
    a.extend(audio_args(file, &Request::default()));
    a.extend(video);
    a.extend(["-sn".into(), "-dn".into(), "-movflags".into(), "+faststart".into(), "-progress".into(), "pipe:1".into(), "-f".into(), "mp4".into(), out.into()]);
    a
}

/// Downloads: the size a converted copy is made in for a quality name
/// (height, video kbit/s), never larger than the original; None = keep the
/// original as it is (asked for, or it already fits and plays everywhere).
pub fn download_plan(file: &MediaFile, quality: &str) -> Option<(u32, u32)> {
    let (h, kb) = self::quality(quality);
    if quality == "original" || (h == 0 && kb == 0) { return None; }
    let d = decide(file, &Request { max_height: h, max_kbps: kb, ..Default::default() });
    if d.method == Method::Direct { return None; }
    let src_h = file.video().map(|v| v.height).unwrap_or(0);
    let height = if src_h > h { h } else { 0 };
    let src_kb = (file.bitrate / 1000) as u32;
    let kb = if src_kb > 0 { kb.min(src_kb.max(1000)) } else { kb };
    Some((height, kb))
}

/// ffmpeg arguments for an HLS session written into `dir`.
pub fn hls_args(settings: &Settings, file: &MediaFile, d: &Decision, req: &Request, offset_s: f64, dir: &str) -> Vec<String> {
    let burn = burn_settings(settings, file, req);
    let method = if burn.is_some() { Method::Transcode } else if d.method == Method::Direct { Method::Remux } else { d.method.clone() };
    let (pre, mut video) = video_args(burn.as_ref().map(|b| &b.0).unwrap_or(settings), d, &method, req.video_quality.unwrap_or(99), req.h264_level);
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into()];
    a.extend(pre);
    if offset_s > 0.0 {
        a.extend(["-ss".into(), format!("{offset_s:.3}")]);
    }
    a.extend(["-i".into(), file.path.clone()]);
    let mut audio = audio_args(file, req);
    if let Some((_, n)) = &burn { apply_burn(&mut video, &mut audio, *n); }
    a.extend(audio);
    a.extend(video);
    let seg = settings.hls_segment_seconds.max(2);
    a.extend([
        "-sn".into(), "-dn".into(),
        "-f".into(), "hls".into(),
        "-hls_time".into(), seg.to_string(),
        "-hls_list_size".into(), "0".into(),
        "-hls_playlist_type".into(), "event".into(),
        "-hls_segment_filename".into(), format!("{dir}/seg%05d.ts"),
        format!("{dir}/index.m3u8"),
    ]);
    a
}

/// WebVTT subtitles from an embedded track or a side file.
pub fn subtitle_args(source: &str, embedded_index: Option<u32>) -> Vec<String> {
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-i".into(), source.into()];
    if let Some(i) = embedded_index {
        a.extend(["-map".into(), format!("0:s:{i}")]);
    }
    a.extend(["-f".into(), "webvtt".into(), "pipe:1".into()]);
    a
}

pub fn thumb_args(source: &str, at_s: f64, out: &str) -> Vec<String> {
    vec![
        "-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-y".into(),
        "-ss".into(), format!("{at_s:.2}"), "-i".into(), source.into(),
        "-frames:v".into(), "1".into(), "-vf".into(), "scale=360:-2".into(), out.into(),
    ]
}

/// A wide still for the page background (when no background was found online).
pub fn art_args(source: &str, at_s: f64, out: &str) -> Vec<String> {
    vec![
        "-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-y".into(),
        "-ss".into(), format!("{at_s:.2}"), "-i".into(), source.into(),
        "-frames:v".into(), "1".into(), "-vf".into(), "scale=1280:-2".into(), "-q:v".into(), "3".into(), out.into(),
    ]
}

pub fn content_type(ext: &str) -> &'static str {
    match ext {
        "mp4" | "m4v" | "f4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" | "mk3d" => "video/x-matroska",
        "mov" | "qt" => "video/quicktime",
        "avi" | "divx" | "xvid" => "video/x-msvideo",
        "ts" | "m2ts" | "mts" | "m2t" | "trp" | "tod" => "video/mp2t",
        "mpg" | "mpeg" | "mpe" | "vob" | "m1v" | "m2v" | "mpv" | "mp2" => "video/mpeg",
        "wmv" | "asf" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "ogv" | "ogm" => "video/ogg",
        "3gp" => "video/3gpp",
        "3g2" => "video/3gpp2",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "m3u8" => "application/vnd.apple.mpegurl",
        "vtt" => "text/vtt; charset=utf-8",
        "srt" => "application/x-subrip",
        _ => "application/octet-stream",
    }
}

/// Is ffmpeg there, and which hardware encoders does it have?
pub fn detect(settings: &Settings) -> Value {
    let run = |prog: &str, args: &[&str]| {
        std::process::Command::new(prog)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    };
    let version = run(&settings.ffmpeg, &["-hide_banner", "-version"]);
    let probe = run(&settings.ffprobe, &["-hide_banner", "-version"]);
    let encoders = run(&settings.ffmpeg, &["-hide_banner", "-encoders"]).unwrap_or_default();
    let hw: Vec<&str> = ["h264_vaapi", "h264_qsv", "h264_nvenc", "h264_videotoolbox", "h264_v4l2m2m"]
        .into_iter()
        .filter(|e| encoders.contains(e))
        .collect();
    json!({
        "ffmpeg": settings.ffmpeg,
        "ffmpeg_found": version.is_some(),
        "ffmpeg_version": version.as_deref().and_then(|v| v.lines().next()).unwrap_or(""),
        "ffprobe_found": probe.is_some(),
        "hardware_encoders": hw,
        "render_device": std::path::Path::new("/dev/dri/renderD128").exists(),
        "hw_accel": settings.hw_accel,
        "hw_in_use": effective_hw(settings),
        "qualities": QUALITIES.iter().map(|q| json!({"name": q.0, "height": q.1, "kbps": q.2})).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Stream;
    fn file(container: &str, v: &str, a: &str, ch: u32, depth: u32) -> MediaFile {
        MediaFile {
            container: container.into(),
            probed: true,
            streams: vec![
                Stream { kind: "video".into(), codec: v.into(), height: 1080, bit_depth: depth, ..Default::default() },
                Stream { kind: "audio".into(), codec: a.into(), channels: ch, default: true, ..Default::default() },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn decisions() {
        let r = Request::default();
        assert_eq!(decide(&file("mp4", "h264", "aac", 2, 8), &r).method, Method::Direct);
        assert_eq!(decide(&file("mkv", "h264", "ac3", 6, 8), &r).method, Method::Remux);
        assert_eq!(decide(&file("mkv", "hevc", "aac", 2, 10), &r).method, Method::Transcode);
        assert_eq!(decide(&file("mp4", "h264", "aac", 8, 8), &r).method, Method::Remux);
        let low = Request { max_height: 720, max_kbps: 4000, ..Default::default() };
        assert_eq!(decide(&file("mp4", "h264", "aac", 2, 8), &low).method, Method::Transcode);
    }
    #[test]
    fn download_plans() {
        let mut f = file("mp4", "h264", "aac", 2, 8);
        f.bitrate = 3_000_000;
        // Small enough and plays everywhere: the original is sent.
        assert_eq!(download_plan(&f, "1080p-8"), None);
        assert_eq!(download_plan(&f, "original"), None);
        // 1080p is too high for 720p: scaled, never above the source's bitrate.
        assert_eq!(download_plan(&f, "720p-4"), Some((720, 3000)));
        let mkv = MediaFile { bitrate: 20_000_000, ..file("mkv", "hevc", "dts", 6, 10) };
        assert_eq!(download_plan(&mkv, "1080p-8"), Some((0, 8000)));
        assert_eq!(download_plan(&mkv, "480p-1.5"), Some((480, 1500)));
    }
    #[test]
    fn original_decision_codes() {
        assert_eq!(decision_code(&Method::Direct), 1000);
        assert_eq!(decision_code(&Method::Transcode), 1001);
        assert_eq!(decision_text(2012), "File is missing");
        assert!(decision_text(4010).contains("simultaneous video transcodes"));
        assert_eq!(decision_text(9999), "");
    }
    #[test]
    fn original_quality_formula() {
        // x264: hi 24, lo 16 → best quality CRF 16, worst 24, middle 20.
        assert_eq!(quality_scale(99, 16, 24), 16);
        assert_eq!(quality_scale(0, 16, 24), 24);
        assert_eq!(quality_scale(50, 16, 24), 20);
        // VAAPI qp range 15…25
        assert_eq!(quality_scale(99, 15, 25), 15);
    }
    #[test]
    fn hardware_rate_modes() {
        let d = Decision { method: Method::Transcode, reasons: vec![], height: 720, bitrate_kbps: 4000 };
        let mut st = Settings { hw_accel: "vaapi".into(), ..Settings::default() };
        let (_, out) = video_args(&st, &d, &Method::Transcode, 99, None);
        assert!(out.windows(2).any(|w| w[0] == "-b:v" && w[1] == "4000k"));
        st.hw_constant_quality = true;
        let (_, out) = video_args(&st, &d, &Method::Transcode, 99, None);
        assert!(out.windows(2).any(|w| w[0] == "-qp" && w[1] == "15"));
        st.hw_accel = "qsv".into();
        let (_, out) = video_args(&st, &d, &Method::Transcode, 0, None);
        assert!(out.windows(2).any(|w| w[0] == "-q:v" && w[1] == "25"));
        assert_eq!(hw_qp(50), 20);
    }
    #[test]
    fn h264_level_rules() {
        // Level 4.0: buffer 25000 kbit → bufsize at most 31250k; 4.1: 62500 → 78125k.
        assert_eq!(bufsize_kbps(20000, Some(40)), 31250);
        assert_eq!(bufsize_kbps(4000, Some(41)), 8000);
        assert_eq!(bufsize_kbps(20000, None), 40000);
        assert_eq!(parse_level("4.1"), Some(41));
        assert_eq!(parse_level("41"), Some(41));
        assert_eq!(parse_level("x"), None);
        let d = Decision { method: Method::Transcode, reasons: vec![], height: 1080, bitrate_kbps: 20000 };
        let st = Settings::default();
        let (_, out) = video_args(&st, &d, &Method::Transcode, 99, Some(40));
        assert!(out.windows(2).any(|w| w[0] == "-level" && w[1] == "4.0"));
        assert!(out.windows(2).any(|w| w[0] == "-bufsize" && w[1] == "31250k"));
    }
    #[test]
    fn watched_point() {
        use crate::db::watched_at_ms;
        // 100 s video, 90 % threshold, credits at 80 s
        assert_eq!(watched_at_ms(3, 90, 100_000, &[80_000]), 80_000);
        assert_eq!(watched_at_ms(3, 90, 100_000, &[95_000]), 90_000);
        assert_eq!(watched_at_ms(3, 90, 100_000, &[]), 90_000);
        assert_eq!(watched_at_ms(0, 90, 100_000, &[80_000]), 90_000);
        assert_eq!(watched_at_ms(1, 90, 100_000, &[70_000, 96_000]), 96_000);
        assert_eq!(watched_at_ms(2, 90, 100_000, &[70_000, 96_000]), 70_000);
        assert!(crate::scanner::is_credits_title("End Credits"));
        assert!(crate::scanner::is_credits_title("Abspann"));
        assert!(!crate::scanner::is_credits_title("Chapter 3"));
    }
    #[test]
    fn credits_marker_switches() {
        use crate::db::{Db, Item, ItemKind, MediaFile, Section, SectionKind};
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 3, "settings": Settings::default(),
            "sections": [], "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        db.sections.push(Section { id: 1, name: "M".into(), kind: SectionKind::Movie, paths: vec![], created_at: 0, scanned_at: 0, credits_detection: true, language: String::new() });
        let mut it: Item = serde_json::from_value(serde_json::json!({"id": 2, "section_id": 1, "kind": "movie", "title": "t", "added_at": 0, "updated_at": 0})).unwrap();
        it.kind = ItemKind::Movie;
        it.file = Some(MediaFile { path: "x".into(), size: 0, modified: 0, container: "mkv".into(), format_name: String::new(), duration_ms: 100_000, bitrate: 0, streams: vec![], probed: true, external_subtitles: vec![], credits_ms: vec![80_000], ..Default::default() });
        assert_eq!(db.settings.credits_markers_behaviour, "asap");
        assert_eq!(db.credits_markers(&it), vec![80_000]);
        db.sections[0].credits_detection = false;
        assert!(db.credits_markers(&it).is_empty());
        db.sections[0].credits_detection = true;
        db.settings.credits_markers_behaviour = "never".into();
        assert!(db.credits_markers(&it).is_empty());
        it.extra.insert("credits_ms".into(), serde_json::json!([70_000]));
        assert_eq!(db.credits_markers(&it), vec![70_000]);
    }
}
