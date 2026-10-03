//! What a J2ME `.jar` says about itself, from `META-INF/MANIFEST.MF`: the MIDlet class the
//! engine must start, the game's name, and the screen size it was made for when it says.
//!
//! The engine is told the class up front (`midletClassName` in the player page's address). With
//! no class it waits for a phone-information answer this player never gives, and a jar with no
//! `MIDlet-1` line is not a MIDlet at all, which is a plain refusal rather than a blank screen.

use std::io::{Cursor, Read};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// `MIDlet-1`'s class, like `com.example.Game`.
    pub midlet_class: String,
    /// `MIDlet-Name`, or `MIDlet-1`'s name, or empty.
    pub name: String,
    /// `MIDlet-Vendor`, or empty.
    pub vendor: String,
    /// The screen the game was made for, when the manifest or the file name says.
    pub screen: Option<(u32, u32)>,
}

/// Manifest attributes, continuation lines joined (a line starting with one space continues the
/// previous one, per the JAR specification).
pub fn attributes(text: &str) -> Vec<(String, String)> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ') {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        lines.push(line.to_string());
    }
    lines
        .into_iter()
        .filter_map(|line| {
            let (k, v) = line.split_once(':')?;
            let k = k.trim();
            (!k.is_empty()).then(|| (k.to_string(), v.trim().to_string()))
        })
        .collect()
}

/// "240,320", "240x320" or "240 x 320": the first two numbers, joined only by a separator.
fn size_in(text: &str) -> Option<(u32, u32)> {
    let text = text.trim();
    let first_end = text.find(|c: char| !c.is_ascii_digit())?;
    let (w, rest) = text.split_at(first_end);
    let rest = rest.trim_start_matches([' ', 'x', 'X', ',', '*']);
    let second_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let h = &rest[..second_end];
    let w: u32 = w.parse().ok()?;
    let h: u32 = h.parse().ok()?;
    ((64..=1024).contains(&w) && (64..=1024).contains(&h)).then_some((w, h))
}

/// Reads the manifest text. `file_name` is a fallback for the screen size ("Game_240x320.jar").
pub fn parse(text: &str, file_name: &str) -> Result<Manifest, String> {
    let attrs = attributes(text);
    let get = |key: &str| {
        attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.clone())
    };
    let midlet = get("MIDlet-1")
        .ok_or("the jar has no MIDlet-1 line in its manifest, so it is not a J2ME game")?;
    let parts: Vec<&str> = midlet.split(',').map(str::trim).collect();
    let class = parts.last().copied().unwrap_or("").to_string();
    if class.is_empty() {
        return Err(format!("the manifest's MIDlet-1 line names no class: '{midlet}'"));
    }
    let name = get("MIDlet-Name")
        .filter(|n| !n.is_empty())
        .or_else(|| parts.first().map(|s| s.to_string()))
        .unwrap_or_default();
    let screen = [
        "Nokia-MIDlet-Original-Display-Size",
        "Nokia-MIDlet-Target-Display-Size",
        "MIDlet-Screen-Size",
    ]
    .iter()
    .find_map(|key| get(key).and_then(|v| size_in(&v)))
    .or_else(|| {
        let stem = file_name.rsplit_once('.').map(|(s, _)| s).unwrap_or(file_name);
        // Only an explicit WxH in a file name, never two stray numbers.
        stem.split(|c: char| !c.is_ascii_alphanumeric())
            .find_map(|part| {
                let lower = part.to_ascii_lowercase();
                let (w, h) = lower.split_once('x')?;
                let w: u32 = w.parse().ok()?;
                let h: u32 = h.parse().ok()?;
                ((64..=1024).contains(&w) && (64..=1024).contains(&h)).then_some((w, h))
            })
    });
    Ok(Manifest {
        midlet_class: class.replace('/', "."),
        name,
        vendor: get("MIDlet-Vendor").unwrap_or_default(),
        screen,
    })
}

/// Reads the manifest out of a `.jar` on disk.
pub fn read(path: &Path) -> Result<Manifest, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("the jar could not be read: {e}"))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("the jar is not a readable zip: {e}"))?;
    let index = (0..archive.len())
        .find(|&i| {
            archive
                .name_for_index(i)
                .is_some_and(|n| n.eq_ignore_ascii_case("META-INF/MANIFEST.MF"))
        })
        .ok_or("the jar has no META-INF/MANIFEST.MF, so it is not a J2ME game")?;
    let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    entry
        .by_ref()
        .take(1024 * 1024)
        .read_to_end(&mut raw)
        .map_err(|e| format!("the manifest could not be read: {e}"))?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    parse(&String::from_utf8_lossy(&raw), file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const MF: &str = "Manifest-Version: 1.0\r\nMIDlet-1: Bounce Tales, /icon.png, com.nokia.bounce\r\n .BounceMIDlet\r\nMIDlet-Name: Bounce Tales\r\nMIDlet-Vendor: Nokia\r\nNokia-MIDlet-Original-Display-Size: 240,320\r\n";

    #[test]
    fn reads_class_name_vendor_and_size() {
        let m = parse(MF, "bounce.jar").unwrap();
        assert_eq!(m.midlet_class, "com.nokia.bounce.BounceMIDlet");
        assert_eq!(m.name, "Bounce Tales");
        assert_eq!(m.vendor, "Nokia");
        assert_eq!(m.screen, Some((240, 320)));
    }

    #[test]
    fn falls_back_to_the_file_name_for_size() {
        let m = parse("MIDlet-1: G,,a.B\n", "Game_176x208.jar").unwrap();
        assert_eq!(m.midlet_class, "a.B");
        assert_eq!(m.name, "G");
        assert_eq!(m.screen, Some((176, 208)));
        assert_eq!(parse("MIDlet-1: G,,a.B\n", "Game 2 4.jar").unwrap().screen, None);
    }

    #[test]
    fn refuses_a_jar_that_is_not_a_midlet() {
        assert!(parse("Manifest-Version: 1.0\n", "x.jar").is_err());
        assert!(parse("MIDlet-1: Name, icon,\n", "x.jar").is_err());
    }

    #[test]
    fn sizes_inside_values() {
        assert_eq!(size_in("240,320"), Some((240, 320)));
        assert_eq!(size_in("360 x 640"), Some((360, 640)));
        assert_eq!(size_in("big"), None);
        assert_eq!(size_in("10,20"), None);
    }

    #[test]
    fn reads_from_a_real_zip() {
        let dir = std::env::temp_dir().join(format!("continuum-jar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Game.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("META-INF/MANIFEST.MF", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(MF.as_bytes()).unwrap();
        zip.finish().unwrap();
        let m = read(&path).unwrap();
        assert_eq!(m.midlet_class, "com.nokia.bounce.BounceMIDlet");
        std::fs::write(&path, b"not a zip").unwrap();
        assert!(read(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
