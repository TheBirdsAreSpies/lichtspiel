//! ffprobe: container, duration, streams, tags and chapters of a file.

use super::*;

/// Reads container, duration and every stream with ffprobe. Returns false
/// when ffprobe is missing or cannot read the file.
pub fn probe(ffprobe: &str, file: &mut MediaFile) -> bool {
    probe_tags(ffprobe, file).is_some()
}

/// Like [`probe`], and gives back the file's tags (title, artist, album …) too.
pub fn probe_tags(ffprobe: &str, file: &mut MediaFile) -> Option<Value> {
    let out = Command::new(ffprobe)
        .args(["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams", "-show_chapters"])
        .arg(&file.path)
        .output();
    let Ok(out) = out else { return None };
    if !out.status.success() {
        return None;
    }
    let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) else {
        return None;
    };
    let fmt = &v["format"];
    file.format_name = fmt["format_name"].as_str().unwrap_or("").to_string();
    file.duration_ms = fmt["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .map(|d| (d * 1000.0) as u64)
        .unwrap_or(0);
    file.bitrate = fmt["bit_rate"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0);
    file.streams.clear();
    if let Some(streams) = v["streams"].as_array() {
        for s in streams {
            let kind = s["codec_type"].as_str().unwrap_or("");
            let kind = match kind {
                "video" if s["disposition"]["attached_pic"].as_i64() == Some(1) => continue,
                "video" | "audio" | "subtitle" => kind,
                _ => continue,
            };
            let pix = s["pix_fmt"].as_str().unwrap_or("");
            let bit_depth = s["bits_per_raw_sample"]
                .as_str()
                .and_then(|b| b.parse().ok())
                .unwrap_or(if pix.contains("10") {
                    10
                } else if pix.contains("12") {
                    12
                } else if kind == "video" {
                    8
                } else {
                    0
                });
            file.streams.push(Stream {
                kind: kind.into(),
                index: s["index"].as_u64().unwrap_or(0) as u32,
                codec: s["codec_name"].as_str().unwrap_or("").to_string(),
                language: s["tags"]["language"].as_str().unwrap_or("").to_string(),
                channels: s["channels"].as_u64().unwrap_or(0) as u32,
                width: s["width"].as_u64().unwrap_or(0) as u32,
                height: s["height"].as_u64().unwrap_or(0) as u32,
                bit_depth,
                default: s["disposition"]["default"].as_i64() == Some(1),
                title: s["tags"]["title"].as_str().unwrap_or("").to_string(),
                forced: subtitle_flags(s).0,
                sdh: subtitle_flags(s).1,
                profile: s["profile"].as_str().unwrap_or("").to_string(),
                color_transfer: s["color_transfer"].as_str().unwrap_or("").to_string(),
                hdr10plus: false,
                dovi: s["side_data_list"].to_string().contains("DOVI")
                    || matches!(s["codec_tag_string"].as_str().unwrap_or(""), "dvhe" | "dvh1" | "dav1" | "dva1" | "dvav"),
                channel_layout: s["channel_layout"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    // Credits markers: a
    // chapter whose name says credits marks where they start.
    file.credits_ms = v["chapters"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|c| is_credits_title(c["tags"]["title"].as_str().unwrap_or("")))
                .filter_map(|c| c["start_time"].as_str().and_then(|t| t.parse::<f64>().ok()))
                .map(|t| (t * 1000.0) as u64)
                .collect()
        })
        .unwrap_or_default();
    // HDR10+ metadata sits in the frames, not the stream header: look at the
    // first frame of PQ video only.
    if let Some(v) = file.streams.iter_mut().find(|s| s.kind == "video" && s.color_transfer == "smpte2084") {
        let frames = Command::new(ffprobe)
            .args(["-v", "quiet", "-print_format", "json", "-select_streams", "v:0", "-read_intervals", "%+#1", "-show_frames", "-show_entries", "frame=side_data_list"])
            .arg(&file.path)
            .output();
        if let Ok(o) = frames {
            v.hdr10plus = String::from_utf8_lossy(&o.stdout).contains("2094-40");
        }
    }
    file.probed = true;
    file.probe_version = crate::data::db::PROBE_VERSION;
    Some(crate::library::music::tags_of(&v))
}

/// Every chapter of a file: (start ms, end ms, title). Untitled ones are named "Chapter N".
pub fn read_chapters(ffprobe: &str, path: &str) -> Option<Vec<(u64, u64, String)>> {
    let out = Command::new(ffprobe).args(["-v", "quiet", "-print_format", "json", "-show_chapters"]).arg(path).output().ok()?;
    if !out.status.success() { return None; }
    Some(parse_chapters(&serde_json::from_slice::<Value>(&out.stdout).ok()?))
}

/// (forced, for the hard of hearing) of a subtitle stream, from its disposition or its title.
pub fn subtitle_flags(s: &Value) -> (bool, bool) {
    if s["codec_type"].as_str() != Some("subtitle") { return (false, false); }
    let title = s["tags"]["title"].as_str().unwrap_or("").to_lowercase();
    let word = |w: &str| title.split(|c: char| !c.is_alphanumeric()).any(|x| x == w);
    let forced = s["disposition"]["forced"].as_i64() == Some(1) || title.contains("forced") || title.contains("erzwungen") || title.contains("forcé");
    let sdh = s["disposition"]["hearing_impaired"].as_i64() == Some(1) || word("sdh") || word("cc") || title.contains("hearing") || title.contains("schwerhörig") || title.contains("hörgeschädigt");
    (forced, sdh)
}

pub fn parse_chapters(v: &Value) -> Vec<(u64, u64, String)> {
    let secs = |c: &Value, k: &str| c[k].as_str().and_then(|t| t.parse::<f64>().ok()).map(|t| (t * 1000.0).max(0.0) as u64);
    v["chapters"].as_array().map(|a| a.iter().enumerate().filter_map(|(i, c)| {
        let (s, e) = (secs(c, "start_time")?, secs(c, "end_time").unwrap_or(0));
        let title = c["tags"]["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from).unwrap_or_else(|| format!("Chapter {}", i + 1));
        Some((s, e.max(s), title))
    }).collect()).unwrap_or_default()
}
