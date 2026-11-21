//! Folder browsing for picking library folders.

use super::*;

pub fn browse(path: &str) -> Resp {
    let path = path.trim();
    if path.is_empty() {
        if cfg!(windows) {
            let drives: Vec<Value> = (b'C'..=b'Z')
                .map(|c| format!("{}:\\", c as char))
                .filter(|d| Path::new(d).exists())
                .map(|d| json!({"name": d, "path": d}))
                .collect();
            return ok(json!({"path": "", "parent": null, "dirs": drives, "videos_here": 0}));
        }
        return list_dir(Path::new("/"));
    }
    list_dir(Path::new(path))
}

fn list_dir(p: &Path) -> Resp {
    let Ok(rd) = std::fs::read_dir(p) else { return fail(404, "That folder can't be opened") };
    let mut dirs: Vec<(String, String)> = vec![];
    let mut videos = 0;
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !name.starts_with('.') && !name.starts_with('@') && !name.starts_with('#') && name != "$RECYCLE.BIN" && name != "System Volume Information" {
                dirs.push((name, path.to_string_lossy().to_string()));
            }
        } else if scanner::is_video(&path) {
            videos += 1;
        }
    }
    dirs.sort_by_key(|d| d.0.to_lowercase());
    let parent = p.parent().map(|x| x.to_string_lossy().to_string()).filter(|s| !s.is_empty());
    let parent = if cfg!(windows) && parent.is_none() { Some(String::new()) } else { parent };
    ok(json!({"path": p.to_string_lossy(), "parent": parent,
        "dirs": dirs.iter().map(|(n, pth)| json!({"name": n, "path": pth})).collect::<Vec<_>>(), "videos_here": videos}))
}
