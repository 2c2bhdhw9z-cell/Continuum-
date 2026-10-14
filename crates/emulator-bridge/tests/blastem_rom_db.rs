//! BlastEm's cartridge list as Continuum ships it: `scripts/patches/blastem-cartridge-saves.patch`.
//!
//! BlastEm finds a Mega Drive cartridge's save hardware (an EEPROM, an SRAM, a J-Cart's two extra
//! pads) in `rom.db`, by the sha1 of the whole ROM first and then by the product code in its
//! header. A cartridge it does not find gets only what its header says, and a header cannot
//! describe an EEPROM, so an EEPROM game it misses never saves. The patch adds every EEPROM and
//! J-Cart dump in MAME's software list (`hash/megadriv.xml`, CC0 data) that `rom.db` did not
//! list by checksum, keyed by the sha1 MAME gives, plus two fixes.
//!
//! Always run: the new entries parse by BlastEm's own rules (`config.c`, `parse_config_int`) and
//! each one is exactly what the table below says MAME lists. With `BLASTEM_SRC` pointing at a
//! clean BlastEm checkout at the pin in `scripts/build-core.sh`, the test also applies every
//! BlastEm patch in the build's order and parses the whole patched `rom.db`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One node of BlastEm's config tree: a value, or a block of named children.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Value(String),
    Block(BTreeMap<String, Node>),
}

impl Node {
    fn block(&self) -> &BTreeMap<String, Node> {
        match self {
            Node::Block(b) => b,
            Node::Value(v) => panic!("expected a block, found the value {v:?}"),
        }
    }
    fn get(&self, key: &str) -> Option<&Node> {
        self.block().get(key)
    }
    fn value(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Node::Value(v) => Some(v),
            Node::Block(_) => None,
        }
    }
}

/// BlastEm's `parse_config_int`, line for line: a line is trimmed; blank lines and lines starting
/// with `#` are skipped; a line starting with `}` closes the block; a line ending in `{` opens one
/// named by the rest of it; anything else is a key, whitespace, and a value. BlastEm stops the
/// whole emulator on a `}` with no block open and silently drops a key with no value, and a
/// repeated key replaces the first one, so all three are errors here.
fn parse(text: &str) -> Result<BTreeMap<String, Node>, String> {
    fn block<'a>(
        lines: &mut impl Iterator<Item = (usize, &'a str)>,
        open: bool,
    ) -> Result<BTreeMap<String, Node>, String> {
        let mut head = BTreeMap::new();
        while let Some((n, raw)) = lines.next() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('}') {
                return if open { Ok(head) } else { Err(format!("line {n}: a }} with no block open")) };
            }
            let (key, node) = if let Some(name) = line.strip_suffix('{') {
                (name.trim().to_string(), Node::Block(block(lines, true)?))
            } else {
                let mut parts = line.splitn(2, char::is_whitespace);
                let key = parts.next().unwrap_or_default();
                let value = parts.next().map(str::trim).unwrap_or_default();
                if value.is_empty() {
                    return Err(format!("line {n}: {key:?} has no value"));
                }
                (key.to_string(), Node::Value(value.to_string()))
            };
            if head.insert(key.clone(), node).is_some() {
                return Err(format!("line {n}: {key:?} is there twice"));
            }
        }
        if open { Err("a block is never closed".into()) } else { Ok(head) }
    }
    block(&mut text.lines().enumerate().map(|(i, l)| (i + 1, l)), false)
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("{s:?} is not hex"))
}

/// The lines a patch adds to one file, hunk by hunk.
fn added_lines(patch: &str, file: &str) -> Vec<Vec<String>> {
    let mut hunks = Vec::new();
    let mut in_file = false;
    for line in patch.lines() {
        if let Some(name) = line.strip_prefix("+++ b/") {
            in_file = name == file;
        } else if line.starts_with("@@") && in_file {
            hunks.push(Vec::new());
        } else if in_file && !line.starts_with("+++") {
            if let (Some(added), Some(hunk)) = (line.strip_prefix('+'), hunks.last_mut()) {
                hunk.push(added.to_string());
            }
        }
    }
    hunks
}

/// The wiring of each board, as BlastEm's own entries write it: (address, last, device,
/// bits_read, bits_write) per map block. `None` for `last` is the ROM's own size.
type Block = (u32, Option<u32>, &'static str, &'static [(u8, &'static str)], &'static [(u8, &'static str)]);
const SEGA: &[Block] = &[
    (0, Some(0x1F_FFFF), "ROM", &[], &[]),
    (0x20_0000, Some(0x3F_FFFF), "EEPROM", &[(0, "sda")], &[(0, "sda"), (1, "scl")]),
];
const EA: &[Block] = &[
    (0, None, "ROM", &[], &[]),
    (0x20_0000, Some(0x3F_FFFF), "EEPROM", &[(7, "sda")], &[(6, "scl"), (7, "sda")]),
];
const ACCLAIM16: &[Block] = &[
    (0, Some(0x1F_FFFF), "ROM", &[], &[]),
    (0x20_0000, Some(0x3F_FFFF), "EEPROM", &[(1, "sda")], &[(0, "sda"), (1, "scl")]),
];
const ACCLAIM32: &[Block] = &[
    (0, Some(0x1F_FFFF), "ROM", &[], &[]),
    (0x20_0000, Some(0x20_0001), "EEPROM", &[(0, "sda")], &[(0, "sda"), (8, "scl")]),
    (0x20_0002, None, "ROM", &[], &[]),
];
const CODEMASTERS: &[Block] = &[
    (0, Some(0x2F_FFFF), "ROM", &[], &[]),
    (0x30_0000, Some(0x37_FFFF), "EEPROM", &[], &[(0, "sda"), (1, "scl")]),
    (0x38_0000, Some(0x3F_FFFF), "EEPROM", &[(7, "sda")], &[]),
];
const CODEMASTERS_JCART: &[Block] = &[
    (0, Some(0x2F_FFFF), "ROM", &[], &[]),
    (0x30_0000, Some(0x37_FFFF), "EEPROM", &[], &[(0, "sda"), (1, "scl")]),
    (0x38_0000, Some(0x38_7FFF), "EEPROM", &[(7, "sda")], &[]),
    (0x38_8000, Some(0x38_FFFF), "jcart", &[], &[]),
];
const JCART: &[Block] = &[(0, Some(0x1F_FFFF), "ROM", &[], &[]), (0x20_0000, Some(0x3F_FFFF), "jcart", &[], &[])];

/// Every entry the patch adds, as MAME's software list has it: the set, the sha1 of its one ROM
/// dump, the ROM's size in bytes, the EEPROM's size in bytes (0: none), and the board's wiring.
/// The sizes are BlastEm's for the same game, or for a game MAME puts on the same board.
const FROM_MAME: &[(&str, &str, u32, u32, &[Block])] = &[
    ("evander", "eb4aca22f8b5837a0a0b10491c46714948b09844", 0x8_0000, 128, SEGA),
    ("gameto", "9673b519c16883c146924c347ec6764281fa3a57", 0x2_0000, 128, SEGA),
    ("ghw", "db28d70f9b744004cadd06eb77d6223534f59215", 0x20_0000, 128, SEGA),
    ("ghwu", "4ef7aec80003aab0e1d2260b7fc0d22d63d90038", 0x20_0000, 128, SEGA),
    ("ghwj", "1d5fe812df75e0dc1ff379c563058e078b839a09", 0x20_0000, 128, SEGA),
    ("megaman", "ea9ae2043c97db716a8d31ee90e581c3d75f4e3e", 0x20_0000, 128, SEGA),
    ("rockman1", "a435ac53589a29dbb655662c942daab425d3f6bd", 0x20_0000, 128, SEGA),
    ("sporttbb", "e223513d9bcecb49a6798720f3195dbd1c34681c", 0x10_0000, 128, SEGA),
    ("wboymw", "772c3c71973762eac46e6c180a036c1f3bbd937a", 0x10_0000, 128, SEGA),
    ("wboy5", "1582f159e1969ff0541319a9bd7e6f7a53505d01", 0xA_0000, 128, SEGA),
    ("turmamon", "80fc2a6a6b8b943f781598094f3b5a5fe4f05ede", 0x10_0000, 128, SEGA),
    ("ddanpei", "ebdf20fd8aaeb3c7ec97302089b3330265118cf0", 0x8_0000, 128, SEGA),
    ("ninjab", "9cc3419ca7ecaf0d106aa896ffc0266c7145fff7", 0x10_0000, 128, SEGA),
    ("billwlsh", "2bbb454900ac99172a2d72d1e6f96a96b8d6840b", 0x10_0000, 128, EA),
    ("madden93", "59d2352ecb31bc1305128a9d8df894a3bfd684cf", 0x10_0000, 128, EA),
    ("madd93ce", "417acacb9f9ef90d08b3cfb81972a9d8b56f4293", 0x10_0000, 128, EA),
    ("nhlpa93", "2ab048fc7209df28b00ef47f2f686f5b7208466e", 0x8_0000, 128, EA),
    ("nhlpa93a", "8efc1cacb079ea223966dda065c16c49e584cac2", 0x8_0000, 128, EA),
    ("nhlpa93b", "af5dbf3e70417753164ceeb76be63f51e492626f", 0x8_0000, 128, EA),
    ("ringspow", "84851ce4527761b8d74ce581c19e15d0dd17f368", 0x10_0000, 128, EA),
    ("nbajam1", "99c5bc57fdea7f9df0cd8dec54160b162342344d", 0x20_0000, 256, ACCLAIM16),
    ("nbajam", "55f2b26a932c69b2c7cb4f24f56b43f24f113a7c", 0x20_0000, 256, ACCLAIM16),
    ("nbajamj", "2a88b2e1ecf115fa6246397d829448b755a5385e", 0x20_0000, 256, ACCLAIM16),
    ("nbajamp", "1aad8b146ec1eb2a9823878fbfddc02efe3f6ad9", 0x20_0000, 256, ACCLAIM16),
    ("nflqb", "5c8b6eb2934cb97e35af36efd70c39dae899048f", 0x40_0000, 256, ACCLAIM32),
    ("nbajamtef", "0f5bb5d5352fe2ebe4b4051a1dd9b9fde4b505ab", 0x30_0000, 512, ACCLAIM32),
    ("blockb", "1a06213a3a26c9105fc3013a141c22d212045a0b", 0x40_0000, 512, ACCLAIM32),
    ("nflqb96", "2948419532a6079404f05348bc4bbf2dd989622d", 0x40_0000, 2048, ACCLAIM32),
    ("collslam", "0dbbe740b14077fe8648955f7e17965ea25f382a", 0x40_0000, 8192, ACCLAIM32),
    ("bighurt", "9c978aaab10e16be59558561b07a0c610c74b43e", 0x40_0000, 8192, ACCLAIM32),
    ("brianlar", "032eb1a15c95540b392b52ee17eef7eee6ac436d", 0x10_0000, 1024, CODEMASTERS),
    ("brianlar1", "0f4bec98af08027fc034b9801c28bb299a04be35", 0x10_0000, 1024, CODEMASTERS),
    ("brianl96", "3e1ef39e9008a4a55fd57b25948668c8c52ba9e3", 0x10_0000, 8192, CODEMASTERS),
    ("brianl96a", "0b08788a0f8214c5d07b8e2293f0b954dd05bef5", 0x10_0000, 8192, CODEMASTERS),
    ("shanewar", "4264625be91c3d96edca9c5bfe6259d00ca8b737", 0x10_0000, 8192, CODEMASTERS),
    ("micromc2", "ab29077a6a5c2ccc777b0bf22f4d5908401f4d47", 0x10_0000, 2048, CODEMASTERS_JCART),
    ("micromc2a", "cb5fb33212592809639b37c2babd72a7953fa102", 0x10_0000, 2048, CODEMASTERS_JCART),
    ("sampra96", "139b9cea418c7c932033ddff87bf10466828cbfa", 0x20_0000, 0, JCART),
    ("sampras1", "3f73dcabcdf0d781834fcc673533301b82f0e91b", 0x10_0000, 0, JCART),
];

/// The other Rockman Mega World: MAME's `rockman`, a ROM and SRAM board with the same product
/// code as the EEPROM one, listed with no map so BlastEm reads its own header.
const ROCKMAN_SRAM: &str = "4dfcf5c07106f6db4d8b7e73a053bb46a21686c3";

/// One entry checked against its row: BlastEm's required fields, then MAME's numbers.
fn check_entry(key: &str, entry: &Node, row: &(&str, &str, u32, u32, &[Block])) {
    let (set, _, rom_size, eeprom, wiring) = *row;
    let name = entry.value("name").unwrap_or_else(|| panic!("{set}: no name"));
    assert!(!name.is_empty() && name.is_ascii(), "{set}: name {name:?}");
    match entry.get("EEPROM") {
        Some(chip) => {
            // process_eeprom_def: type i2c (the only one BlastEm has) and a size it can parse.
            assert_eq!(chip.value("type"), Some("i2c"), "{set}");
            assert_eq!(chip.value("size").map(|s| s.parse::<u32>().unwrap()), Some(eeprom), "{set}");
        }
        None => assert_eq!(eeprom, 0, "{set}: the EEPROM is missing"),
    }
    let map = entry.get("map").unwrap_or_else(|| panic!("{set}: no map")).block();
    assert_eq!(map.len(), wiring.len(), "{set}: {map:?}");
    for (address, last, device, read, write) in wiring.iter() {
        let block = map
            .get(&format!("{address:X}"))
            .unwrap_or_else(|| panic!("{set}: nothing mapped at {address:X}"));
        // map_node_common: `last` is required, non-zero and not below the start.
        let got_last = hex(block.value("last").unwrap_or_else(|| panic!("{set}: {address:X} has no last")));
        assert!(got_last >= *address && got_last != 0, "{set}: {address:X}");
        assert_eq!(got_last, last.unwrap_or(rom_size - 1), "{set}: {address:X}");
        assert_eq!(block.value("device"), Some(*device), "{set}: {address:X}");
        let bits = |which: &str| -> Vec<(u8, String)> {
            block.get(which).map_or(Vec::new(), |b| {
                b.block()
                    .iter()
                    .map(|(bit, pin)| match pin {
                        Node::Value(pin) => (bit.parse().unwrap(), pin.clone()),
                        Node::Block(_) => panic!("{set}: bit {bit} is a block"),
                    })
                    .collect()
            })
        };
        let want = |list: &[(u8, &str)]| -> Vec<(u8, String)> {
            let mut v: Vec<_> = list.iter().map(|(b, p)| (*b, p.to_string())).collect();
            v.sort();
            v
        };
        let (mut got_read, mut got_write) = (bits("bits_read"), bits("bits_write"));
        got_read.sort();
        got_write.sort();
        assert_eq!(got_read, want(read), "{set}: {address:X} bits_read");
        assert_eq!(got_write, want(write), "{set}: {address:X} bits_write");
        if *device == "ROM" && *address != 0 {
            assert_eq!(block.value("offset").map(hex), Some(*address & !0xF), "{set}: {address:X} offset");
        }
    }
    // The comment naming the set is what lets anyone check the entry against MAME's list.
    assert!(key.len() == 40 && key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{key}");
}

fn the_patch() -> String {
    std::fs::read_to_string(repo().join("scripts/patches/blastem-cartridge-saves.patch"))
        .expect("scripts/patches/blastem-cartridge-saves.patch")
}

#[test]
fn every_new_cartridge_parses_and_is_what_mames_list_says() {
    let patch = the_patch();
    let hunks = added_lines(&patch, "rom.db");
    let block = hunks
        .iter()
        .find(|h| h.first().is_some_and(|l| l.starts_with("#Continuum: every EEPROM and J-Cart cartridge dump")))
        .expect("the hunk with the new entries");
    let text = block.join("\n");
    let db = parse(&text).unwrap_or_else(|e| panic!("BlastEm would not read the new entries: {e}"));
    assert_eq!(db.len(), FROM_MAME.len() + 1, "{:?}", db.keys().collect::<Vec<_>>());
    for row in FROM_MAME {
        let (set, sha1, ..) = *row;
        let entry = db.get(sha1).unwrap_or_else(|| panic!("{set} ({sha1}) is not in the patch"));
        check_entry(sha1, entry, row);
        // Each entry's own comment names its MAME set, right under the key.
        let at = text.find(&format!("{sha1} {{")).unwrap();
        assert!(text[at..].lines().nth(1).unwrap().trim().starts_with(&format!("#{set}: ")), "{set}");
    }
    let rockman = db.get(ROCKMAN_SRAM).expect("the SRAM Rockman Mega World");
    assert_eq!(rockman.value("name"), Some("Rockman Mega World"));
    assert!(rockman.get("map").is_none() && rockman.get("EEPROM").is_none(), "{rockman:?}");
}

#[test]
fn the_two_fixes_are_in_the_patch() {
    let patch = the_patch();
    // NFL Quarterback Club: MAME's dump is one 4 MB chip, so the ROM is mapped to $3FFFFF.
    let nfl = patch.split("@@").find(|h| h.contains("T-081276 {")).expect("the NFL Quarterback Club hunk");
    assert!(nfl.contains("\n-\t\t\tlast 2FFFFF\n") && nfl.contains("\n+\t\t\tlast 3FFFFF\n"), "{nfl}");
    // Micro Machines 96's second dump (MAME microm96a) gets the J-Cart its board has.
    let mm96 = patch.split("@@").find(|h| h.contains("+\t\t\tdevice jcart\n")).expect("the Micro Machines 96 hunk");
    assert!(mm96.contains("+\t\t388000 {\n+\t\t\tdevice jcart\n+\t\t\tlast 38FFFF\n"), "{mm96}");
    assert!(mm96.contains("-\t\t\tlast 3FFFFF\n+\t\t\tlast 387FFF\n"), "{mm96}");
}

/// The BlastEm patches in the order `scripts/build-core.sh` applies them, from its blastem arm.
fn patches_in_build_order() -> Vec<String> {
    let script = std::fs::read_to_string(repo().join("scripts/build-core.sh")).unwrap();
    let words: Vec<&str> = script.split_whitespace().collect();
    words
        .windows(3)
        .filter(|w| w[0] == "ios_apply_simple_patch" && w[1] == "blastem")
        .map(|w| w[2].to_string())
        .collect()
}

#[test]
fn the_build_applies_the_cartridge_list_after_the_memory_patch() {
    assert_eq!(
        patches_in_build_order()[..2],
        ["blastem-continuum-memory.patch".to_string(), "blastem-cartridge-saves.patch".to_string()]
    );
}

/// With `BLASTEM_SRC` set to a clean checkout of BlastEm at the pin: every patch applies in the
/// build's order, and the whole patched `rom.db` parses with each key once.
#[test]
fn the_patches_apply_to_the_pinned_source_and_the_whole_list_parses() {
    let Ok(src) = std::env::var("BLASTEM_SRC") else {
        eprintln!("BLASTEM_SRC not set: skipped (point it at a clean BlastEm checkout at the pin)");
        return;
    };
    let work = std::env::temp_dir().join(format!("blastem-rom-db-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let copied = std::process::Command::new("cp").arg("-a").arg(&src).arg(&work).status().unwrap();
    assert!(copied.success());
    for name in patches_in_build_order() {
        let file = std::fs::File::open(repo().join("scripts/patches").join(&name)).unwrap();
        let out = std::process::Command::new("patch")
            .args(["-p1", "--forward", "--batch", "-d"])
            .arg(&work)
            .stdin(file)
            .output()
            .unwrap();
        assert!(out.status.success(), "{name} does not apply: {}", String::from_utf8_lossy(&out.stdout));
    }
    let text = String::from_utf8_lossy(&std::fs::read(work.join("rom.db")).unwrap()).into_owned();
    let db = parse(&text).unwrap_or_else(|e| panic!("the patched rom.db: {e}"));
    for row in FROM_MAME {
        check_entry(row.1, db.get(row.1).unwrap_or_else(|| panic!("{} missing", row.0)), row);
    }
    // The fixes, read back from the patched list itself.
    let nfl = &db["T-081276"];
    assert_eq!(nfl.get("map").unwrap().get("200002").unwrap().value("last"), Some("3FFFFF"));
    let mm96 = db["9f47fcc7bb2f5921cb1c3beb06b668ffb292cb08"].get("map").unwrap().block().clone();
    assert_eq!(mm96["388000"].value("device"), Some("jcart"));
    assert_eq!(mm96["380000"].value("last"), Some("387FFF"));
    eprintln!("{} entries in the patched rom.db", db.len());
    let _ = std::fs::remove_dir_all(&work);
}
