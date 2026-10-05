//! RAM pokes: a value the engine writes into the console's RAM after every frame.
//!
//! ## Why a poke is stored as a code string
//!
//! A poke made from a RAM search lives in the SAME per-game cheat list as the codes the user
//! typed, so it can be named, toggled, deleted, exported and counted against the same 128 cap,
//! and so that list stays the one thing pushed whole and in order. The list is a list of strings,
//! so a poke is a string too, in a shape no core uses: `poke:<address>:<value>:<bytes>`, all hex,
//! for example `poke:00C0:63:1`. The engine splits those out of the list before the core sees it
//! (see `EmulatorBridge::apply_cheats`), so a core never receives a code it would misread.
//!
//! ## Bus pokes
//!
//! A poke made from a search over one of a core's MAPPED regions (the GBA's internal work RAM at
//! `$03000000`, say, from `RETRO_ENVIRONMENT_SET_MEMORY_MAPS`) carries the console's own address
//! and a fourth field: `poke:03001234:63:1:bus`. The engine resolves it through the core's memory
//! map every frame instead of indexing `SYSTEM_RAM`. Old three-field codes mean what they always
//! did.
//!
//! ## Cheat-file pokes
//!
//! A RetroArch `.cht` RAM cheat (`cheatN_handler = 1`) gives an address in RetroArch's OWN cheat
//! address space, and that is not `SYSTEM_RAM` on every core. RetroArch's
//! `cheat_manager_initialize_memory` (cheat_manager.c) builds the space from the core's memory map
//! when it can: every descriptor flagged `RETRO_MEMDESC_SYSTEM_RAM` that has a buffer and a length,
//! laid end to end in descriptor order. Only a core with no such descriptor gets plain
//! `SYSTEM_RAM`. On mGBA's GBA that space is IWRAM (32 KB) followed by EWRAM (256 KB), so a file's
//! `0x8010` is EWRAM byte `0x10`, while mGBA's `SYSTEM_RAM` is EWRAM alone and would put the same
//! number 32 KB further in. So these pokes carry a fourth field, `poke:8010:63:1:cht`, and the
//! engine resolves them RetroArch's way every frame (see `memory_maps::write_cheat`). The address
//! is still an offset, so it is shown like a `SYSTEM_RAM` one.
//!
//! ## Byte order
//!
//! Little endian, because every system Continuum runs that has a RAM worth searching stores
//! multi-byte values that way in the buffer libretro exposes, and the search reads them the same
//! way. One rule for both halves is what makes "make a cheat from this address" write back the
//! exact value the search displayed.
//!
//! The one exception is a cheat-file poke whose entry says `cheatN_big_endian = true`: RetroArch's
//! `cheat_manager_apply_retro_cheats` writes that cheat most significant byte first, so the file's
//! author chose the order and it is kept, as a fifth field: `poke:8010:03E8:2:cht:be`.

/// The prefix that marks a cheat list entry as an engine poke rather than a core code.
pub const POKE_PREFIX: &str = "poke:";

/// The fourth field of a bus poke. See the module note.
pub const BUS_SUFFIX: &str = "bus";

/// The fourth field of a cheat-file poke. See the module note.
pub const CHT_SUFFIX: &str = "cht";

/// The fifth field of a big-endian cheat-file poke. See the module note on byte order.
pub const BIG_ENDIAN_SUFFIX: &str = "be";

/// One poke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Poke {
    /// Byte offset into `SYSTEM_RAM`, the console's own address when `bus` is set, or an address
    /// in RetroArch's cheat address space when `cht` is set.
    pub address: u32,
    pub value: u32,
    /// 1, 2 or 4.
    pub bytes: u8,
    /// `address` is a console bus address, resolved through the core's memory map.
    pub bus: bool,
    /// `address` is a RetroArch cheat-manager address, from a `.cht` RAM cheat. Never set together
    /// with `bus`.
    pub cht: bool,
    /// Written most significant byte first. Only ever set on a `cht` poke wider than one byte: it
    /// is the only kind whose source can ask for it, and a single byte has no order.
    pub big_endian: bool,
}

impl Poke {
    /// Builds a poke, refusing a width that is not 1, 2 or 4 or a value that does not fit it.
    pub fn new(address: u32, value: u32, bytes: u8) -> Result<Self, String> {
        let max = match bytes {
            1 => 0xFF,
            2 => 0xFFFF,
            4 => u32::MAX,
            other => return Err(format!("a poke is 1, 2 or 4 bytes wide, not {other}")),
        };
        if value > max {
            return Err(format!(
                "{value} does not fit in {bytes} byte(s); the largest is {max}"
            ));
        }
        Ok(Self {
            address,
            value,
            bytes,
            bus: false,
            cht: false,
            big_endian: false,
        })
    }

    /// A poke at a console bus address. See the module note.
    pub fn new_bus(address: u32, value: u32, bytes: u8) -> Result<Self, String> {
        Self::new(address, value, bytes).map(|poke| Self { bus: true, ..poke })
    }

    /// A poke at a RetroArch cheat-manager address, from a `.cht` RAM cheat. See the module note.
    ///
    /// `big_endian` is dropped for a one-byte poke, which has no byte order, so that two pokes
    /// that write the same thing compare equal and have the same code.
    pub fn new_cht(address: u32, value: u32, bytes: u8, big_endian: bool) -> Result<Self, String> {
        Self::new(address, value, bytes).map(|poke| Self {
            cht: true,
            big_endian: big_endian && bytes > 1,
            ..poke
        })
    }

    /// The canonical code string. See the module note.
    pub fn code(&self) -> String {
        let digits = usize::from(self.bytes) * 2;
        // A console address is shown whole ($03001234); a RAM offset, which is what a cheat-file
        // address is too, as at least four digits.
        let (suffix, address_digits) = if self.bus {
            (format!(":{BUS_SUFFIX}"), 8)
        } else if self.cht && self.big_endian {
            (format!(":{CHT_SUFFIX}:{BIG_ENDIAN_SUFFIX}"), 4)
        } else if self.cht {
            (format!(":{CHT_SUFFIX}"), 4)
        } else {
            (String::new(), 4)
        };
        format!(
            "{POKE_PREFIX}{:0address_digits$X}:{:0digits$X}:{}{suffix}",
            self.address, self.value, self.bytes
        )
    }

    /// Whether a cheat list entry is a poke at all, whether or not it parses.
    pub fn is_poke_code(code: &str) -> bool {
        code.trim()
            .get(..POKE_PREFIX.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(POKE_PREFIX))
    }

    /// Parses a code made by [`Poke::code`]. Case-insensitive and whitespace-tolerant, because the
    /// Swift store upper-cases typed codes and a user may type one by hand.
    pub fn parse(code: &str) -> Result<Self, String> {
        let trimmed = code.trim();
        if !Self::is_poke_code(trimmed) {
            return Err(format!("'{trimmed}' is not a poke code"));
        }
        let body = &trimmed[POKE_PREFIX.len()..];
        let parts: Vec<&str> = body.split(':').map(str::trim).collect();
        let is = |field: &str, word: &str| field.eq_ignore_ascii_case(word);
        // (bus, cht, big endian). Anything else is refused rather than guessed at, so a typo in
        // the mode cannot quietly turn into a write somewhere else.
        let (bus, cht, big_endian) = match parts[..] {
            [_, _, _] => (false, false, false),
            [_, _, _, mode] if is(mode, BUS_SUFFIX) => (true, false, false),
            [_, _, _, mode] if is(mode, CHT_SUFFIX) => (false, true, false),
            [_, _, _, mode, order] if is(mode, CHT_SUFFIX) && is(order, BIG_ENDIAN_SUFFIX) => {
                (false, true, true)
            }
            _ => {
                return Err(format!(
                    "'{trimmed}' should be poke:ADDRESS:VALUE:BYTES, for example poke:00C0:63:1 \
                     (or with :bus on the end for a console address, or :cht for an address \
                     from a RetroArch cheat file)"
                ))
            }
        };
        let hex = |text: &str, what: &str| {
            let text = text
                .strip_prefix("0x")
                .or_else(|| text.strip_prefix("0X"))
                .unwrap_or(text);
            u32::from_str_radix(text, 16)
                .map_err(|_| format!("the {what} in '{trimmed}' is not a hex number"))
        };
        let address = hex(parts[0], "address")?;
        let value = hex(parts[1], "value")?;
        let bytes: u8 = parts[2]
            .parse()
            .map_err(|_| format!("the width in '{trimmed}' should be 1, 2 or 4"))?;
        if bus {
            Self::new_bus(address, value, bytes)
        } else if cht {
            Self::new_cht(address, value, bytes, big_endian)
        } else {
            Self::new(address, value, bytes)
        }
    }

    /// The bytes this poke writes, in the order they go into memory: little endian, or most
    /// significant first for a big-endian cheat-file poke. See the module note on byte order.
    pub fn value_bytes(&self) -> Vec<u8> {
        self.ordered_bytes()[..usize::from(self.bytes)].to_vec()
    }

    /// [`Self::value_bytes`] without the allocation, for the write every frame. Only the first
    /// `bytes` entries mean anything.
    fn ordered_bytes(&self) -> [u8; 4] {
        if !self.big_endian {
            return self.value.to_le_bytes();
        }
        let width = usize::from(self.bytes);
        let mut out = [0; 4];
        out[..width].copy_from_slice(&self.value.to_be_bytes()[4 - width..]);
        out
    }

    /// Writes this poke into `ram`, a buffer whose byte 0 is address 0: `SYSTEM_RAM` for a plain
    /// poke, and for a cheat-file poke on a core whose memory map gives RetroArch no cheat RAM.
    /// Returns false, and writes nothing, when it does not fit.
    ///
    /// Out of range is not an error worth stopping a frame for: a region can shrink between games
    /// on the same core, and a poke made for one game's RAM size may simply not apply.
    pub fn apply(&self, ram: &mut [u8]) -> bool {
        let start = self.address as usize;
        let width = usize::from(self.bytes);
        let Some(end) = start.checked_add(width) else {
            return false;
        };
        let Some(target) = ram.get_mut(start..end) else {
            return false;
        };
        target.copy_from_slice(&self.ordered_bytes()[..width]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_round_trips() {
        for poke in [
            Poke::new(0xC0, 0x63, 1).unwrap(),
            Poke::new(0x1234, 0xBEEF, 2).unwrap(),
            Poke::new(0x1F_FFFF, 0xDEAD_BEEF, 4).unwrap(),
        ] {
            assert_eq!(Poke::parse(&poke.code()).unwrap(), poke);
        }
        assert_eq!(Poke::new(0xC0, 0x63, 1).unwrap().code(), "poke:00C0:63:1");
        assert_eq!(Poke::new(0xC0, 0x63, 2).unwrap().code(), "poke:00C0:0063:2");
    }

    #[test]
    fn bus_pokes_round_trip_and_old_codes_are_unchanged() {
        let bus = Poke::new_bus(0x0300_1234, 0x63, 1).unwrap();
        assert_eq!(bus.code(), "poke:03001234:63:1:bus");
        assert_eq!(Poke::parse(&bus.code()).unwrap(), bus);
        assert!(Poke::parse("POKE:03001234:63:1:BUS").unwrap().bus);
        assert!(!Poke::parse("poke:00C0:63:1").unwrap().bus);
        assert!(Poke::parse("poke:00C0:63:1:map").is_err());
        assert!(Poke::parse("poke:00C0:63:1:bus:x").is_err());
        assert_eq!(Poke::new(1, 0xAABB, 2).unwrap().value_bytes(), vec![0xBB, 0xAA]);
    }

    #[test]
    fn cheat_file_pokes_round_trip() {
        let cht = Poke::new_cht(0x8010, 0x63, 1, false).unwrap();
        assert_eq!(cht.code(), "poke:8010:63:1:cht");
        assert_eq!(Poke::parse(&cht.code()).unwrap(), cht);
        assert!(cht.cht && !cht.bus && !cht.big_endian);

        let be = Poke::new_cht(0x1234, 0x03E8, 2, true).unwrap();
        assert_eq!(be.code(), "poke:1234:03E8:2:cht:be");
        assert_eq!(Poke::parse(&be.code()).unwrap(), be);
        let wide = Poke::new_cht(0x4_7FFC, 0xDEAD_BEEF, 4, false).unwrap();
        assert_eq!(wide.code(), "poke:47FFC:DEADBEEF:4:cht");
        assert_eq!(Poke::parse(&wide.code()).unwrap(), wide);

        // The Swift store upper-cases typed codes.
        assert_eq!(Poke::parse("POKE:1234:03E8:2:CHT:BE").unwrap(), be);
        // One byte has no order, so `:be` on it is the same poke as without.
        assert_eq!(Poke::parse("poke:8010:63:1:cht:be").unwrap(), cht);
        assert_eq!(Poke::new_cht(0x8010, 0x63, 1, true).unwrap(), cht);

        // Byte order is only ever a fifth field after `cht`.
        assert!(Poke::parse("poke:8010:63:1:be").is_err());
        assert!(Poke::parse("poke:8010:63:1:bus:be").is_err());
        assert!(Poke::parse("poke:8010:63:1:cht:le").is_err());
        assert!(Poke::parse("poke:8010:63:1:cht:be:x").is_err());
        assert!(
            Poke::parse("poke:8010:1FF:1:cht").is_err(),
            "value too large for one byte"
        );
        assert!(Poke::is_poke_code("poke:8010:63:1:cht"));
    }

    #[test]
    fn old_codes_parse_exactly_as_before() {
        // Saved cheat lists hold these; the new mode must not change a single field of them.
        assert_eq!(
            Poke::parse("poke:00C0:63:1").unwrap(),
            Poke {
                address: 0xC0,
                value: 0x63,
                bytes: 1,
                bus: false,
                cht: false,
                big_endian: false
            }
        );
        assert_eq!(
            Poke::parse("poke:03001234:BEEF:2:bus").unwrap(),
            Poke {
                address: 0x0300_1234,
                value: 0xBEEF,
                bytes: 2,
                bus: true,
                cht: false,
                big_endian: false
            }
        );
        for code in [
            "poke:00C0:63:1",
            "poke:1234:BEEF:2",
            "poke:03001234:63:1:bus",
        ] {
            assert_eq!(Poke::parse(code).unwrap().code(), code);
        }
    }

    #[test]
    fn parse_is_lenient_about_case_and_spacing() {
        let poke = Poke::parse("  POKE:0x00c0 : 63 : 1 ").unwrap();
        assert_eq!(poke, Poke::new(0xC0, 0x63, 1).unwrap());
        let cht = Poke::parse(" poke:0x8010 : 63 : 1 : Cht ").unwrap();
        assert_eq!(cht, Poke::new_cht(0x8010, 0x63, 1, false).unwrap());
    }

    #[test]
    fn parse_refuses_bad_shapes() {
        assert!(Poke::parse("SXIOPO").is_err());
        assert!(Poke::parse("poke:00C0:63").is_err());
        assert!(Poke::parse("poke:zz:63:1").is_err());
        assert!(Poke::parse("poke:00C0:63:3").is_err());
        assert!(Poke::parse("poke:00C0:1FF:1").is_err(), "value too large for one byte");
    }

    #[test]
    fn detects_poke_codes() {
        assert!(Poke::is_poke_code("poke:1:2:1"));
        assert!(Poke::is_poke_code("POKE:garbage"));
        assert!(!Poke::is_poke_code("010-CAFE"));
        assert!(!Poke::is_poke_code("po"));
    }

    #[test]
    fn applies_little_endian_and_refuses_out_of_range() {
        let mut ram = [0u8; 8];
        assert!(Poke::new(1, 0xAABB, 2).unwrap().apply(&mut ram));
        assert_eq!(ram, [0, 0xBB, 0xAA, 0, 0, 0, 0, 0]);
        assert!(Poke::new(4, 0x0102_0304, 4).unwrap().apply(&mut ram));
        assert_eq!(&ram[4..], &[4, 3, 2, 1]);
        assert!(!Poke::new(7, 1, 2).unwrap().apply(&mut ram));
        assert!(!Poke::new(u32::MAX, 1, 4).unwrap().apply(&mut ram));
    }

    #[test]
    fn big_endian_cheat_file_pokes_write_most_significant_first() {
        let mut ram = [0u8; 8];
        let two = Poke::new_cht(1, 0xAABB, 2, true).unwrap();
        assert_eq!(two.value_bytes(), vec![0xAA, 0xBB]);
        assert!(two.apply(&mut ram));
        assert_eq!(&ram[..3], &[0, 0xAA, 0xBB]);
        let four = Poke::new_cht(4, 0x0102_0304, 4, true).unwrap();
        assert_eq!(four.value_bytes(), vec![1, 2, 3, 4]);
        assert!(four.apply(&mut ram));
        assert_eq!(&ram[4..], &[1, 2, 3, 4]);
        // Without `be` a cheat-file poke is little endian like every other.
        assert_eq!(
            Poke::new_cht(0, 0xAABB, 2, false).unwrap().value_bytes(),
            vec![0xBB, 0xAA]
        );
    }
}
