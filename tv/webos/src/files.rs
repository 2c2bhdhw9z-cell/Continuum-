//! Where things live on the TV, safe file writes, reading games out of .zip and .7z files, and
//! battery saves and save states.

use std::io::Read;
use std::path::{Path, PathBuf};

pub struct Paths {
    pub data: PathBuf,
    pub games: PathBuf,
    pub saves: PathBuf,
    pub states: PathBuf,
    pub system: PathBuf,
    pub app: PathBuf,
}

impl Paths {
    pub fn new() -> Paths {
        let data = std::env::var("CONTINUUM_DATA").unwrap_or_else(|_| "/media/developer/continuum".into());
        let games = std::env::var("CONTINUUM_GAMES").unwrap_or_else(|_| "/media/developer/games".into());
        let app = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let data = PathBuf::from(data);
        let p = Paths {
            saves: data.join("saves"),
            states: data.join("states"),
            system: data.join("system"),
            games: PathBuf::from(games),
            data,
            app,
        };
        for d in [&p.data, &p.games, &p.saves, &p.states, &p.system] {
            let _ = std::fs::create_dir_all(d);
        }
        p
    }

    pub fn settings(&self) -> String {
        self.data.join("settings.json").to_string_lossy().into_owned()
    }
    pub fn core_options(&self) -> String {
        self.data.join("cboy-options.json").to_string_lossy().into_owned()
    }
    pub fn library(&self) -> String {
        self.data.join("library.json").to_string_lossy().into_owned()
    }
    pub fn log(&self) -> String {
        self.data.join("log.txt").to_string_lossy().into_owned()
    }
}

/// Write a file so a power cut never leaves half of it: write a temporary copy, then swap it in.
pub fn write_atomic(path: impl AsRef<Path>, data: &[u8]) -> bool {
    let path = path.as_ref();
    let tmp = path.with_extension(format!(
        "{}.writing",
        path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    let ok = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if let Err(e) = &ok {
        crate::logln!("couldn't write {}: {e}", path.display());
        let _ = std::fs::remove_file(&tmp);
    }
    ok.is_ok()
}

pub const ROM_EXTS: &[&str] = &["gb", "gbc", "dmg", "cgb", "sgb", "gba", "agb"];
pub const ARCHIVE_EXTS: &[&str] = &["zip", "7z"];

pub fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// The game inside an archive: (its file name, its bytes). The first Game Boy file wins.
pub fn read_archive(path: &Path) -> Result<(String, Vec<u8>), String> {
    let ext = ext_of(&path.to_string_lossy());
    if ext == "zip" {
        let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut z = zip::ZipArchive::new(f).map_err(|e| format!("not a readable zip: {e}"))?;
        let mut pick = None;
        for i in 0..z.len() {
            if let Ok(e) = z.by_index(i) {
                let n = e.name().to_string();
                if ROM_EXTS.contains(&ext_of(&n).as_str()) && !e.is_dir() {
                    pick = Some(i);
                    break;
                }
            }
        }
        let i = pick.ok_or("there's no Game Boy game in this zip")?;
        let mut e = z.by_index(i).map_err(|e| e.to_string())?;
        let name = Path::new(e.name()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut buf = Vec::with_capacity(e.size() as usize);
        e.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Ok((name, buf))
    } else if ext == "7z" {
        let mut found: Option<(String, Vec<u8>)> = None;
        let mut r = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty()).map_err(|e| format!("not a readable 7z: {e}"))?;
        r.for_each_entries(|entry, reader| {
            let n = entry.name().to_string();
            if found.is_none() && !entry.is_directory() && ROM_EXTS.contains(&ext_of(&n).as_str()) {
                let mut buf = Vec::new();
                reader.read_to_end(&mut buf)?;
                let name = Path::new(&n).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or(n);
                found = Some((name, buf));
                return Ok(false);
            }
            // skip this entry's bytes
            std::io::copy(reader, &mut std::io::sink())?;
            Ok(true)
        })
        .map_err(|e| e.to_string())?;
        found.ok_or_else(|| "there's no Game Boy game in this 7z".to_string())
    } else {
        Err("not an archive".into())
    }
}

/// The name of the game inside an archive, without unpacking it all (zip), or by unpacking (7z).
pub fn archive_inner(path: &Path) -> Option<(String, Vec<u8>)> {
    let ext = ext_of(&path.to_string_lossy());
    if ext == "zip" {
        let f = std::fs::File::open(path).ok()?;
        let mut z = zip::ZipArchive::new(f).ok()?;
        for i in 0..z.len() {
            let mut e = z.by_index(i).ok()?;
            let n = e.name().to_string();
            if ROM_EXTS.contains(&ext_of(&n).as_str()) && !e.is_dir() {
                // the header is in the first 0x150 bytes (Game Boy) or 0xC0 (Advance)
                let mut head = vec![0u8; 0x150];
                let got = read_up_to(&mut e, &mut head);
                head.truncate(got);
                let name = Path::new(&n).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or(n);
                return Some((name, head));
            }
        }
        None
    } else {
        read_archive(path).ok().map(|(n, mut b)| {
            b.truncate(0x150);
            (n, b)
        })
    }
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> usize {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    n
}

pub fn read_head(path: &Path) -> Vec<u8> {
    let mut head = vec![0u8; 0x150];
    let n = std::fs::File::open(path).map(|mut f| read_up_to(&mut f, &mut head)).unwrap_or(0);
    head.truncate(n);
    head
}

/// Free space on the disk holding `path`, in bytes.
pub fn free_space(path: &Path) -> u64 {
    let c = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) == 0 {
            s.f_bavail as u64 * s.f_frsize as u64
        } else {
            0
        }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "just now", "5 min ago", "2h ago", "3d ago"
pub fn ago(t: u64) -> String {
    let now = now_secs();
    if t == 0 {
        return String::new();
    }
    let d = now.saturating_sub(t);
    if d < 60 {
        "just now".into()
    } else if d < 3600 {
        format!("{} min ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else if d < 86400 * 30 {
        format!("{}d ago", d / 86400)
    } else {
        format!("{} months ago", d / (86400 * 30))
    }
}

// ---- save states ----

/// The folder holding one game's save states.
pub fn state_dir(paths: &Paths, game_id: &str) -> PathBuf {
    paths.states.join(safe_name(game_id))
}

pub fn state_file(paths: &Paths, game_id: &str, slot: Option<u32>) -> PathBuf {
    let d = state_dir(paths, game_id);
    match slot {
        None => d.join("auto.state"),
        Some(n) => d.join(format!("slot{n}.state")),
    }
}

pub fn safe_name(s: &str) -> String {
    s.chars().map(|c| if c == '/' || c == '\\' || c == '\0' { '_' } else { c }).collect()
}

/// A small picture of the screen, saved next to a save state.
pub fn write_png(path: &Path, w: u32, h: u32, xrgb: &[u32]) {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for p in xrgb.iter().take((w * h) as usize) {
        rgb.push((p >> 16) as u8);
        rgb.push((p >> 8) as u8);
        rgb.push(*p as u8);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut wr) = enc.write_header() {
            let _ = wr.write_image_data(&rgb);
        }
    }
    write_atomic(path, &out);
}

/// Read a PNG as RGBA.
pub fn read_png(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(data);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let px = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        png::ColorType::Rgb => buf[..px * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..px * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..px].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return None,
    };
    Some((w, h, rgba))
}
