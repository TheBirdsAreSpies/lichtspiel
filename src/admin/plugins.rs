//! Plugins ("mods"). Each plugin is a folder inside data/plugins with a
//! plugin.json file. The server runs the plugin's command for the events it
//! listens to, sends the event as JSON on standard input and reads an
//! optional JSON answer from standard output. Any language works (Python,
//! shell, Node, a compiled program …).
//!
//! plugin.json:
//! {
//!   "id": "nfo-reader", "name": "NFO reader", "version": "1.0",
//!   "description": "Reads .nfo files", "command": ["python3", "main.py"],
//!   "hooks": ["metadata.lookup", "library.item.added"],
//!   "settings": {"overwrite": false}, "timeout_seconds": 20
//! }
//!
//! Events: metadata.lookup (answer {"metadata": {...}} to fill in details),
//! library.item.added, library.item.removed, library.scan.finished,
//! playback.started, playback.progress, playback.stopped, playback.watched,
//! item.problem, download.ready, user.invited, user.joined, playlist.created, playlist.updated,
//! server.started, schedule.daily (once a day), api.call (answer any JSON; reached through
//! POST /api/plugins/{id}/call/{action}). An api.call answer with
//! {"mark_watched": [...]} marks those items watched for the caller.
//!
//! Playback events carry "item" (kind, title, year, ids, show, season,
//! episode), "time" (ms) and "progress" (percent); playback.progress is sent
//! when a player pauses or plays on.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const EVENTS: &[&str] = &[
    "metadata.lookup",
    "library.item.added",
    "library.item.removed",
    "library.scan.finished",
    "playback.started",
    "playback.progress",
    "playback.stopped",
    "playback.watched",
    "item.problem",
    "download.ready",
    "user.invited",
    "user.joined",
    "playlist.created",
    "playlist.updated",
    "server.started",
    "schedule.daily",
    "api.call",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub settings: serde_json::Map<String, Value>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub enabled_by_default: bool,
    /// Each person gets a panel of this plugin in Settings (connect an
    /// account …); the plugin draws it when asked with api.call "panel".
    #[serde(default)]
    pub user_panel: bool,
    /// The panel is shown to admins only (a report about the library …).
    #[serde(default)]
    pub panel_admins_only: bool,
    /// The plugin may read the whole library: it gets `server.library`, an
    /// address that lists every movie and episode with its files.
    #[serde(default)]
    pub reads_library: bool,
    /// The plugin sends e-mail: it gets `server.smtp`, the mail server from Settings → General
    /// ({host, port, security, user, password, from}), when one is set.
    #[serde(default)]
    pub sends_mail: bool,
    /// Where the panel is shown: "settings" (Settings → Account, the default), "sidebar" (a page of
    /// its own with an entry in the left menu) or "both". An admin can choose otherwise.
    #[serde(default)]
    pub placement: String,
}

pub const PLACEMENTS: &[&str] = &["settings", "sidebar", "both"];

/// Where a plugin's panel goes: the admin's choice, else the plugin's, else Settings.
pub fn placement<'a>(manifest: &'a Manifest, chosen: &'a str) -> &'a str {
    [chosen, manifest.placement.as_str()].into_iter().find(|p| PLACEMENTS.contains(p)).unwrap_or("settings")
}

/// At most this many plugin programs run at once; the others wait for their turn.
pub const MAX_RUNNING: usize = 4;
static RUNNING: (std::sync::Mutex<usize>, std::sync::Condvar) = (std::sync::Mutex::new(0), std::sync::Condvar::new());

/// One of the MAX_RUNNING places, given back when dropped.
struct Turn;
impl Turn {
    fn wait() -> Turn {
        let mut n = RUNNING.0.lock().unwrap();
        while *n >= MAX_RUNNING { n = RUNNING.1.wait(n).unwrap(); }
        *n += 1;
        Turn
    }
}
impl Drop for Turn {
    fn drop(&mut self) {
        *RUNNING.0.lock().unwrap() -= 1;
        RUNNING.1.notify_one();
    }
}

fn default_timeout() -> u64 {
    20
}

#[derive(Clone, Debug)]
pub struct Plugin {
    pub manifest: Manifest,
    pub dir: PathBuf,
}

#[derive(Default)]
pub struct Plugins {
    pub dir: PathBuf,
    pub list: Vec<Plugin>,
    pub errors: Vec<String>,
    pub log: std::collections::VecDeque<Value>,
}

impl Plugins {
    pub fn load(dir: &Path) -> Plugins {
        let mut p = Plugins { dir: dir.to_path_buf(), ..Default::default() };
        p.reload();
        p
    }

    pub fn reload(&mut self) {
        self.list.clear();
        self.errors.clear();
        let _ = std::fs::create_dir_all(&self.dir);
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return };
        let mut dirs: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).collect();
        dirs.sort_by_key(|e| e.file_name());
        for e in dirs {
            let mf = e.path().join("plugin.json");
            let Ok(text) = std::fs::read_to_string(&mf) else { continue };
            match serde_json::from_str::<Manifest>(&text) {
                Ok(mut m) => {
                    if m.id.is_empty() {
                        m.id = e.file_name().to_string_lossy().to_string();
                    }
                    if m.command.is_empty() {
                        self.errors.push(format!("{}: plugin.json has no command", m.id));
                        continue;
                    }
                    self.list.push(Plugin { manifest: m, dir: e.path() });
                }
                Err(err) => self.errors.push(format!("{}: {err}", mf.display())),
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Plugin> {
        self.list.iter().find(|p| p.manifest.id == id)
    }

    pub fn note(&mut self, plugin: &str, event: &str, ok: bool, detail: String) {
        self.log.push_back(json!({"at": crate::data::db::now(), "plugin": plugin, "event": event, "ok": ok, "detail": detail}));
        while self.log.len() > 300 {
            self.log.pop_front();
        }
    }
}

/// What `Plugins::install_zip` installed.
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    /// A plugin of that id (or its folder) was there and got the new files.
    pub replaced: bool,
    pub files: usize,
}

pub enum InstallError {
    /// The plugin is there already and replacing it was not asked for.
    Exists { id: String, name: String, version: String },
    Bad(String),
}

/// A plugin id that can be a folder name everywhere: letters, digits, `-`, `_` and `.`, not starting with a dot.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && !id.starts_with('.') && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl Plugins {
    /// Installs a plugin from a .zip (uploaded on the Plugins page). The zip holds plugin.json at
    /// the top or in one folder that holds everything (as GitHub's "Download ZIP" makes it). The
    /// plugin goes into a folder named after its id. A plugin that is there already is replaced
    /// only when `replace` is set: the files in the zip are written over its own, and the files
    /// the zip does not have (its tokens, a cache) stay. Afterwards the plugins are read again.
    pub fn install_zip(&mut self, zip: &[u8], replace: bool) -> Result<Installed, InstallError> {
        use InstallError::Bad;
        let entries: Vec<_> = crate::admin::unzip::read(zip).map_err(Bad)?.into_iter()
            // What macOS adds to a zip it makes.
            .filter(|e| !e.name.starts_with("__MACOSX/") && !e.name.ends_with(".DS_Store"))
            .collect();
        let prefix = if entries.iter().any(|e| e.name == "plugin.json") {
            String::new()
        } else {
            match entries.first().and_then(|e| e.name.split_once('/')).map(|(top, _)| format!("{top}/")) {
                Some(top) if entries.iter().all(|e| e.name.starts_with(&top)) && entries.iter().any(|e| e.name == format!("{top}plugin.json")) => top,
                _ => return Err(Bad("The zip has no plugin.json (at the top, or in one folder that holds everything)".into())),
            }
        };
        let manifest = entries.iter().find(|e| e.name == format!("{prefix}plugin.json")).map(|e| e.data.as_slice()).unwrap_or_default();
        let mut m: Manifest = serde_json::from_slice(manifest).map_err(|e| Bad(format!("plugin.json: {e}")))?;
        if m.command.is_empty() {
            return Err(Bad("plugin.json has no command".into()));
        }
        if m.id.is_empty() {
            m.id = prefix.trim_end_matches('/').to_string();
        }
        if !valid_id(&m.id) {
            return Err(Bad(format!("The plugin id “{}” cannot be a folder name: use letters, digits, - _ and . only", m.id)));
        }
        let existing = self.get(&m.id).cloned();
        let dir = existing.as_ref().map(|p| p.dir.clone()).unwrap_or_else(|| self.dir.join(&m.id));
        let replaced = existing.is_some() || dir.exists();
        if replaced && !replace {
            let (name, version) = existing.map(|p| (p.manifest.name, p.manifest.version)).unwrap_or_default();
            return Err(InstallError::Exists { id: m.id, name, version });
        }
        let _ = std::fs::create_dir_all(&dir);
        let root = dir.canonicalize().map_err(|e| Bad(format!("The plugin folder could not be made: {e}")))?;
        for e in &entries {
            let to = dir.join(&e.name[prefix.len()..]);
            let parent = to.parent().unwrap_or(&dir);
            std::fs::create_dir_all(parent).map_err(|x| Bad(format!("{}: {x}", e.name)))?;
            // A link inside the plugin's folder must not lead the files somewhere else.
            if !parent.canonicalize().is_ok_and(|p| p.starts_with(&root)) {
                return Err(Bad(format!("{}: the folder leads outside the plugin's folder", e.name)));
            }
            std::fs::write(&to, &e.data).map_err(|x| Bad(format!("{} could not be saved: {x}", e.name)))?;
            #[cfg(unix)]
            if e.executable {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755));
            }
        }
        self.reload();
        crate::admin::log::add(format!("Plugin {} {} {} from an uploaded zip", m.id, if replaced { "replaced with version" } else { "installed, version" }, m.version));
        Ok(Installed { id: m.id, name: m.name, version: m.version, replaced, files: entries.len() })
    }
}

/// Runs one plugin for one event and returns its JSON answer (or Null).
pub fn invoke(plugin: &Plugin, settings: &serde_json::Map<String, Value>, event: &str, data: &Value, server: &Value) -> Result<Value, String> {
    let mut merged = plugin.manifest.settings.clone();
    for (k, v) in settings {
        merged.insert(k.clone(), v.clone());
    }
    let input = json!({"event": event, "data": data, "settings": merged, "server": server});
    let _turn = Turn::wait();
    let cmd = &plugin.manifest.command;
    let program = resolve_program(&plugin.dir, &cmd[0]);
    let mut child = Command::new(&program)
        .args(&cmd[1..])
        .current_dir(&plugin.dir)
        .env("LICHTSPIEL_EVENT", event)
        .env("MEDIASERVER_EVENT", event)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))?;
    // Write the input on its own thread: a plugin that never reads it would block us before the timeout.
    if let Some(mut stdin) = child.stdin.take() {
        let bytes = input.to_string().into_bytes();
        std::thread::spawn(move || { let _ = stdin.write_all(&bytes); });
    }
    // Read both outputs on their own threads so a chatty plugin can't block.
    let reader = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out_t = reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let err_t = reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let timeout = Duration::from_secs(plugin.manifest.timeout_seconds.clamp(1, 600));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("took longer than {} s and was stopped", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let out_bytes = out_t.join().unwrap_or_default();
    let err_bytes = err_t.join().unwrap_or_default();
    let stdout = String::from_utf8_lossy(&out_bytes);
    if !status.success() {
        let err = String::from_utf8_lossy(&err_bytes);
        return Err(format!("exit {status}: {}", err.trim()));
    }
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    // Use the last line that is JSON, so plugins may print other lines too.
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Ok(v);
    }
    for line in trimmed.lines().rev() {
        if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
            return Ok(v);
        }
    }
    Ok(json!({"output": trimmed}))
}

fn resolve_program(dir: &Path, prog: &str) -> PathBuf {
    if prog == "{server}" {
        return std::env::current_exe().unwrap_or_else(|_| PathBuf::from(prog));
    }
    let local = dir.join(prog);
    if (prog.starts_with("./") || prog.starts_with(".\\") || prog.contains('/')) && local.exists() {
        return local;
    }
    if let Some(p) = windows_python(prog) { return p; }
    PathBuf::from(prog)
}

/// On Windows "python3" is usually only the Microsoft Store stub ("Python was not found"); look for
/// a real Python in the search path (skipping the WindowsApps stubs), then for the "py" launcher.
fn windows_python(prog: &str) -> Option<PathBuf> {
    if !cfg!(windows) || !matches!(prog, "python3" | "python") { return None; }
    let path = std::env::var_os("PATH")?;
    let find = |name: &str| std::env::split_paths(&path)
        .filter(|d| !d.to_string_lossy().to_lowercase().contains("windowsapps"))
        .map(|d| d.join(name)).find(|f| f.is_file());
    find("python.exe").or_else(|| find("py.exe"))
}

/// A tiny plugin built into the server, used as a working example and by
/// the self-test: `lichtspiel --example-plugin`. It answers metadata
/// lookups with a tagline, echoes api.call requests ("mark" answers
/// mark_watched for body.id, "panel" draws a Settings panel) and writes the
/// last playback event to last_playback.json in its folder.
pub fn example_plugin_main() {
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let v: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let event = v["event"].as_str().unwrap_or("");
    let prefix = v["settings"]["tagline_prefix"].as_str().unwrap_or("Added by the example plugin");
    let answer = match event {
        "metadata.lookup" => {
            let title = v["data"]["title"].as_str().unwrap_or("");
            json!({"metadata": {"tagline": format!("{prefix}: {title}"), "extra": {"example_plugin": true}}})
        }
        "api.call" if v["data"]["action"] == "mark" => json!({"ok": true, "mark_watched": [{"id": v["data"]["body"]["id"]}]}),
        "api.call" if v["data"]["action"] == "panel" => json!({"title": "Example", "text": "Hello from the example plugin", "buttons": []}),
        "api.call" => json!({"ok": true, "action": v["data"]["action"], "echo": v["data"]["body"]}),
        e if e.starts_with("playback.") => {
            let _ = std::fs::write("last_playback.json", v.to_string());
            json!({"ok": true})
        }
        _ => json!({"ok": true}),
    };
    println!("{answer}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn timeout_applies_when_plugin_never_reads_input() {
        let m: Manifest = serde_json::from_value(json!({"id": "deaf", "name": "deaf", "command": ["sh", "-c", "sleep 30"], "timeout_seconds": 1})).unwrap();
        let p = Plugin { manifest: m, dir: std::env::temp_dir() };
        // Much bigger than a pipe buffer, so writing it blocks while nobody reads.
        let data = json!({"body": "x".repeat(400_000)});
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || { let _ = tx.send(invoke(&p, &serde_json::Map::new(), "api.call", &data, &Value::Null)); });
        let r = rx.recv_timeout(Duration::from_secs(5)).expect("the plugin call did not come back within its timeout");
        assert!(r.unwrap_err().contains("took longer"));
    }

    #[test]
    fn the_admins_placement_wins_over_the_plugins() {
        let m = |place: &str| -> Manifest { serde_json::from_value(json!({"id": "p", "name": "p", "command": ["x"], "placement": place})).unwrap() };
        assert_eq!(placement(&m(""), ""), "settings");
        assert_eq!(placement(&m("sidebar"), ""), "sidebar");
        assert_eq!(placement(&m("nonsense"), ""), "settings");
        assert_eq!(placement(&m("sidebar"), "both"), "both");
        assert_eq!(placement(&m("sidebar"), "settings"), "settings");
        assert_eq!(placement(&m("both"), "nonsense"), "both");
    }

    #[test]
    fn a_plugin_comes_in_from_a_zip() {
        let root = std::env::temp_dir().join(format!("lichtspiel-upload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut p = Plugins::load(&root);
        let zip = |version: &str, code: &str| crate::admin::unzip::tests::make(&[
            ("hello-main/plugin.json", format!(r#"{{"id": "hello", "name": "Hello", "version": "{version}", "command": ["python3", "main.py"]}}"#).as_bytes(), true),
            ("hello-main/main.py", code.as_bytes(), true),
            ("hello-main/lib/util.py", b"x = 1", false),
            ("__MACOSX/hello-main/._main.py", b"junk", false),
        ]);
        let got = p.install_zip(&zip("1.0", "old"), false).ok().expect("the plugin was not installed");
        assert_eq!((got.id.as_str(), got.replaced, got.files), ("hello", false, 3));
        assert_eq!(std::fs::read_to_string(root.join("hello/lib/util.py")).unwrap(), "x = 1");
        assert!(!root.join("__MACOSX").exists() && !root.join("hello-main").exists());
        assert_eq!(p.get("hello").map(|x| x.manifest.version.as_str()), Some("1.0"));
        // Again: only with replace, and the plugin's own files stay.
        std::fs::write(root.join("hello/tokens.json"), "mine").unwrap();
        assert!(matches!(p.install_zip(&zip("2.0", "new"), false), Err(InstallError::Exists { ref version, .. }) if version == "1.0"));
        assert_eq!(std::fs::read_to_string(root.join("hello/main.py")).unwrap(), "old");
        assert!(p.install_zip(&zip("2.0", "new"), true).ok().is_some_and(|i| i.replaced));
        assert_eq!(std::fs::read_to_string(root.join("hello/main.py")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(root.join("hello/tokens.json")).unwrap(), "mine");
        assert_eq!(p.get("hello").map(|x| x.manifest.version.as_str()), Some("2.0"));
        // Without plugin.json, without a command, or with an id that is no folder name: nothing is written.
        let mut bad = |files: &[(&str, &[u8], bool)]| match p.install_zip(&crate::admin::unzip::tests::make(files), true) { Err(InstallError::Bad(e)) => e, _ => panic!("accepted") };
        assert!(bad(&[("main.py", b"", false)]).contains("no plugin.json"));
        assert!(bad(&[("plugin.json", br#"{"id": "x", "name": "x", "command": []}"#, false)]).contains("no command"));
        assert!(bad(&[("plugin.json", br#"{"id": "../x", "name": "x", "command": ["a"]}"#, false)]).contains("folder name"));
        assert!(bad(&[("a/plugin.json", b"{}", false), ("b/main.py", b"", false)]).contains("no plugin.json"));
        assert!(!root.join("x").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
