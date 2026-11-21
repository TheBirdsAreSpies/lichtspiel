//! Where the end credits start: from chapter names, or by looking at the last minutes of the picture.

use super::*;

/// Reads only the chapters of a file and returns where credits start (ms).
/// None when ffprobe can't read the file.
pub fn read_credits(ffprobe: &str, path: &str) -> Option<Vec<u64>> {
    let out = Command::new(ffprobe).args(["-v", "quiet", "-print_format", "json", "-show_chapters"]).arg(path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(
        v["chapters"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|c| is_credits_title(c["tags"]["title"].as_str().unwrap_or("")))
                    .filter_map(|c| c["start_time"].as_str().and_then(|t| t.parse::<f64>().ok()))
                    .map(|t| (t * 1000.0) as u64)
                    .collect()
            })
            .unwrap_or_default(),
    )
}

//
// The end of the video is looked at directly: end credits are
// mostly dark pictures with a little bright text on them, for a while.

/// Small greyscale frames, one per second, of the last part of a video.
/// (160×90: smaller, and the thin letters of credits blur into the background.)
pub(super) const FRAME_W: usize = 160;
pub(super) const FRAME_H: usize = 90;

/// What one frame looks like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameKind {
    /// Dark with a little bright text: looks like credits.
    Credits,
    /// (Nearly) black: fits between credit cards.
    Black,
    /// A normal picture.
    Picture,
}

pub fn frame_kind(px: &[u8]) -> FrameKind {
    let n = px.len().max(1) as f32;
    let dark = px.iter().filter(|&&p| p < 40).count() as f32 / n;
    let bright = px.iter().filter(|&&p| p > 100).count() as f32 / n;
    if dark >= 0.9 && bright < 0.003 {
        FrameKind::Black
    } else if dark >= 0.75 && (0.003..=0.2).contains(&bright) {
        FrameKind::Credits
    } else {
        FrameKind::Picture
    }
}

/// The second (counted from the first frame) where the credits begin: the
/// longest stretch of at least `min_len` seconds that is mostly credit-like.
pub fn credits_start(kinds: &[FrameKind], min_len: usize) -> Option<usize> {
    if kinds.len() < min_len {
        return None;
    }
    let v: Vec<f32> = kinds.iter().map(|k| match k { FrameKind::Credits => 1.0, FrameKind::Black => 0.5, FrameKind::Picture => 0.0 }).collect();
    // Average over 9 s, so a single bright frame doesn't break the credits.
    let half = 4usize;
    let smooth: Vec<f32> = (0..v.len())
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half + 1).min(v.len());
            v[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect();
    let mut best: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < smooth.len() {
        if smooth[i] < 0.6 {
            i += 1;
            continue;
        }
        let start = i;
        while i < smooth.len() && smooth[i] >= 0.6 {
            i += 1;
        }
        let len = i - start;
        let has_text = kinds[start..i].iter().filter(|k| **k == FrameKind::Credits).count() * 3 >= len;
        if len >= min_len && has_text && best.is_none_or(|(_, l)| len >= l) {
            best = Some((start, len));
        }
    }
    // Start at the first frame that really shows credits.
    best.map(|(s, l)| (s..s + l).find(|&k| kinds[k] == FrameKind::Credits).unwrap_or(s))
}

/// Looks at the picture of the last part of a video (the last 30 %, between
/// 1 and 15 minutes) and returns where the end credits start (ms), None when
/// there are none, or an error when ffmpeg can't read the file.
pub fn detect_credits_in_video(ffmpeg: &str, path: &str, duration_ms: u64) -> Result<Option<u64>, String> {
    if duration_ms < 60_000 {
        return Ok(None);
    }
    let full = (duration_ms * 3 / 10).clamp(60_000, 15 * 60_000).min(duration_ms);
    // Most credits are short: look at the last 5 minutes first, and only when the credits reach back to the
    // start of that window (they may be longer) or the video is longer than that, look at the whole window.
    const QUICK: u64 = 5 * 60_000;
    if full > QUICK {
        let start_ms = duration_ms - QUICK;
        match credits_in_window(ffmpeg, path, start_ms)? {
            Some(ms) if ms > start_ms + 30_000 => return Ok(Some(ms)),
            None => return Ok(None),
            Some(_) => {}
        }
    }
    credits_in_window(ffmpeg, path, duration_ms - full)
}

/// The credits start found in the picture from `start_ms` to the end of the file.
fn credits_in_window(ffmpeg: &str, path: &str, start_ms: u64) -> Result<Option<u64>, String> {
    use std::io::Read as _;
    use std::process::Stdio;
    let mut child = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-skip_frame", "nokey", "-ss", &format!("{:.3}", start_ms as f64 / 1000.0), "-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-an", "-sn", "-dn", "-vf", &format!("fps=1,scale={FRAME_W}:{FRAME_H},format=gray"), "-f", "rawvideo", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("ffmpeg could not be started: {e}"))?;
    let mut out = child.stdout.take().ok_or("no output from ffmpeg")?;
    let reader = std::thread::spawn(move || {
        let mut buf = vec![];
        let _ = out.read_to_end(&mut buf);
        buf
    });
    // Only key frames are decoded, so this is quick; still, never wait forever.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let buf = reader.join().unwrap_or_default();
    let frame = FRAME_W * FRAME_H;
    if status.is_none() {
        return Err("ffmpeg took too long reading the video".into());
    }
    if buf.len() < frame * 10 {
        return Err("ffmpeg could not read the picture of this video".into());
    }
    let kinds: Vec<FrameKind> = buf.chunks_exact(frame).map(frame_kind).collect();
    Ok(credits_start(&kinds, 20).map(|s| start_ms + s as u64 * 1000))
}

/// Credits for one file: chapters named like "Credits" first, else the
/// picture itself. Returns the markers and how they were found, or an error.
pub fn find_credits(ffmpeg: &str, ffprobe: &str, path: &str, duration_ms: u64, use_picture: bool) -> Result<(Vec<u64>, &'static str), String> {
    let chapters = read_credits(ffprobe, path);
    if let Some(c) = chapters.as_ref().filter(|c| !c.is_empty()) {
        return Ok((c.clone(), "chapters"));
    }
    if !use_picture {
        return chapters.map(|c| (c, "chapters")).ok_or_else(|| "ffprobe could not read this file".into());
    }
    match detect_credits_in_video(ffmpeg, path, duration_ms)? {
        Some(ms) => Ok((vec![ms], "picture")),
        None => Ok((vec![], "picture")),
    }
}

/// Chapter names that mean "end credits" (a few languages).
pub fn is_credits_title(t: &str) -> bool {
    let t = t.to_lowercase();
    ["credits", "credit roll", "end titles", "abspann", "générique", "generique", "créditos", "titoli di coda", "aftiteling"]
        .iter()
        .any(|w| t.contains(w))
}
