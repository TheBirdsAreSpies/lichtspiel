//! Playback decisions (direct play, remux, transcode) and the ffmpeg
//! command lines behind them. The browser profile: HLS in MPEG-TS with H.264 + AAC/MP3, progressive
//! MP4 with H.264 + AAC, at most 8-bit video and 6 audio channels for
//! direct play, and multi-channel audio mixed down to stereo when
//! transcoding.

use crate::data::db::{MediaFile, Settings};
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
/// Containers of audio-only files that a web browser plays as they are.
pub const BROWSER_AUDIO_CONTAINERS: &[&str] = &["mp3", "m4a", "ogg", "oga", "opus", "wav", "flac"];
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
    /// The video is HEVC (copied into an mp4 it needs the hvc1 tag).
    #[serde(skip)]
    pub hevc: bool,
    /// The video is HDR (HDR10 / HLG): converting it needs tone mapping to look right on a normal screen.
    #[serde(skip)]
    pub hdr: bool,
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
    /// Lift the dialogue when surround sound is mixed down to stereo.
    pub boost_dialogue: bool,
    /// The app's own player (libmpv) plays every container and codec: nothing needs converting for the format.
    pub native: bool,
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

/// HDR10 / HLG pictures (Dolby Vision files that carry one of these as their base layer too).
fn is_hdr_picture(v: &crate::data::db::Stream) -> bool {
    matches!(v.color_transfer.as_str(), "smpte2084" | "arib-std-b67")
}

/// ffmpeg can tone map: it has the zscale (libzimg) and tonemap filters. Looked up once per ffmpeg.
pub(crate) fn can_tonemap(ffmpeg: &str) -> bool {
    use std::sync::{Mutex, OnceLock};
    #[cfg(test)]
    { let _ = ffmpeg; return true; }
    #[allow(unreachable_code)]
    {
        static CACHE: OnceLock<Mutex<std::collections::HashMap<String, bool>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(Default::default()));
        if let Some(v) = cache.lock().unwrap().get(ffmpeg) { return *v; }
        let ok = std::process::Command::new(ffmpeg).args(["-hide_banner", "-filters"]).stdin(std::process::Stdio::null()).output().is_ok_and(|o| {
            let t = String::from_utf8_lossy(&o.stdout);
            t.contains(" zscale ") && t.contains(" tonemap ")
        });
        cache.lock().unwrap().insert(ffmpeg.to_string(), ok);
        ok
    }
}

/// The filters in front of the scaling that turn HDR (PQ / HLG, BT.2020) into SDR (BT.709): to linear
/// light, to BT.709 primaries, compress the highlights (hable), back to the BT.709 curve.
const TONEMAP: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,";

pub fn decide(file: &MediaFile, req: &Request) -> Decision {
    let mut reasons = vec![];
    let container_ok = req.native
        || BROWSER_CONTAINERS.contains(&file.container.as_str())
        || (file.probed && file.video().is_none() && BROWSER_AUDIO_CONTAINERS.contains(&file.container.as_str()))
        || req.extra_containers.iter().any(|c| c == &file.container);
    if !container_ok {
        reasons.push(format!("container {} does not play in the browser", file.container));
    }
    let v = file.video();
    let a = file.audio();
    let vcodec = v.map(|v| v.codec.as_str()).unwrap_or("");
    let video_ok = req.native
        || !file.probed
        || v.is_none()
        || BROWSER_VIDEO.contains(&vcodec)
        || req.extra_video.iter().any(|c| c == vcodec);
    if !video_ok {
        reasons.push(format!("video codec {vcodec} needs converting"));
    }
    // A codec the browser said it plays (HEVC in Chrome with a hardware decoder) may be 10-bit.
    let client_codec = req.extra_video.iter().any(|c| c == vcodec);
    let depth_ok = req.native || v.is_none_or(|v| v.bit_depth <= MAX_DIRECT_BIT_DEPTH || (client_codec && v.bit_depth <= 10));
    if !depth_ok {
        reasons.push(format!("{}-bit video needs converting to 8-bit", v.map(|v| v.bit_depth).unwrap_or(0)));
    }
    let audio_ok = req.native || a.is_none_or(|a| BROWSER_AUDIO.contains(&a.codec.as_str()) && a.channels <= MAX_DIRECT_AUDIO_CHANNELS);
    if !audio_ok {
        if let Some(a) = a {
            reasons.push(format!("audio {} with {} channels needs converting", a.codec, a.channels));
        }
    }
    let src_h = v.map(|v| v.height).unwrap_or(0);
    // The speed the file really needs (deep analysis), not its average.
    let too_big = (req.max_height > 0 && src_h > req.max_height)
        || (req.max_kbps > 0 && crate::playback::deep::required_kbps(file, req.max_kbps) > req.max_kbps);
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
        if vcodec == "h264" || vcodec.is_empty() || !file.probed || client_codec {
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
    Decision { method, reasons, height, bitrate_kbps, hevc: vcodec == "hevc", hdr: v.is_some_and(is_hdr_picture) }
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

/// Playback decision codes and their messages (1xxx = fine, 2xxx = cannot play, 3xxx = direct
/// play refused, 4xxx = conversion problems).
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
    let want = settings.hw_accel.as_str();
    if want == "none" || settings.on_helper {
        return want.into();
    }
    // (The tests run on machines without such hardware: they look at the command lines only.)
    #[cfg(test)]
    if want != "auto" {
        return want.into();
    }
    let key = format!("{}|{want}|{}", settings.ffmpeg, settings.hw_device);
    let cache = CACHE.get_or_init(|| Mutex::new(Default::default()));
    if let Some(v) = cache.lock().unwrap().get(&key) {
        return v.clone();
    }
    // "auto" tries the graphics chip; a backend that was picked by hand is tested too, because an
    // ffmpeg may list an encoder (h264_qsv, h264_nvenc) although this computer has no such hardware.
    let pick = if want == "auto" {
            ["vaapi", "nvenc", "qsv"].into_iter().find(|b| hw_works(&settings.ffmpeg, b, &settings.hw_device)).unwrap_or("none").to_string()
        }
        else if hw_works(&settings.ffmpeg, want, &settings.hw_device) { want.to_string() }
        else {
            elog!("hardware acceleration '{want}' does not work on this computer (a test encode failed); converting with the processor instead");
            "none".to_string()
        };
    cache.lock().unwrap().insert(key, pick.clone());
    pick
}

/// The VAAPI render node to use (the setting, or the first one).
fn vaapi_device(device: &str) -> &str {
    let d = device.trim();
    if d.is_empty() { "/dev/dri/renderD128" } else { d }
}

/// The NVENC card number: digits only, else the first card.
fn nvenc_gpu(device: &str) -> String {
    let d = device.trim();
    if !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()) { d.to_string() } else { "0".to_string() }
}

/// A one-frame test encode with the hardware encoder of this backend.
pub fn hw_works(ffmpeg: &str, backend: &str, device: &str) -> bool {
    let src = ["-hide_banner", "-loglevel", "error", "-nostdin"];
    let mut args: Vec<String> = src.iter().map(|x| x.to_string()).collect();
    let lavfi = ["-f", "lavfi", "-i", "testsrc=size=320x240:rate=1", "-frames:v", "1"];
    match backend {
        "vaapi" => {
            let dev = vaapi_device(device);
            if !std::path::Path::new(dev).exists() { return false; }
            args.extend(["-vaapi_device", dev].map(String::from));
            args.extend(lavfi.map(String::from));
            args.extend(["-vf", "format=nv12,hwupload", "-c:v", "h264_vaapi"].map(String::from));
        }
        "qsv" => { args.extend(lavfi.map(String::from)); args.extend(["-vf", "format=nv12", "-c:v", "h264_qsv"].map(String::from)); }
        "nvenc" => {
            args.extend(lavfi.map(String::from));
            args.extend(["-vf", "format=yuv420p", "-c:v", "h264_nvenc"].map(String::from));
            args.extend(["-gpu".to_string(), nvenc_gpu(device)]);
        }
        _ => return false,
    }
    args.extend(["-f", "null", "-"].map(String::from));
    std::process::Command::new(ffmpeg).args(&args)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .status().is_ok_and(|s| s.success())
}

/// Hardware constant-quality number: is the quality scaled to 15..25,
/// so 99 → 15 (best) … 0 → 25.
pub fn hw_qp(quality: u32) -> u32 {
    quality_scale(quality, 15, 25)
}

/// Input options and the picture part of an ffmpeg command line.
/// Encoder setup:
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
        // HEVC in an mp4 needs the "hvc1" tag for browsers.
        if d.hevc { out.extend([s("-tag:v"), s("hvc1")]); }
        return (pre, out);
    }
    let kb = d.bitrate_kbps.max(300);
    let tm = if d.hdr && settings.hdr_tone_mapping && (settings.on_helper || can_tonemap(&settings.ffmpeg)) { TONEMAP } else { "" };
    if !tm.is_empty() {
        out.extend([s("-colorspace"), s("bt709"), s("-color_primaries"), s("bt709"), s("-color_trc"), s("bt709")]);
    }
    let scale_h = if d.height > 0 { d.height } else { 0 };
    let tq = settings.transcoder_quality.as_str();
    // Hardware encoders: a bitrate (-b:v), or with constant quality on, a
    // second formula: VAAPI gets -qp, others -q, = hw_qp(quality).
    let rate = |flag: &str| -> Vec<String> {
        if settings.hw_constant_quality { vec![s(flag), hw_qp(quality).to_string()] } else { vec![s("-b:v"), format!("{kb}k")] }
    };
    match effective_hw(settings).as_str() {
        "vaapi" => {
            pre.extend([s("-vaapi_device"), s(vaapi_device(&settings.hw_device))]);
            let scale = if scale_h > 0 { format!(",scale_vaapi=w=-2:h={scale_h}") } else { String::new() };
            out.extend([s("-vf"), format!("{tm}format=nv12,hwupload{scale}"), s("-c:v"), s("h264_vaapi")]);
            out.extend(rate("-qp"));
        }
        "qsv" => {
            let scale = if scale_h > 0 { format!("scale=-2:{scale_h},") } else { String::new() };
            out.extend([s("-vf"), format!("{tm}{scale}format=nv12"), s("-c:v"), s("h264_qsv")]);
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
            out.extend([s("-vf"), format!("{tm}{scale}format=yuv420p"), s("-c:v"), s("h264_nvenc")]);
            out.extend(rate("-q:v"));
            if !settings.hw_device.trim().is_empty() { out.extend([s("-gpu"), nvenc_gpu(&settings.hw_device)]); }
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
                s("-vf"), format!("{tm}{scale}format=yuv420p"),
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
    if req.boost_dialogue && audio_streams.get(pick).is_some_and(|a| a.channels > 2) {
        // Any surround layout → 5.1, then a stereo mix with a strong centre (the voices); the limiter stops clipping.
        v.extend([s("-af"), s("aformat=channel_layouts=5.1,pan=stereo|FL=0.9*FC+0.55*FL+0.4*BL|FR=0.9*FC+0.55*FR+0.4*BR,alimiter=limit=0.97")]);
    }
    v.extend([
        s("-c:a"), s("aac"),
        s("-ac"), TRANSCODE_AUDIO_CHANNELS.to_string(),
        s("-b:a"), format!("{TRANSCODE_AUDIO_KBPS}k"),
    ]);
    v
}

/// The `-ss` for a stream from `offset_s`. With a copied picture ffmpeg seeks 3/23 s early when the video has B-frames
/// (it seeks by decode time), so a seek to exactly a key frame (`stream_start`) would land on the one before; a little
/// later finds it. The stream's times start at its first frame either way.
fn seek_target(method: &Method, offset_s: f64) -> f64 {
    if *method == Method::Remux { offset_s + 0.15 } else { offset_s }
}

/// ffmpeg arguments for a progressive, seekable-by-restart MP4 stream on stdout.
pub fn progressive_args(settings: &Settings, file: &MediaFile, d: &Decision, req: &Request, offset_s: f64) -> Vec<String> {
    let burn = burn_settings(settings, file, req);
    let method = if burn.is_some() { Method::Transcode } else if d.method == Method::Direct { Method::Remux } else { d.method.clone() };
    let (pre, mut video) = video_args(burn.as_ref().map(|b| &b.0).unwrap_or(settings), d, &method, req.video_quality.unwrap_or(99), req.h264_level);
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into()];
    a.extend(pre);
    if offset_s > 0.0 {
        // A copied picture can only start at a key frame, before the asked-for time. Sound cut exactly there would play
        // ahead of the picture by the difference, so with a copied picture the sound starts at the key frame as well.
        if method == Method::Remux { a.push("-noaccurate_seek".into()); }
        a.extend(["-ss".into(), format!("{:.3}", seek_target(&method, offset_s))]);
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

/// Where the stream `progressive_args` makes for a seek to `offset_s` really starts. A copied picture starts at the last
/// key frame at or before it, so a player that counts the stream as starting at `offset_s` would show times ahead by up
/// to the key frame distance. The time is rounded up to the millisecond, so a stream asked for at exactly that time
/// starts at that key frame. `offset_s` when the picture is converted or no key frame is found within 30 s before.
pub fn stream_start(settings: &Settings, file: &MediaFile, d: &Decision, req: &Request, offset_s: f64) -> f64 {
    let copied = burn_settings(settings, file, req).is_none() && matches!(d.method, Method::Direct | Method::Remux);
    if offset_s <= 0.0 || !copied { return offset_s.max(0.0); }
    keyframe_before(&settings.ffprobe, &file.path, offset_s).unwrap_or(offset_s)
}

/// The last key frame of the first video stream at or before `at_s` (seconds from the file's start), looking back 30 s.
fn keyframe_before(ffprobe: &str, path: &str, at_s: f64) -> Option<f64> {
    let window = format!("{:.3}%{:.3}", (at_s - 30.0).max(0.0), at_s + 0.001);
    let out = std::process::Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-read_intervals", &window, "-show_entries", "packet=pts_time,flags:format=start_time", "-of", "json", path])
        .stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .output().ok()?;
    let j: Value = serde_json::from_slice(&out.stdout).ok()?;
    let num = |v: &Value| v.as_str().and_then(|s| s.parse::<f64>().ok());
    let start = j["format"].get("start_time").and_then(num).unwrap_or(0.0);
    let key = j["packets"].as_array()?.iter()
        .filter(|p| p["flags"].as_str().is_some_and(|f| f.starts_with('K')))
        .filter_map(|p| p.get("pts_time").and_then(num))
        .map(|t| t - start)
        .filter(|&t| t <= at_s)
        .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))))?;
    Some((key.max(0.0) * 1000.0).ceil() / 1000.0)
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
    let d = Decision { method: Method::Transcode, reasons: vec![], height, bitrate_kbps: kb, hevc: false, hdr: file.video().is_some_and(is_hdr_picture) };
    let (pre, video) = video_args(settings, &d, &Method::Transcode, 99, Some(41));
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-y".into()];
    a.extend(pre);
    a.extend(input.iter().cloned());
    // Optimized versions and downloads get the dialogue boost too (when it is on), as the voices matter most on a phone.
    a.extend(audio_args(file, &Request { boost_dialogue: settings.boost_dialogue, ..Default::default() }));
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
        // A copied picture can only start at a key frame, before the asked-for time. Sound cut exactly there would play
        // ahead of the picture by the difference, so with a copied picture the sound starts at the key frame as well.
        if method == Method::Remux { a.push("-noaccurate_seek".into()); }
        a.extend(["-ss".into(), format!("{:.3}", seek_target(&method, offset_s))]);
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

/// The picture inside an audio file (cover art), as a small jpg.
pub fn cover_args(source: &str, out: &str) -> Vec<String> {
    vec![
        "-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-y".into(),
        "-i".into(), source.into(), "-an".into(), "-frames:v".into(), "1".into(), "-vf".into(), "scale=400:-2".into(), out.into(),
    ]
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
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
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

/// The graphics devices this computer offers: (backend, value for the setting, name).
pub fn graphics_devices() -> Vec<(String, String, String)> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir("/dev/dri") {
        let mut nodes: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("renderD")).collect();
        nodes.sort();
        for n in nodes { out.push(("vaapi".to_string(), format!("/dev/dri/{n}"), format!("/dev/dri/{n}"))); }
    }
    if let Some(text) = std::process::Command::new("nvidia-smi").args(["--query-gpu=index,name", "--format=csv,noheader"]).output().ok()
        .filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).to_string()) {
        for l in text.lines() {
            if let Some((i, name)) = l.split_once(',') {
                let i = i.trim();
                if !i.is_empty() && i.chars().all(|c| c.is_ascii_digit()) { out.push(("nvenc".to_string(), i.to_string(), format!("{i} · {}", name.trim()))); }
            }
        }
    }
    out
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
    let working: Vec<&str> = ["vaapi", "qsv", "nvenc"].into_iter().filter(|b| hw.iter().any(|e| e.ends_with(b)) && hw_works(&settings.ffmpeg, b, &settings.hw_device)).collect();
    json!({
        "ffmpeg": settings.ffmpeg,
        "ffmpeg_found": version.is_some(),
        "ffmpeg_version": version.as_deref().and_then(|v| v.lines().next()).unwrap_or(""),
        "ffprobe_found": probe.is_some(),
        "hardware_encoders": hw,
        "hardware_working": working,
        "render_device": std::path::Path::new("/dev/dri/renderD128").exists(),
        "devices": graphics_devices().into_iter().map(|(b, v, n)| json!({"backend": b, "value": v, "name": n})).collect::<Vec<_>>(),
        "hw_accel": settings.hw_accel,
        "hw_in_use": effective_hw(settings),
        "qualities": QUALITIES.iter().map(|q| json!({"name": q.0, "height": q.1, "kbps": q.2})).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::db::Stream;
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
    fn optimized_copies_get_the_dialogue_boost() {
        let f = file("mkv", "h264", "ac3", 6, 8);
        let args = |boost: bool| file_args(&Settings { boost_dialogue: boost, hw_accel: "none".into(), ..Settings::default() }, &f, &["-i".into(), "x.mkv".into()], 0, 4000, "out.mp4");
        assert!(args(true).iter().any(|a| a.contains("alimiter")));
        assert!(!args(false).iter().any(|a| a.contains("alimiter")));
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
        let d = Decision { method: Method::Transcode, reasons: vec![], height: 720, bitrate_kbps: 4000, hevc: false, hdr: false };
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
    fn hdr_is_tone_mapped_when_converted() {
        let d = Decision { method: Method::Transcode, reasons: vec![], height: 1080, bitrate_kbps: 8000, hevc: true, hdr: true };
        let mut st = Settings { hw_accel: "none".into(), ..Settings::default() };
        let vf = |st: &Settings, d: &Decision| { let (_, o) = video_args(st, d, &Method::Transcode, 99, None); o.windows(2).find(|w| w[0] == "-vf").map(|w| w[1].clone()).unwrap() };
        let chain = vf(&st, &d);
        assert!(chain.starts_with("zscale=t=linear") && chain.contains("tonemap=hable") && chain.ends_with("scale=-2:1080,format=yuv420p"), "{chain}");
        // SDR video, the setting off, and a plain copy are left alone.
        assert_eq!(vf(&st, &Decision { hdr: false, ..d.clone() }), "scale=-2:1080,format=yuv420p");
        st.hdr_tone_mapping = false;
        assert_eq!(vf(&st, &d), "scale=-2:1080,format=yuv420p");
        st.hdr_tone_mapping = true;
        let (_, o) = video_args(&st, &d, &Method::Remux, 99, None);
        assert!(!o.iter().any(|a| a == "-vf"));
        // Hardware encoders get the same filters in front.
        st.hw_accel = "nvenc".into();
        assert!(vf(&st, &d).starts_with("zscale=t=linear"));
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
        let d = Decision { method: Method::Transcode, reasons: vec![], height: 1080, bitrate_kbps: 20000, hevc: false, hdr: false };
        let st = Settings::default();
        let (_, out) = video_args(&st, &d, &Method::Transcode, 99, Some(40));
        assert!(out.windows(2).any(|w| w[0] == "-level" && w[1] == "4.0"));
        assert!(out.windows(2).any(|w| w[0] == "-bufsize" && w[1] == "31250k"));
    }
    #[test]
    fn watched_point() {
        use crate::data::db::watched_at_ms;
        // 100 s video, 90 % threshold, credits at 80 s
        assert_eq!(watched_at_ms(3, 90, 100_000, &[80_000]), 80_000);
        assert_eq!(watched_at_ms(3, 90, 100_000, &[95_000]), 90_000);
        assert_eq!(watched_at_ms(3, 90, 100_000, &[]), 90_000);
        assert_eq!(watched_at_ms(0, 90, 100_000, &[80_000]), 90_000);
        assert_eq!(watched_at_ms(1, 90, 100_000, &[70_000, 96_000]), 96_000);
        assert_eq!(watched_at_ms(2, 90, 100_000, &[70_000, 96_000]), 70_000);
        assert!(crate::library::scanner::is_credits_title("End Credits"));
        assert!(crate::library::scanner::is_credits_title("Abspann"));
        assert!(!crate::library::scanner::is_credits_title("Chapter 3"));
    }
    #[test]
    fn credits_marker_switches() {
        use crate::data::db::{Db, Item, ItemKind, MediaFile, Section, SectionKind};
        let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 3, "settings": Settings::default(),
            "sections": [], "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        db.sections.push(Section { id: 1, name: "M".into(), kind: SectionKind::Movie, paths: vec![], created_at: 0, scanned_at: 0, credits_detection: true, intro_detection: true, language: String::new(), scan_interval_minutes: None });
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
