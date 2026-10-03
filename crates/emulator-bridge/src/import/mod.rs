//! Getting games, saves and manuals into the app, the platform-neutral half.
//!
//! Everything here is plain file and byte work with no iOS in it, so Android reuses it as is:
//!
//! - [`detect`] names the system of a file whose extension several systems share (.cue, .chd,
//!   .iso, .bin, .zip and friends) by looking inside it: disc headers, cue sheets, CHD metadata,
//!   cartridge headers and archive listings.
//! - [`archive`] lists and unpacks .zip and .7z, and zips a folder back up (PSP and 3DS saves).
//! - [`saves`] converts save files between the formats other emulators write (.dsv, .mcr, .gme,
//!   .eep, .sra, .fla, ...) and the raw battery RAM the cores read.
//! - [`webdav`] builds the PROPFIND request and parses its multistatus answer.
//! - [`http`] parses the request head the Wi-Fi transfer server receives, and holds its page.
//!
//! The system ids are the shared strings every lane uses (see `wt-notes` BRIEF): `ps1`, `segacd`,
//! `saturn`, `dreamcast`, `pcecd`, `psp`, `arcade`, `dos`, `amiga` and the rest.

pub mod archive;
pub mod detect;
pub mod http;
pub mod saves;
pub mod webdav;

#[cfg(test)]
pub(crate) mod testdir {
    //! A unique scratch directory per test, under the system temp dir, removed on drop.
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    pub struct TestDir(PathBuf);

    impl TestDir {
        pub fn new(tag: &str) -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("continuum-import-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
