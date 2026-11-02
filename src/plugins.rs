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
//! server.started, api.call (answer any JSON; reached through
//! POST /api/plugins/{id}/call/{action}).

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
        self.log.push_back(json!({"at": crate::db::now(), "plugin": plugin, "event": event, "ok": ok, "detail": detail}));
        while self.log.len() > 300 {
            self.log.pop_front();
        }
    }
}

/// Runs one plugin for one event and returns its JSON answer (or Null).
pub fn invoke(plugin: &Plugin, settings: &serde_json::Map<String, Value>, event: &str, data: &Value, server: &Value) -> Result<Value, String> {
    let mut merged = plugin.manifest.settings.clone();
    for (k, v) in settings {
        merged.insert(k.clone(), v.clone());
    }
    let input = json!({"event": event, "data": data, "settings": merged, "server": server});
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
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.to_string().as_bytes());
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
    PathBuf::from(prog)
}

/// A tiny plugin built into the server, used as a working example and by
/// the self-test: `lichtspiel --example-plugin`. It answers metadata
/// lookups with a tagline and echoes api.call requests.
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
        "api.call" => json!({"ok": true, "action": v["data"]["action"], "echo": v["data"]["body"]}),
        _ => json!({"ok": true}),
    };
    println!("{answer}");
}
