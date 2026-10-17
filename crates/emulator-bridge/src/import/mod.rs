//! Getting games, saves and manuals into the app, the platform-neutral half.
//!
//! Everything here is plain file and byte work with no iOS in it, so Android reuses it as is:
//!
//! - [`detect`] names the system of a file whose extension several systems share (.cue, .chd,
//!   .iso, .bin, .zip and friends) by looking inside it: disc headers, cue sheets, CHD metadata,
//!   cartridge headers and archive listings. It also tells a text file with a ROM extension
//!   (README.md) from a game.
//! - [`not_games`] moves library files that are text, not games, into the Not games folder.
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
pub mod not_games;
pub mod patch;
pub mod saves;
pub mod webdav;

#[cfg(test)]
pub(crate) mod fixtures {
    //! Generated stand-ins for ROMs and for the text files that got into a library as games.
    //! Every byte is made up here: these are never real ROMs.

    /// Pseudo-random bytes (an LCG), the way a ROM's code and data look to a text check.
    pub fn rom_bytes(len: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 24) as u8
            })
            .collect()
    }

    /// Binary bytes with no 0x00 anywhere, so only the odd-byte count can tell them from text.
    pub fn nul_free_rom(len: usize) -> Vec<u8> {
        rom_bytes(len, 5).into_iter().map(|b| if b == 0 { 0xFF } else { b }).collect()
    }

    /// A raw Mega Drive ROM: 68000 vectors at 0 and the `SEGA MEGA DRIVE` header at 0x100.
    pub fn mega_drive_rom() -> Vec<u8> {
        let mut rom = rom_bytes(0x8000, 7);
        rom[..8].copy_from_slice(&[0x00, 0xFF, 0xFE, 0x00, 0x00, 0x00, 0x02, 0x00]);
        rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
        rom
    }

    /// `raw` as an SMD copier dump: a 512-byte header, then each 16 KB block's odd bytes followed
    /// by its even bytes.
    pub fn smd_dump(raw: &[u8]) -> Vec<u8> {
        const BLOCK: usize = 0x4000;
        let mut out = vec![0u8; 512];
        out[0] = raw.len().div_ceil(BLOCK) as u8;
        out[1] = 0x03;
        out[8] = 0xAA;
        out[9] = 0xBB;
        out[10] = 0x06;
        for block in raw.chunks(BLOCK) {
            let mut padded = block.to_vec();
            padded.resize(BLOCK, 0);
            out.extend(padded.iter().skip(1).step_by(2));
            out.extend(padded.iter().step_by(2));
        }
        out
    }

    fn repeat_to(part: &str, len: usize) -> Vec<u8> {
        let mut out = String::new();
        while out.len() < len {
            out.push_str(part);
        }
        out.into_bytes()
    }

    /// Russian UTF-8 markdown with a heading and a code fence, about 28 KB like the owner's.
    pub fn readme_ru() -> Vec<u8> {
        repeat_to(
            "# Континуум\n\nЭмулятор для iPhone. Установите приложение и откройте игру.\n\n\
             ```sh\ncargo test --workspace\n```\n\n- Сохранения\n- Обложки\n\n",
            28 * 1024,
        )
    }

    /// Traditional Chinese UTF-8 markdown, longer than the text check reads.
    pub fn readme_zh() -> Vec<u8> {
        repeat_to("# 連續體\n\n這是一個給手機用的模擬器。請先安裝應用程式，然後打開遊戲。\n\n", 70 * 1024)
    }

    /// An ASCII MIT licence with CRLF line ends.
    pub fn license_text() -> Vec<u8> {
        "MIT License\r\n\r\nCopyright (c) 2026 The Authors\r\n\r\n\
         Permission is hereby granted, free of charge, to any person obtaining a copy\r\n\
         of this software and associated documentation files (the \"Software\"), to deal\r\n\
         in the Software without restriction, including without limitation the rights\r\n\
         to use, copy, modify, merge, publish, distribute, sublicense, and/or sell\r\n\
         copies of the Software, and to permit persons to whom the Software is\r\n\
         furnished to do so, subject to the following conditions:\r\n\r\n\
         The above copyright notice and this permission notice shall be included in all\r\n\
         copies or substantial portions of the Software.\r\n\r\n\
         THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\r\n\
         IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\r\n\
         FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.\r\n"
            .as_bytes()
            .to_vec()
    }
}

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
