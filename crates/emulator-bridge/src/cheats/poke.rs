//! RAM pokes: a value the engine writes into `SYSTEM_RAM` after every frame.
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
//! Little endian, because every system Continuum runs that has a RAM worth searching stores
//! multi-byte values that way in the buffer libretro exposes, and the search reads them the same
//! way. One rule for both halves is what makes "make a cheat from this address" write back the
//! exact value the search displayed.

/// The prefix that marks a cheat list entry as an engine poke rather than a core code.
pub const POKE_PREFIX: &str = "poke:";

/// The fourth field of a bus poke. See the module note.
pub const BUS_SUFFIX: &str = "bus";

/// One poke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Poke {
    /// Byte offset into `SYSTEM_RAM`, or the console's own address when `bus` is set.
    pub address: u32,
    pub value: u32,
    /// 1, 2 or 4.
    pub bytes: u8,
    /// `address` is a console bus address, resolved through the core's memory map.
    pub bus: bool,
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
        })
    }

    /// A poke at a console bus address. See the module note.
    pub fn new_bus(address: u32, value: u32, bytes: u8) -> Result<Self, String> {
        Self::new(address, value, bytes).map(|poke| Self { bus: true, ..poke })
    }

    /// The canonical code string. See the module note.
    pub fn code(&self) -> String {
        let digits = usize::from(self.bytes) * 2;
        // A console address is shown whole ($03001234), a RAM offset as at least four digits.
        let (suffix, address_digits) = if self.bus {
            (format!(":{BUS_SUFFIX}"), 8)
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
        let bus = match parts.len() {
            3 => false,
            4 if parts[3].eq_ignore_ascii_case(BUS_SUFFIX) => true,
            _ => {
                return Err(format!(
                    "'{trimmed}' should be poke:ADDRESS:VALUE:BYTES, for example poke:00C0:63:1 \
                     (or with :bus on the end for a console address)"
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
        } else {
            Self::new(address, value, bytes)
        }
    }

    /// The little-endian bytes this poke writes.
    pub fn value_bytes(&self) -> Vec<u8> {
        self.value.to_le_bytes()[..usize::from(self.bytes)].to_vec()
    }

    /// Writes this poke into `ram`. Returns false, and writes nothing, when it does not fit.
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
        target.copy_from_slice(&self.value.to_le_bytes()[..width]);
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
    fn parse_is_lenient_about_case_and_spacing() {
        let poke = Poke::parse("  POKE:0x00c0 : 63 : 1 ").unwrap();
        assert_eq!(poke, Poke::new(0xC0, 0x63, 1).unwrap());
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
}
