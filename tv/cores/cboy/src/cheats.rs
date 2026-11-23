//! Cheat codes on the Game Boy and Game Boy Color: the kinds mGBA (the app's other Game Boy
//! emulator) reads, so a code from any cheat list works on either.
//!
//! - **Game Genie**, `XXX-XXX`, or `XXX-XXX-XXX` with a compare byte. The device sits between the
//!   cartridge and the console and answers the code's byte when the console reads the code's
//!   address from the cartridge's ROM; with a compare byte, only while the cartridge's own byte
//!   there is that one (which is how a code meant for one bank leaves the others alone). The
//!   digits, dashes left out: the first two are the new byte; the address is the sixth (xor F) on
//!   top of the third, fourth and fifth; the compare byte is the seventh and ninth, rotated right
//!   by two and xor BA; the eighth isn't used. A code for an address past the ROM ($8000 up) is
//!   not one the device can make, and is left out (as SameBoy does). Nothing changes while the
//!   console's own start-up program runs.
//! - **GameShark**, `TTVVLLHH`: the byte `VV` written at `HHLL` at the start of every picture's
//!   blank, the moment the device steps in. `TT` says where: 01 is wherever the address is now.
//!   Anything else carries a bank number in its low digit (80-8F, 90-97 on cheat lists): a
//!   cartridge RAM bank for $A000-$BFFF, a Game Boy Color work RAM bank for $D000-$DFFF. The
//!   device pages that bank in for the write and puts the game's own back; here the byte goes
//!   straight into that bank, the same thing. mGBA ignores `TT`, so a banked code lands in
//!   whichever bank happens to be in; SameBoy reads it as here.
//! - **CodeBreaker** (`BBAAAA-VV`, as mGBA reads it: `VV` written at `AAAA`) and **a raw address
//!   and value** (`C000:FF`), written the way an `01` GameShark code is.
//!
//! Several codes can be joined with `+`. Anything that is none of these is left out.

/// One Game Genie code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Genie {
    pub addr: u16,
    pub value: u8,
    /// Only while the ROM's own byte at `addr` is this one.
    pub compare: Option<u8>,
}

/// Where a written code's byte goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Wherever the address is mapped now.
    Mapped,
    /// That cartridge RAM bank ($A000-$BFFF).
    CartBank(u8),
    /// That work RAM bank of a Game Boy Color ($D000-$DFFF, and its echo).
    WramBank(u8),
}

/// One code that writes a byte at the start of every blank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Poke {
    pub addr: u16,
    pub value: u8,
    pub target: Target,
}

/// One code, as read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Genie(Genie),
    Poke(Poke),
}

/// Every code switched on, ready for the console to use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cheats {
    pub genie: Vec<Genie>,
    pub pokes: Vec<Poke>,
}

fn hex_digits(s: &str) -> Option<Vec<u8>> {
    s.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect()
}

fn hex_number(s: &str, max_digits: usize) -> Option<u32> {
    if s.is_empty() || s.len() > max_digits {
        return None;
    }
    hex_digits(s).map(|d| d.iter().fold(0u32, |acc, &x| acc << 4 | u32::from(x)))
}

/// One code as typed: spaces ignored, any case. `None` when it is none of these kinds.
pub fn parse(code: &str) -> Option<Code> {
    let s: String = code.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    if let Some((a, v)) = s.split_once(':') {
        let addr = hex_number(a, 4)? as u16;
        let value = hex_number(v, 2)? as u8;
        return Some(Code::Poke(Poke { addr, value, target: Target::Mapped }));
    }
    // CodeBreaker: six digits, a dash, two digits.
    if s.len() == 9 && s.as_bytes()[6] == b'-' {
        if let (Some(a), Some(v)) = (hex_number(&s[..6], 6), hex_number(&s[7..], 2)) {
            return Some(Code::Poke(Poke { addr: a as u16, value: v as u8, target: Target::Mapped }));
        }
    }
    if s.len() == 8 && !s.contains('-') {
        let d = hex_number(&s, 8)?;
        let (kind, value) = ((d >> 24) as u8, (d >> 16) as u8);
        let addr = ((d & 0xFF) as u16) << 8 | ((d >> 8) & 0xFF) as u16;
        let target = match (kind, addr) {
            (0x01, _) => Target::Mapped,
            (_, 0xA000..=0xBFFF) => Target::CartBank(kind & 0x0F),
            (_, 0xD000..=0xDFFF | 0xF000..=0xFDFF) => Target::WramBank(kind & 0x0F),
            _ => Target::Mapped,
        };
        return Some(Code::Poke(Poke { addr, value, target }));
    }
    // Game Genie: the dashes, if any, only where the device prints them.
    let dashes_ok = match s.len() {
        7 => s.as_bytes()[3] == b'-',
        11 => s.as_bytes()[3] == b'-' && s.as_bytes()[7] == b'-',
        6 | 9 => true,
        _ => false,
    };
    if !dashes_ok {
        return None;
    }
    let d = hex_digits(&s.replace('-', ""))?;
    if d.len() != 6 && d.len() != 9 {
        return None;
    }
    let addr = u16::from(d[5] ^ 0xF) << 12 | u16::from(d[2]) << 8 | u16::from(d[3]) << 4 | u16::from(d[4]);
    if addr > 0x7FFF {
        return None;
    }
    let compare = (d.len() == 9).then(|| (d[6] << 4 | d[8]).rotate_right(2) ^ 0xBA);
    Some(Code::Genie(Genie { addr, value: d[0] << 4 | d[1], compare }))
}

impl Cheats {
    /// The codes switched on (each may join several with `+`), in order.
    pub fn from_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> Cheats {
        let mut cheats = Cheats::default();
        for part in codes.into_iter().flat_map(|c| c.split('+')) {
            match parse(part) {
                Some(Code::Genie(g)) => cheats.genie.push(g),
                Some(Code::Poke(p)) => cheats.pokes.push(p),
                None => {}
            }
        }
        cheats
    }

    pub fn is_empty(&self) -> bool {
        self.genie.is_empty() && self.pokes.is_empty()
    }

    /// The byte the console sees when the ROM answers `v` at `addr`: the first Game Genie code
    /// for that address whose compare byte (if it has one) matches, else the ROM's own.
    #[inline]
    pub fn read(&self, addr: u16, v: u8) -> u8 {
        self.genie
            .iter()
            .find(|g| g.addr == addr && g.compare.is_none_or(|c| c == v))
            .map_or(v, |g| g.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_genie_codes_are_read_like_the_device_sameboy_and_mgba() {
        // 01A-B2F: byte 01 at ((F xor F) << 12) | A << 8 | B << 4 | 2.
        assert_eq!(parse("01A-B2F"), Some(Code::Genie(Genie { addr: 0x0AB2, value: 0x01, compare: None })));
        // With a compare byte from the seventh and ninth digits: $E6 rotated right by two is
        // $B9, xor BA: $03.
        assert_eq!(
            parse("01A-B2F-E06"),
            Some(Code::Genie(Genie { addr: 0x0AB2, value: 0x01, compare: Some(0x03) }))
        );
        assert_eq!(parse("01a b2f e46"), parse("01A-B2F-E06"), "the eighth digit doesn't count");
        // Past the ROM: the sixth digit 7 makes $8xxx, which no Game Genie can patch.
        assert_eq!(parse("01A-B27"), None);
        assert_eq!(parse("01-AB2F"), None, "dashes where the device never prints them");
    }

    #[test]
    fn gameshark_codes_carry_their_bank() {
        // 01 FF 34 C1: byte FF at $C134, wherever it is.
        assert_eq!(parse("01FF34C1"), Some(Code::Poke(Poke { addr: 0xC134, value: 0xFF, target: Target::Mapped })));
        assert_eq!(parse("9263 05D0"), Some(Code::Poke(Poke { addr: 0xD005, value: 0x63, target: Target::WramBank(2) })));
        assert_eq!(parse("8399 00A0"), Some(Code::Poke(Poke { addr: 0xA000, value: 0x99, target: Target::CartBank(3) })));
        // A bank digit on an address with no banks is just a write.
        assert_eq!(parse("9263 05C0"), Some(Code::Poke(Poke { addr: 0xC005, value: 0x63, target: Target::Mapped })));
    }

    #[test]
    fn codebreaker_and_raw_codes_write_a_byte() {
        assert_eq!(parse("01C123-45"), Some(Code::Poke(Poke { addr: 0xC123, value: 0x45, target: Target::Mapped })));
        assert_eq!(parse("c123:45"), Some(Code::Poke(Poke { addr: 0xC123, value: 0x45, target: Target::Mapped })));
    }

    #[test]
    fn anything_else_is_left_out() {
        for bad in ["", "12", "1234567", "123456789A", "C1234:45", "C123:456", "GG-HHH", "01C12-345"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
        let c = Cheats::from_codes(["junk", "01FF34C1+01A-B2F", ""]);
        assert_eq!((c.pokes.len(), c.genie.len()), (1, 1));
    }

    #[test]
    fn a_game_genie_code_changes_its_address_only_while_the_compare_byte_is_there() {
        let c = Cheats::from_codes(["01A-B2F-E06"]);
        assert_eq!(c.read(0x0AB2, 0x03), 0x01);
        assert_eq!(c.read(0x0AB2, 0x04), 0x04);
        assert_eq!(c.read(0x0AB3, 0x03), 0x03);
    }

    /// A Game Boy Color game on an MBC5 cartridge with four RAM banks, looping at $0100.
    fn colour_machine() -> crate::gb::GameBoy {
        let mut rom = vec![0u8; 0x8000];
        rom[0x100..0x102].copy_from_slice(&[0x18, 0xFE]); // JR $
        rom[0x143] = 0x80;
        rom[0x147] = 0x1B;
        rom[0x149] = 0x03;
        crate::gb::GameBoy::new_mode(rom, true).unwrap()
    }

    #[test]
    fn a_banked_gameshark_code_writes_into_its_own_bank() {
        let mut gb = colour_machine();
        // Work RAM bank 2 at $D005, while the game has bank 1 paged in.
        gb.bus.raw_write(0xFF70, 1);
        gb.bus.cheats = Cheats::from_codes(["926305D0"]);
        gb.bus.apply_cheat_pokes();
        assert_eq!(gb.bus.wram[2 * 0x1000 + 5], 0x63);
        assert_ne!(gb.bus.raw_read(0xD005), 0x63, "the game's own bank is left as it was");
        gb.bus.raw_write(0xFF70, 2);
        assert_eq!(gb.bus.raw_read(0xD005), 0x63);
        // Cartridge RAM bank 3 at $A000, while bank 0 is in, and only with the RAM switched on.
        gb.bus.cheats = Cheats::from_codes(["839900A0"]);
        let before = gb.bus.cart.ram[3 * 0x2000];
        gb.bus.apply_cheat_pokes();
        assert_eq!(gb.bus.cart.ram[3 * 0x2000], before, "the game keeps its RAM switched off");
        gb.bus.raw_write(0x0000, 0x0A);
        gb.bus.apply_cheat_pokes();
        assert_eq!(gb.bus.cart.ram[3 * 0x2000], 0x99);
        assert_ne!(gb.bus.raw_read(0xA000), 0x99, "bank 0 still in");
        gb.bus.raw_write(0x4000, 3);
        assert_eq!(gb.bus.raw_read(0xA000), 0x99);
    }

    #[test]
    fn a_game_genie_code_changes_what_the_console_reads_from_rom() {
        let mut gb = colour_machine();
        gb.bus.cheats = Cheats::from_codes(["5A1-02F"]); // $0102 = $5A
        assert_eq!(gb.bus.raw_read(0x0102), 0x5A);
        assert_eq!(gb.bus.raw_read(0x0103), 0x00);
        gb.bus.cheats = Cheats::default();
        assert_eq!(gb.bus.raw_read(0x0102), 0x00);
    }
}
