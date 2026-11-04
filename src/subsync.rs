//! Subtitle synchronization: finds out when people speak in a video (loud stretches in the
//! voice range) and moves a subtitle track so its lines fall on them. It tries a shift and
//! the usual frame-rate stretches (23.976 / 24 / 25 fps), and only applies a result it is sure of.

use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};

/// One step of the speech curve, in seconds.
pub const BIN_S: f64 = 0.05;
/// How far a subtitle may be off, either way.
pub const MAX_SHIFT_S: f64 = 150.0;
/// The speed ratios between common frame rates, tried besides "unchanged".
pub const SCALES: &[f64] = &[1.0, 25.0 / 23.976, 23.976 / 25.0, 25.0 / 24.0, 24.0 / 25.0, 24.0 / 23.976, 23.976 / 24.0];
/// How many standard deviations the best shift must stand out from the others.
pub const MIN_CONFIDENCE: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub offset_ms: i64,
    pub scale: f64,
}

impl Timing {
    pub fn from_json(v: &Value) -> Option<Timing> {
        Some(Timing { offset_ms: v.get("offset_ms")?.as_i64()?, scale: v.get("scale").and_then(|s| s.as_f64()).unwrap_or(1.0) })
    }
    pub fn to_json(self) -> Value {
        serde_json::json!({"offset_ms": self.offset_ms, "scale": self.scale})
    }
    pub fn is_none(self) -> bool {
        self.offset_ms == 0 && (self.scale - 1.0).abs() < 1e-9
    }
}

/// "01:02:03.456" or "02:03.456" -> seconds.
fn parse_ts(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + sec)
}

fn fmt_ts(sec: f64) -> String {
    let ms = (sec.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// "start --> end [settings]" -> (start, end, rest after the end time).
fn timing_line(line: &str) -> Option<(f64, f64, String)> {
    let (a, b) = line.split_once("-->")?;
    let start = parse_ts(a)?;
    let b = b.trim_start();
    let end_txt: String = b.chars().take_while(|c| !c.is_whitespace()).collect();
    let end = parse_ts(&end_txt)?;
    Some((start, end, b[end_txt.len()..].to_string()))
}

/// Start and end (seconds) of every line of a WebVTT file.
pub fn cues(vtt: &str) -> Vec<(f64, f64)> {
    vtt.lines().filter_map(timing_line).map(|(s, e, _)| (s, e)).collect()
}

/// The WebVTT text with every time `t` replaced by `t * scale + offset`.
pub fn shift_vtt(vtt: &str, timing: Timing) -> String {
    let off = timing.offset_ms as f64 / 1000.0;
    let mut out = String::with_capacity(vtt.len());
    for line in vtt.lines() {
        match timing_line(line) {
            Some((s, e, rest)) => out.push_str(&format!("{} --> {}{}", fmt_ts(s * timing.scale + off), fmt_ts(e * timing.scale + off), rest)),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// How loud the voice range is, in dB, every `BIN_S` seconds of the first audio track.
pub fn loudness_curve(ffmpeg: &str, path: &str) -> Result<Vec<f32>, String> {
    let mut child = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i", path, "-map", "0:a:0", "-vn", "-ac", "1", "-ar", "8000",
            "-af", "highpass=f=200,lowpass=f=3400", "-f", "s16le", "pipe:1"])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().map_err(|e| format!("ffmpeg could not be started: {e}"))?;
    let mut out = child.stdout.take().ok_or("no output")?;
    let per_bin = (8000.0 * BIN_S) as usize;
    let mut buf = vec![0u8; per_bin * 2];
    let mut curve = vec![];
    loop {
        let mut got = 0;
        while got < buf.len() {
            match out.read(&mut buf[got..]) { Ok(0) => break, Ok(n) => got += n, Err(_) => break }
        }
        if got < buf.len() { break; }
        let sum: f64 = buf.chunks_exact(2).map(|b| { let s = i16::from_le_bytes([b[0], b[1]]) as f64 / 32768.0; s * s }).sum();
        curve.push((10.0 * (sum / per_bin as f64 + 1e-10).log10()) as f32);
    }
    let _ = child.wait();
    if curve.len() < (60.0 / BIN_S) as usize { return Err("The audio is too short or could not be read".into()); }
    Ok(curve)
}

/// 1 where someone seems to speak, else 0, minus the average (so a line on silence counts against it).
pub fn speech_signal(curve: &[f32]) -> Vec<f64> {
    let mut sorted: Vec<f32> = curve.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize] as f64;
    let (floor, loud) = (at(0.15), at(0.9));
    let thr = floor + 0.45 * (loud - floor);
    let raw: Vec<f64> = curve.iter().map(|&l| if l as f64 > thr { 1.0 } else { 0.0 }).collect();
    let mean = raw.iter().sum::<f64>() / raw.len() as f64;
    raw.into_iter().map(|x| x - mean).collect()
}

/// For one stretch: the best shift (in bins) of the subtitle against the speech, its score, and
/// how far the best stands out from all the others (in standard deviations).
fn best_shift(cues: &[(f64, f64)], prefix: &[f64], scale: f64) -> Option<(i64, f64, f64)> {
    let max = (MAX_SHIFT_S / BIN_S) as i64;
    let n = prefix.len() as i64 - 1;
    let spans: Vec<(i64, i64)> = cues.iter().map(|(s, e)| (((s * scale) / BIN_S).floor() as i64, ((e * scale) / BIN_S).ceil() as i64)).filter(|(s, e)| e > s).collect();
    if spans.len() < 20 { return None; }
    let mut scores = Vec::with_capacity((2 * max + 1) as usize);
    for k in -max..=max {
        let mut sum = 0.0;
        for &(s, e) in &spans {
            let (a, b) = ((s + k).clamp(0, n), (e + k).clamp(0, n));
            sum += prefix[b as usize] - prefix[a as usize];
        }
        scores.push(sum);
    }
    let (best_i, best) = scores.iter().enumerate().fold((0, f64::MIN), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    let sd = (scores.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / scores.len() as f64).sqrt();
    if sd <= 0.0 { return None; }
    Some((best_i as i64 - max, best, (best - mean) / sd))
}

/// What to do to the subtitle (None: its lines do not fit the sound well enough to say).
pub fn find_timing(cue_list: &[(f64, f64)], signal: &[f64]) -> Option<(Timing, f64)> {
    let mut prefix = Vec::with_capacity(signal.len() + 1);
    prefix.push(0.0);
    for v in signal { prefix.push(prefix.last().unwrap() + v); }
    let mut results: Vec<(f64, i64, f64, f64)> = vec![];
    for &scale in SCALES {
        if let Some((k, score, z)) = best_shift(cue_list, &prefix, scale) { results.push((scale, k, score, z)); }
    }
    let plain = results.iter().find(|r| r.0 == 1.0).copied();
    let best = results.iter().copied().fold(None::<(f64, i64, f64, f64)>, |m, r| match m { Some(x) if x.2 >= r.2 => Some(x), _ => Some(r) })?;
    // A stretch must clearly beat "no stretch", else the plain shift is kept.
    let pick = match plain { Some(p) if best.2 < p.2 * 1.15 => p, _ => best };
    if pick.3 < MIN_CONFIDENCE { return None; }
    Some((Timing { offset_ms: (pick.1 as f64 * BIN_S * 1000.0).round() as i64, scale: pick.0 }, pick.3))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VTT: &str = "WEBVTT\n\n00:00:01.000 --> 00:00:03.500\nHello\n\n01:00:00.250 --> 01:00:02.000 line:90%\nWorld\n";

    #[test]
    fn shifts_and_stretches_times() {
        assert_eq!(cues(VTT), vec![(1.0, 3.5), (3600.25, 3602.0)]);
        let s = shift_vtt(VTT, Timing { offset_ms: 2500, scale: 1.0 });
        assert!(s.contains("00:00:03.500 --> 00:00:06.000\nHello"), "{s}");
        assert!(s.contains("01:00:02.750 --> 01:00:04.500 line:90%\nWorld"), "{s}");
        // Never before the start.
        assert!(shift_vtt(VTT, Timing { offset_ms: -5000, scale: 1.0 }).contains("00:00:00.000 --> 00:00:00.000\nHello"));
        assert!(parse_ts("02:03.5") == Some(123.5) && parse_ts("00:00:01,250") == Some(1.25));
    }

    /// A made-up film: speech at the cue times, shifted by `late` seconds, with some stray noise.
    fn curve_for(cues: &[(f64, f64)], late: f64, length_s: f64) -> Vec<f32> {
        let n = (length_s / BIN_S) as usize;
        let mut c = vec![-60.0f32; n];
        for (s, e) in cues {
            for i in (((s + late) / BIN_S) as usize)..(((e + late) / BIN_S) as usize).min(n) { c[i] = -20.0; }
        }
        // Noise bursts that match no line.
        for i in (0..n).step_by(997) { for j in i..(i + 30).min(n) { c[j] = -25.0; } }
        c
    }

    fn lines() -> Vec<(f64, f64)> {
        // Irregular lines over 20 minutes.
        let mut v = vec![];
        let mut t = 20.0;
        let mut x = 7u64;
        while t < 1150.0 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let len = 1.0 + ((x >> 33) % 400) as f64 / 100.0;
            let gap = 1.0 + ((x >> 13) % 900) as f64 / 100.0;
            v.push((t, t + len));
            t += len + gap;
        }
        v
    }

    #[test]
    fn finds_the_shift_of_a_subtitle() {
        let cues = lines();
        // The speech is 12.35 s later than the subtitle says.
        let sig = speech_signal(&curve_for(&cues, 12.35, 1200.0));
        let (timing, z) = find_timing(&cues, &sig).expect("sure enough");
        assert_eq!(timing.scale, 1.0);
        assert!((timing.offset_ms - 12350).abs() <= 100, "{timing:?}");
        assert!(z >= MIN_CONFIDENCE);
        // Already right: nothing to move.
        let sig = speech_signal(&curve_for(&cues, 0.0, 1200.0));
        assert!(find_timing(&cues, &sig).unwrap().0.offset_ms.abs() <= 100);
    }

    #[test]
    fn finds_a_frame_rate_stretch() {
        let cues = lines();
        // Subtitle made for 23.976 fps, the film runs at 25: the speech comes earlier and earlier.
        let real: Vec<(f64, f64)> = cues.iter().map(|(s, e)| (s * 23.976 / 25.0, e * 23.976 / 25.0)).collect();
        let sig = speech_signal(&curve_for(&real, 0.0, 1200.0));
        let (timing, _) = find_timing(&cues, &sig).expect("sure enough");
        assert!((timing.scale - 23.976 / 25.0).abs() < 1e-6, "{timing:?}");
        assert!(timing.offset_ms.abs() <= 100, "{timing:?}");
    }

    #[test]
    fn silence_is_no_answer() {
        let cues = lines();
        let sig = speech_signal(&vec![-50.0f32; 24000]);
        assert!(find_timing(&cues, &sig).is_none());
    }
}
