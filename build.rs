// Lets `cargo build` succeed while the server is running from target\<profile>.
//
// Windows won't let cargo delete a program that is running, so the build used to
// stop with "failed to remove file …\lichtspiel-server.exe (Access denied)". Windows
// does allow *renaming* a running program, so before the new one is written the
// running one is moved aside to lichtspiel-server.exe.old (it keeps running; the next
// start uses the new build). Old copies that are no longer running are deleted.
// Nothing happens on other systems (Docker / Synology build).

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=web");
    println!("cargo:rerun-if-changed=.git/logs/HEAD");
    println!("cargo:rerun-if-env-changed=GIT_SHA");
    // The commit the build is made from: GIT_SHA (the Docker build passes it, there is no .git
    // there), else git itself; "+" marks changes that are not committed yet.
    let git = |args: &[&str]| {
        std::process::Command::new("git").args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let build = match std::env::var("GIT_SHA").ok().filter(|s| !s.trim().is_empty()) {
        Some(s) => s.trim().chars().take(8).collect::<String>(),
        None => {
            let sha = git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_default();
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
            if sha.is_empty() { "unknown".to_string() } else if dirty { format!("{sha}+") } else { sha }
        }
    };
    println!("cargo:rustc-env=LICHTSPIEL_BUILD={build}");
    let built = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("cargo:rustc-env=LICHTSPIEL_BUILT_AT={built}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(dir) = profile_dir() else { return };
    let exe = dir.join("lichtspiel-server.exe");
    // Leftovers from earlier builds, if they aren't running any more.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("lichtspiel-server.exe.old") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if !exe.is_file() || !in_use(&exe) {
        return;
    }
    let mut dest = dir.join("lichtspiel-server.exe.old");
    let mut n = 1;
    while dest.exists() {
        dest = dir.join(format!("lichtspiel-server.exe.old{n}"));
        n += 1;
    }
    match std::fs::rename(&exe, &dest) {
        Ok(()) => println!("cargo:warning=lichtspiel-server.exe is running; it was moved to {} so the new build can be written (restart the server to use it)", dest.display()),
        Err(e) => println!("cargo:warning=lichtspiel-server.exe is running and could not be moved aside ({e}); stop the server and build again"),
    }
}

/// target\<profile> (OUT_DIR is target\<profile>\build\<package>-<hash>\out).
fn profile_dir() -> Option<PathBuf> {
    let out = PathBuf::from(std::env::var_os("OUT_DIR")?);
    let dir = out.ancestors().nth(3)?.to_path_buf();
    dir.is_dir().then_some(dir)
}

/// A running program can't be opened for writing.
fn in_use(p: &Path) -> bool {
    std::fs::OpenOptions::new().write(true).open(p).is_err()
}
