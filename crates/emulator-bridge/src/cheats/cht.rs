//! RetroArch `.cht` cheat files.
//!
//! The format is a flat config file, one `key = value` per line:
//!
//! ```text
//! cheats = 2
//!
//! cheat0_desc = "Infinite Lives"
//! cheat0_code = "SXIOPO"
//! cheat0_enable = false
//!
//! cheat1_desc = "Max money"
//! cheat1_code = "7E0DBA:63+7E0DBB:63"
//! cheat1_enable = true
//! ```
//!
//! Values may be quoted or bare. `#` starts a comment line. A code with `+` in it is several codes
//! the core applies together, and is passed through as one entry, exactly as RetroArch does.
//!
//! Newer RetroArch files also describe RAM cheats with no code at all: `cheatN_handler = 1`,
//! `cheatN_address`, `cheatN_value` and `cheatN_memory_search_size`. When that describes a plain
//! 8, 16 or 32 bit "set to this value" cheat it is turned into a [`super::poke::Poke`] code, which
//! the engine applies every frame the same way RetroArch's handler does. Anything fancier (bit
//! widths, increase/decrease types, repeat counts) is skipped and named in the warnings rather
//! than half-applied.
//!
//! The address in such a cheat is NOT a `SYSTEM_RAM` offset on every core: it is an offset into
//! RetroArch's cheat address space, which on a core that publishes `SYSTEM_RAM`-flagged memory map
//! descriptors (mGBA does, for IWRAM then EWRAM) is those buffers end to end. So the poke made from
//! it is a cheat-file poke (`poke:ADDR:VALUE:BYTES:cht`), resolved that way when it is applied, and
//! carries `cheatN_big_endian` along, which RetroArch honours when it writes. See
//! [`super::poke`].
//!
//! ORDER IS PRESERVED and is the file's numbering, not the order lines happen to appear in,
//! because the cheat list is pushed to the core whole and in order and the core's table is indexed.

use std::collections::BTreeMap;

use super::poke::Poke;

/// One cheat read from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChtCheat {
    pub description: String,
    pub code: String,
    pub enabled: bool,
}

/// Everything read from one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChtFile {
    /// In file order (by index).
    pub cheats: Vec<ChtCheat>,
    /// What the file's `cheats = N` line said, if it had one.
    pub declared: Option<u32>,
    /// One sentence per entry that was skipped, and why. Never silently dropped.
    pub warnings: Vec<String>,
}

/// Indices past this are not read. RetroArch's own files stay in the hundreds; a file claiming a
/// million cheats is damaged or hostile, and the per-game cap is 128 anyway.
const MAX_INDEX: u32 = 4096;

/// `cheatN_memory_search_size`: RetroArch numbers 0 to 5 as 1, 2, 4, 8, 16 and 32 bits.
fn bytes_for_search_size(size: u32) -> Option<u8> {
    match size {
        3 => Some(1),
        4 => Some(2),
        5 => Some(4),
        _ => None,
    }
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let inner = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value);
    inner.to_string()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_number(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

/// Splits `cheat12_desc` into `(12, "desc")`.
fn split_key(key: &str) -> Option<(u32, &str)> {
    let rest = key.strip_prefix("cheat")?;
    let underscore = rest.find('_')?;
    let index: u32 = rest[..underscore].parse().ok()?;
    Some((index, &rest[underscore + 1..]))
}

/// Parses a `.cht` file's text. Never fails outright: a file with nothing usable in it comes back
/// with no cheats and warnings that say why, which is a sentence the user can act on.
pub fn parse(text: &str) -> ChtFile {
    let mut file = ChtFile::default();
    let mut fields: BTreeMap<u32, BTreeMap<String, String>> = BTreeMap::new();
    let mut ignored_indices = false;

    // A UTF-8 byte order mark is common in files saved by Windows editors.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if key == "cheats" {
            file.declared = parse_number(&unquote(value));
            continue;
        }
        let Some((index, field)) = split_key(&key) else {
            continue;
        };
        if index >= MAX_INDEX {
            ignored_indices = true;
            continue;
        }
        fields
            .entry(index)
            .or_default()
            .insert(field.to_string(), unquote(value));
    }
    if ignored_indices {
        file.warnings.push(format!(
            "entries numbered {MAX_INDEX} and above were ignored"
        ));
    }

    let limit = file.declared.unwrap_or(u32::MAX);
    for (index, entry) in fields {
        if index >= limit {
            file.warnings.push(format!(
                "cheat{index} is past the {limit} the file declares, so it was skipped"
            ));
            continue;
        }
        let description = entry.get("desc").cloned().unwrap_or_default();
        let enabled = entry
            .get("enable")
            .and_then(|v| parse_bool(v))
            .unwrap_or(false);
        let label = if description.is_empty() {
            format!("cheat{index}")
        } else {
            description.clone()
        };

        let code = entry.get("code").map(|c| c.trim().to_string()).unwrap_or_default();
        if !code.is_empty() {
            file.cheats.push(ChtCheat {
                description,
                code,
                enabled,
            });
            continue;
        }

        // No code: maybe a RetroArch RAM cheat (handler 1).
        let handler = entry.get("handler").and_then(|v| parse_number(v)).unwrap_or(0);
        if handler != 1 {
            file.warnings.push(format!("{label} has no code, so it was skipped"));
            continue;
        }
        // cheat_type 1 is "set to value". Absent is treated as that, since it is the default.
        let cheat_type = entry.get("cheat_type").and_then(|v| parse_number(v)).unwrap_or(1);
        let repeat = entry.get("repeat_count").and_then(|v| parse_number(v)).unwrap_or(1);
        let bytes = entry
            .get("memory_search_size")
            .and_then(|v| parse_number(v))
            .and_then(bytes_for_search_size);
        let address = entry.get("address").and_then(|v| parse_number(v));
        let value = entry.get("value").and_then(|v| parse_number(v));
        // RetroArch's own test, `string_is_equal(value, "true") || string_is_equal(value, "1")`
        // in `cheat_manager_load_cb_second_pass`, and deliberately not the looser `parse_bool`:
        // this one decides which bytes get written, so a file must mean here what it means there.
        let big_endian = entry
            .get("big_endian")
            .is_some_and(|v| matches!(v.as_str(), "true" | "1"));
        match (cheat_type, repeat, bytes, address, value) {
            (1, 0 | 1, Some(bytes), Some(address), Some(value)) => {
                match Poke::new_cht(address, value, bytes, big_endian) {
                    Ok(poke) => file.cheats.push(ChtCheat {
                        description,
                        code: poke.code(),
                        enabled,
                    }),
                    Err(reason) => file.warnings.push(format!("{label}: {reason}")),
                }
            }
            _ => file.warnings.push(format!(
                "{label} is a RetroArch RAM cheat of a kind Continuum does not apply (only 8, 16 \
                 and 32 bit set-to-value cheats are), so it was skipped"
            )),
        }
    }

    if let Some(declared) = file.declared {
        let seen = file.cheats.len() as u32;
        if declared > seen && file.warnings.is_empty() {
            file.warnings.push(format!(
                "the file declares {declared} cheat(s) and {seen} were found in it"
            ));
        }
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_classic_format_in_index_order() {
        let text = "\
cheats = 3

cheat1_desc = \"Max money\"
cheat1_code = \"7E0DBA:63+7E0DBB:63\"
cheat1_enable = true

cheat0_desc = \"Infinite Lives\"
cheat0_code = \"SXIOPO\"
cheat0_enable = false

cheat2_desc = Moon jump
cheat2_code = 010-CAFE
";
        let file = parse(text);
        assert_eq!(file.declared, Some(3));
        assert!(file.warnings.is_empty(), "{:?}", file.warnings);
        assert_eq!(
            file.cheats,
            vec![
                ChtCheat {
                    description: "Infinite Lives".into(),
                    code: "SXIOPO".into(),
                    enabled: false
                },
                ChtCheat {
                    description: "Max money".into(),
                    code: "7E0DBA:63+7E0DBB:63".into(),
                    enabled: true
                },
                ChtCheat {
                    description: "Moon jump".into(),
                    code: "010-CAFE".into(),
                    // Absent enable means off, as in RetroArch.
                    enabled: false
                },
            ]
        );
    }

    #[test]
    fn tolerates_bom_comments_crlf_and_spacing() {
        let text = "\u{feff}# a comment\r\ncheats=1\r\n  cheat0_desc=\"A\"\r\ncheat0_code   =   \"B\"  \r\ncheat0_enable=TRUE\r\n";
        let file = parse(text);
        assert_eq!(file.cheats.len(), 1);
        assert_eq!(file.cheats[0].code, "B");
        assert!(file.cheats[0].enabled);
    }

    #[test]
    fn entries_past_the_declared_count_are_skipped_and_named() {
        let text = "cheats = 1\ncheat0_code = A\ncheat1_code = B\n";
        let file = parse(text);
        assert_eq!(file.cheats.len(), 1);
        assert_eq!(file.warnings.len(), 1);
        assert!(file.warnings[0].contains("cheat1"));
    }

    #[test]
    fn a_missing_count_line_reads_everything() {
        let file = parse("cheat0_code = A\ncheat5_code = B\n");
        assert_eq!(file.declared, None);
        assert_eq!(file.cheats.len(), 2);
    }

    #[test]
    fn an_entry_with_no_code_is_skipped_with_a_reason() {
        let file = parse("cheats = 2\ncheat0_desc = Empty\ncheat1_code = X\n");
        assert_eq!(file.cheats.len(), 1);
        assert!(file.warnings[0].contains("Empty"));
    }

    #[test]
    fn retroarch_ram_cheats_become_pokes() {
        let text = "\
cheats = 2
cheat0_desc = \"99 lives\"
cheat0_handler = 1
cheat0_address = 192
cheat0_value = 99
cheat0_memory_search_size = 3
cheat0_cheat_type = 1
cheat0_enable = true
cheat1_desc = \"Rising\"
cheat1_handler = 1
cheat1_address = 200
cheat1_value = 1
cheat1_memory_search_size = 3
cheat1_cheat_type = 2
";
        let file = parse(text);
        assert_eq!(file.cheats.len(), 1);
        // A cheat-file poke, not a SYSTEM_RAM one: the address is RetroArch's.
        assert_eq!(file.cheats[0].code, "poke:00C0:63:1:cht");
        assert!(file.cheats[0].enabled);
        assert_eq!(file.warnings.len(), 1);
        assert!(file.warnings[0].contains("Rising"));
    }

    #[test]
    fn sixteen_bit_ram_cheat_with_hex_address() {
        let text = "cheat0_handler = 1\ncheat0_address = 0x1234\ncheat0_value = 1000\n\
                    cheat0_memory_search_size = 4\n";
        let file = parse(text);
        assert_eq!(file.cheats[0].code, "poke:1234:03E8:2:cht");
        let poke = Poke::parse(&file.cheats[0].code).unwrap();
        assert!(poke.cht && !poke.bus && !poke.big_endian);
    }

    #[test]
    fn big_endian_is_read_the_way_retroarch_reads_it() {
        let cheat = |n: u32, size: u32, flag: &str| {
            format!(
                "cheat{n}_handler = 1\ncheat{n}_address = 32784\ncheat{n}_value = 1000\n\
                 cheat{n}_memory_search_size = {size}\ncheat{n}_big_endian = {flag}\n"
            )
        };
        let text = [
            cheat(0, 4, "\"true\""),
            cheat(1, 4, "1"),
            cheat(2, 4, "false"),
            // RetroArch compares the exact strings, so this is little endian there too.
            cheat(3, 4, "TRUE"),
            cheat(4, 5, "true"),
            // One byte has no order.
            cheat(5, 3, "true"),
        ]
        .concat()
        .replace("cheat5_value = 1000", "cheat5_value = 99");
        let codes: Vec<String> = parse(&text).cheats.into_iter().map(|c| c.code).collect();
        assert_eq!(
            codes,
            vec![
                "poke:8010:03E8:2:cht:be",
                "poke:8010:03E8:2:cht:be",
                "poke:8010:03E8:2:cht",
                "poke:8010:03E8:2:cht",
                "poke:8010:000003E8:4:cht:be",
                "poke:8010:63:1:cht",
            ]
        );
    }

    #[test]
    fn garbage_yields_nothing_and_does_not_panic() {
        let file = parse("this is not a cheat file\n\0\0\n=\ncheatX_code=1\ncheat99999999999_code=1");
        assert!(file.cheats.is_empty());
    }

    #[test]
    fn huge_indices_are_ignored_and_said() {
        let file = parse("cheat5000_code = A\n");
        assert!(file.cheats.is_empty());
        assert_eq!(file.warnings.len(), 1);
    }
}
