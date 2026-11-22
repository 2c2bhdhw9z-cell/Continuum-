//! A small log file next to the player's data, so a problem on the TV can be read over SSH.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

static LOG: Mutex<Option<File>> = Mutex::new(None);

pub fn open(path: &str) {
    // Keep the log small: start a fresh one when it passes 1 MB.
    if std::fs::metadata(path).map(|m| m.len() > 1 << 20).unwrap_or(false) {
        let _ = std::fs::rename(path, format!("{path}.old"));
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(path) {
        *LOG.lock().unwrap() = Some(f);
    }
}

pub fn line(s: &str) {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let text = format!("{t} {s}\n");
    eprint!("{text}");
    if let Ok(mut g) = LOG.lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(text.as_bytes());
        }
    }
}

#[macro_export]
macro_rules! logln {
    ($($t:tt)*) => { $crate::log::line(&format!($($t)*)) };
}
