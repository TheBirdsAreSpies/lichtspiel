//! Reads a .zip file in memory: enough for plugins uploaded on the Plugins page (stored and
//! deflated entries, no encryption, no ZIP64). Names are checked: an entry that would land outside
//! the folder it is unpacked into (`..`, an absolute path, a drive letter) makes the whole file fail.

use std::io::Read;

/// Together the files may unpack to at most this much.
pub const MAX_TOTAL: u64 = 200 * 1024 * 1024;
/// At most this many entries.
pub const MAX_ENTRIES: usize = 5000;

pub struct Entry {
    /// The path inside the zip, with `/` between the parts and never `..`, `.` or an empty part.
    pub name: String,
    pub data: Vec<u8>,
    /// The file can be run (from the Unix permissions the zip kept, when it kept them).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub executable: bool,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The cleaned path of an entry name, `None` for a folder, `Err` for a name that leaves the folder.
fn clean_name(raw: &str) -> Result<Option<String>, String> {
    let s = raw.replace('\\', "/");
    if s.starts_with('/') || s.contains(':') || s.contains('\0') {
        return Err(format!("The zip holds a file with a path that is not allowed: {raw}"));
    }
    let mut parts = vec![];
    for p in s.split('/') {
        match p {
            "" | "." => {}
            ".." => return Err(format!("The zip holds a file with a path that is not allowed: {raw}")),
            p => parts.push(p),
        }
    }
    if s.ends_with('/') || parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(parts.join("/")))
}

/// All files in the zip (folders are left out; they come with their files).
pub fn read(zip: &[u8]) -> Result<Vec<Entry>, String> {
    let bad = || "That is not a zip file, or it is damaged".to_string();
    // The end record is in the last 22 bytes + up to 64 KB of comment.
    let from = zip.len().saturating_sub(22 + 65535);
    let end = (from..zip.len().saturating_sub(21)).rev().find(|&i| u32_at(zip, i) == Some(0x0605_4b50)).ok_or_else(bad)?;
    let count = u16_at(zip, end + 10).ok_or_else(bad)? as usize;
    let mut at = u32_at(zip, end + 16).ok_or_else(bad)? as usize;
    if count == 0xffff || at == 0xffff_ffff {
        return Err("Zip files over 4 GB (ZIP64) are not supported".into());
    }
    if count > MAX_ENTRIES {
        return Err(format!("The zip holds more than {MAX_ENTRIES} files"));
    }
    let mut out = vec![];
    let mut total = 0u64;
    for _ in 0..count {
        if u32_at(zip, at) != Some(0x0201_4b50) {
            return Err(bad());
        }
        let made_by = u16_at(zip, at + 4).ok_or_else(bad)? >> 8;
        let flags = u16_at(zip, at + 8).ok_or_else(bad)?;
        let method = u16_at(zip, at + 10).ok_or_else(bad)?;
        let packed = u32_at(zip, at + 20).ok_or_else(bad)? as usize;
        let size = u32_at(zip, at + 24).ok_or_else(bad)? as u64;
        let name_len = u16_at(zip, at + 28).ok_or_else(bad)? as usize;
        let extra_len = u16_at(zip, at + 30).ok_or_else(bad)? as usize;
        let comment_len = u16_at(zip, at + 32).ok_or_else(bad)? as usize;
        let attrs = u32_at(zip, at + 38).ok_or_else(bad)?;
        let local = u32_at(zip, at + 42).ok_or_else(bad)? as usize;
        let raw = zip.get(at + 46..at + 46 + name_len).ok_or_else(bad)?;
        // Bit 11: the name is UTF-8; older zips use the DOS code page, close enough for ASCII names.
        let raw = String::from_utf8_lossy(raw).to_string();
        at += 46 + name_len + extra_len + comment_len;
        let Some(name) = clean_name(&raw)? else { continue };
        if flags & 1 != 0 {
            return Err(format!("{name} is encrypted; send the zip without a password"));
        }
        if packed == 0xffff_ffff || size == 0xffff_ffff {
            return Err("Zip files over 4 GB (ZIP64) are not supported".into());
        }
        total += size;
        if total > MAX_TOTAL {
            return Err(format!("The zip unpacks to more than {} MB", MAX_TOTAL / 1024 / 1024));
        }
        if u32_at(zip, local) != Some(0x0403_4b50) {
            return Err(bad());
        }
        let start = local + 30 + u16_at(zip, local + 26).ok_or_else(bad)? as usize + u16_at(zip, local + 28).ok_or_else(bad)? as usize;
        let packed = zip.get(start..start + packed).ok_or_else(bad)?;
        let data = match method {
            0 => packed.to_vec(),
            8 => {
                let mut d = Vec::with_capacity(size as usize);
                flate2::read::DeflateDecoder::new(packed).take(size + 1).read_to_end(&mut d).map_err(|_| bad())?;
                d
            }
            m => return Err(format!("{name} is packed in a way that is not supported (method {m}); use a normal zip")),
        };
        if data.len() as u64 != size {
            return Err(bad());
        }
        // Made on Unix (3) or macOS (19): the upper half holds the permissions.
        let executable = matches!(made_by, 3 | 19) && (attrs >> 16) & 0o111 != 0;
        out.push(Entry { name, data, executable });
    }
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;

    /// A zip of these files (name, content, deflated), as zip programs write them.
    pub fn make(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let (mut body, mut dir) = (vec![], vec![]);
        for (name, data, deflate) in files {
            let packed = if *deflate {
                let mut e = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.to_vec()
            };
            let crc = 0u32; // not checked when reading
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = body.len() as u32;
            body.extend(0x0403_4b50u32.to_le_bytes());
            body.extend([20, 0, 0, 0]);
            body.extend(method.to_le_bytes());
            body.extend([0, 0, 0, 0]);
            body.extend(crc.to_le_bytes());
            body.extend((packed.len() as u32).to_le_bytes());
            body.extend((data.len() as u32).to_le_bytes());
            body.extend((name.len() as u16).to_le_bytes());
            body.extend([0, 0]);
            body.extend(name.as_bytes());
            body.extend(&packed);
            dir.extend(0x0201_4b50u32.to_le_bytes());
            dir.extend([20, 3, 20, 0, 0, 0]); // made on Unix
            dir.extend(method.to_le_bytes());
            dir.extend([0, 0, 0, 0]);
            dir.extend(crc.to_le_bytes());
            dir.extend((packed.len() as u32).to_le_bytes());
            dir.extend((data.len() as u32).to_le_bytes());
            dir.extend((name.len() as u16).to_le_bytes());
            dir.extend([0, 0, 0, 0, 0, 0, 0, 0]);
            let mode: u32 = if name.ends_with(".sh") { 0o100755 } else { 0o100644 };
            dir.extend((mode << 16).to_le_bytes());
            dir.extend(offset.to_le_bytes());
            dir.extend(name.as_bytes());
        }
        let dir_at = body.len() as u32;
        let dir_len = dir.len() as u32;
        body.extend(dir);
        body.extend(0x0605_4b50u32.to_le_bytes());
        body.extend([0, 0, 0, 0]);
        body.extend((files.len() as u16).to_le_bytes());
        body.extend((files.len() as u16).to_le_bytes());
        body.extend(dir_len.to_le_bytes());
        body.extend(dir_at.to_le_bytes());
        body.extend([0, 0]);
        body
    }

    #[test]
    fn reads_stored_and_deflated_files() {
        let text = "print('hi')\n".repeat(50);
        let z = make(&[("p/", b"", false), ("p/plugin.json", b"{}", false), ("p/main.py", text.as_bytes(), true), ("p/run.sh", b"#!/bin/sh", false)]);
        let e = read(&z).unwrap();
        assert_eq!(e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["p/plugin.json", "p/main.py", "p/run.sh"]);
        assert_eq!(e[1].data, text.as_bytes());
        assert!(e[2].executable && !e[1].executable);
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        for name in ["../evil.py", "p/../../evil.py", "/etc/passwd", "C:/x.py", "p\\..\\..\\x"] {
            assert!(read(&make(&[(name, b"x", false)])).is_err(), "{name} was accepted");
        }
        assert_eq!(read(&make(&[("./p//a.py", b"x", false)])).unwrap()[0].name, "p/a.py");
        assert!(read(b"not a zip at all").is_err());
    }
}
