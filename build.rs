// Lets `cargo build` succeed while the server is running from target\<profile>.
//
// Windows won't let cargo delete a program that is running, so the build used to
// stop with "failed to remove file …\mediaserver.exe (Access denied)". Windows
// does allow *renaming* a running program, so before the new one is written the
// running one is moved aside to mediaserver.exe.old (it keeps running; the next
// start uses the new build). Old copies that are no longer running are deleted.
// Nothing happens on other systems (Docker / Synology build).

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=web");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(dir) = profile_dir() else { return };
    let exe = dir.join("mediaserver.exe");
    // Leftovers from earlier builds, if they aren't running any more.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("mediaserver.exe.old") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if !exe.is_file() || !in_use(&exe) {
        return;
    }
    let mut dest = dir.join("mediaserver.exe.old");
    let mut n = 1;
    while dest.exists() {
        dest = dir.join(format!("mediaserver.exe.old{n}"));
        n += 1;
    }
    match std::fs::rename(&exe, &dest) {
        Ok(()) => println!("cargo:warning=mediaserver.exe is running; it was moved to {} so the new build can be written (restart the server to use it)", dest.display()),
        Err(e) => println!("cargo:warning=mediaserver.exe is running and could not be moved aside ({e}); stop the server and build again"),
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
