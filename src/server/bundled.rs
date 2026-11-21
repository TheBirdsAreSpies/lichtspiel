//! The plugins shipped with the server: copied into the data folder on first start, updated when a newer version ships.

use super::*;

/// On first start, copies the plugins shipped with the server (switched off)
/// into the data folder, so they appear on the Plugins page.
pub(super) fn install_bundled_plugins(dest: &Path) {
    let mut sources: Vec<PathBuf> = vec![];
    if let Some(p) = crate::env("BUNDLED_PLUGINS") {
        sources.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        sources.push(dir.join("plugins"));
        sources.push(dir.join("..").join("..").join("plugins")); // cargo's target/debug next to the project
    }
    sources.push(PathBuf::from("/opt/lichtspiel/plugins"));
    sources.push(PathBuf::from("/opt/mediaserver/plugins"));
    let Some(src) = sources.into_iter().find(|s| s.is_dir() && s.canonicalize().ok() != dest.canonicalize().ok()) else { return };
    copy_bundled_plugins(&src, dest);
}


/// `a` is a higher dotted version number than `b` ("1.10" > "1.9").
fn newer_version(a: &str, b: &str) -> bool {
    let nums = |v: &str| v.trim().split('.').map(|n| n.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (nums(a), nums(b));
    (0..a.len().max(b.len())).map(|i| (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0))).find(|(x, y)| x != y).is_some_and(|(x, y)| x > y)
}


/// The "version" in a plugin folder's plugin.json ("" when there is none).
fn plugin_version(dir: &Path) -> String {
    std::fs::read(dir.join("plugin.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["version"].as_str().map(String::from)).unwrap_or_default()
}


fn copy_bundled_plugins(src: &Path, dest: &Path) {
    // The marker lists the bundled plugins copied so far (one per line), so a
    // plugin bundled by a newer version arrives once and one deleted by hand
    // stays deleted. Older versions wrote one sentence instead: they had copied
    // the first two.
    let marker = dest.join(".bundled-installed");
    let mut done: Vec<String> = match std::fs::read_to_string(&marker) {
        Ok(t) if t.starts_with("bundled example plugins") => vec!["nfo-reader".into(), "webhook-notify".into()],
        Ok(t) => t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        Err(_) => vec![],
    };
    let Ok(rd) = std::fs::read_dir(src) else { return };
    for e in rd.flatten() {
        let from = e.path();
        if !from.join("plugin.json").exists() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let to = dest.join(e.file_name());
        let seen = done.contains(&name);
        if !seen { done.push(name.clone()); }
        // A newer bundled version replaces the plugin's own program files and plugin.json, never its
        // other files (its tokens and settings) or its tests; a plugin deleted by hand stays deleted.
        if to.join("plugin.json").is_file() && newer_version(&plugin_version(&from), &plugin_version(&to)) {
            for f in std::fs::read_dir(&from).into_iter().flatten().flatten() {
                let n = f.file_name().to_string_lossy().to_string();
                if n == "plugin.json" || (n.ends_with(".py") && !n.starts_with("test_")) {
                    let _ = std::fs::copy(f.path(), to.join(&n));
                }
            }
            crate::admin::log::add(format!("Plugin {name} updated to version {}", plugin_version(&to)));
            continue;
        }
        if seen || to.exists() {
            continue;
        }
        let _ = std::fs::create_dir_all(&to);
        if let Ok(files) = std::fs::read_dir(&from) {
            for f in files.flatten().filter(|f| f.path().is_file()) {
                let _ = std::fs::copy(f.path(), to.join(f.file_name()));
            }
        }
    }
    done.sort();
    let _ = std::fs::write(marker, done.join("\n") + "\n");
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_plugin_json_can_be_read() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let mf = e.path().join("plugin.json");
            let Ok(text) = std::fs::read_to_string(&mf) else { continue };
            if let Err(err) = serde_json::from_str::<crate::admin::plugins::Manifest>(&text) {
                panic!("{}: {err}", mf.display());
            }
        }
    }

    #[test]
    fn a_newer_bundled_plugin_replaces_only_its_own_files() {
        let root = std::env::temp_dir().join(format!("lichtspiel-bundled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, dest) = (root.join("bundled"), root.join("plugins"));
        let plugin = |dir: &Path, version: &str, code: &str| {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("plugin.json"), format!(r#"{{"id": "x", "version": "{version}"}}"#)).unwrap();
            std::fs::write(dir.join("main.py"), code).unwrap();
            std::fs::write(dir.join("test_main.py"), code).unwrap();
        };
        plugin(&src.join("trakt"), "1.10", "new");
        plugin(&src.join("gone"), "2.0", "new");
        plugin(&dest.join("trakt"), "1.9", "old");
        std::fs::write(dest.join("trakt/tokens.json"), "mine").unwrap();
        std::fs::write(dest.join(".bundled-installed"), "trakt\ngone\n").unwrap();
        copy_bundled_plugins(&src, &dest);
        let read = |p: &str| std::fs::read_to_string(dest.join(p)).unwrap_or_default();
        assert_eq!(read("trakt/main.py"), "new", "the newer bundled version was not installed");
        assert!(read("trakt/plugin.json").contains("1.10"));
        assert_eq!(read("trakt/tokens.json"), "mine", "the plugin's own data was touched");
        assert_eq!(read("trakt/test_main.py"), "old", "a test file was copied over");
        assert!(!dest.join("gone").exists(), "a plugin deleted by hand came back");
        // The same or an older bundled version changes nothing.
        std::fs::write(dest.join("trakt/main.py"), "edited").unwrap();
        copy_bundled_plugins(&src, &dest);
        assert_eq!(read("trakt/main.py"), "edited");
        assert!(newer_version("1.10", "1.9") && newer_version("1.1", "1") && !newer_version("1.1", "1.1") && !newer_version("1.0", "1.1") && !newer_version("", "1.0"));
        let _ = std::fs::remove_dir_all(&root);
    }

}
