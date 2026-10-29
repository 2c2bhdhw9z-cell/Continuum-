//! Cheat codes: the three kinds Gearsystem (the app's other Master System emulator) reads, so a
//! code from any cheat list works on either.
//!
//! - **Game Genie**, `XXX-XXX`, or `XXX-XXX-XXX` with a compare byte. The device sits between the
//!   cartridge and the console. When the console reads the code's address from the cartridge, it
//!   answers the code's byte instead; with a compare byte, only while the cartridge's own byte
//!   there is that one, which is how a code meant for one bank leaves the other banks alone. It
//!   only sees the cartridge, so the console's own work RAM and start-up program are never
//!   changed. The digits, dashes left out: the first two are the new byte; the address is the
//!   sixth (xor F) on top of the third, fourth and fifth; the compare byte is the seventh and the
//!   ninth, rotated right by two and xor BA; the eighth isn't used. Gearsystem and Genesis Plus GX
//!   read them the same way.
//! - **Pro Action Replay**, `00AAAAVV`, also written `00AA-AAVV`: the byte `VV` written at
//!   `AAAA` whenever the console takes an interrupt, which is when the device steps in (and when
//!   Gearsystem writes them too).
//! - **A raw address and value**, `C000:FF` (or `C000=FF`): written the same way.
//!
//! Several codes can be joined with `+`. Anything that is none of these is left out.
//!
//! Unlike Gearsystem, which rewrites the byte in every bank of the cartridge at that offset, a
//! Game Genie code here is matched on the address the console reads, as the device does: a code
//! for $8123 changes $8123 only, never $0123 or $4123 when the same bank is paged in there.

/// One Game Genie code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Genie {
    pub addr: u16,
    pub value: u8,
    /// Only while the cartridge's own byte at `addr` is this one.
    pub compare: Option<u8>,
}

/// One code, as read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Genie(Genie),
    /// A Pro Action Replay code or a raw one: a byte written at an address.
    Write(u16, u8),
}

/// Every code switched on, ready for the console to use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cheats {
    pub genie: Vec<Genie>,
    pub writes: Vec<(u16, u8)>,
}

fn hex(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

fn hex_number(s: &str, max_digits: usize) -> Option<u32> {
    if s.is_empty() || s.len() > max_digits {
        return None;
    }
    s.chars().try_fold(0u32, |acc, c| Some(acc << 4 | u32::from(hex(c)?)))
}

/// One code as typed: spaces ignored, any case. `None` when it is none of the three kinds.
pub fn parse(code: &str) -> Option<Code> {
    let s: String = code.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    if let Some((a, v)) = s.split_once(':').or_else(|| s.split_once('=')) {
        let addr = hex_number(a, 4)? as u16;
        let value = hex_number(v, 2)? as u8;
        return Some(Code::Write(addr, value));
    }
    let d: Vec<u8> = s.chars().filter(|&c| c != '-').map(hex).collect::<Option<_>>()?;
    match d.len() {
        6 | 9 => {
            let value = d[0] << 4 | d[1];
            let addr = u16::from(d[5] ^ 0xF) << 12 | u16::from(d[2]) << 8 | u16::from(d[3]) << 4 | u16::from(d[4]);
            let compare = (d.len() == 9).then(|| (d[6] << 4 | d[8]).rotate_right(2) ^ 0xBA);
            Some(Code::Genie(Genie { addr, value, compare }))
        }
        8 => {
            let addr = u16::from(d[2]) << 12 | u16::from(d[3]) << 8 | u16::from(d[4]) << 4 | u16::from(d[5]);
            Some(Code::Write(addr, d[6] << 4 | d[7]))
        }
        _ => None,
    }
}

impl Cheats {
    /// The codes switched on (each may join several with `+`), in order.
    pub fn from_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> Cheats {
        let mut cheats = Cheats::default();
        for part in codes.into_iter().flat_map(|c| c.split('+')) {
            match parse(part) {
                Some(Code::Genie(g)) => cheats.genie.push(g),
                Some(Code::Write(a, v)) => cheats.writes.push((a, v)),
                None => {}
            }
        }
        cheats
    }

    pub fn is_empty(&self) -> bool {
        self.genie.is_empty() && self.writes.is_empty()
    }

    /// The byte the console sees when the cartridge answers `v` at `addr`: the first Game Genie
    /// code for that address whose compare byte (if it has one) matches, else the cartridge's own.
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
    fn a_game_genie_code_gives_its_byte_and_address() {
        // 3A6-5BF: byte 3A at ((F xor F) << 12) | 6 << 8 | 5 << 4 | B.
        assert_eq!(parse("3A6-5BF"), Some(Code::Genie(Genie { addr: 0x065B, value: 0x3A, compare: None })));
        // The sixth digit is the address's top digit, inverted: 0 there means $F000 and up.
        assert_eq!(parse("123-450"), Some(Code::Genie(Genie { addr: 0xF345, value: 0x12, compare: None })));
        // Lower case, no dashes and spaces are all the same code.
        assert_eq!(parse(" 3a65bf "), parse("3A6-5BF"));
    }

    #[test]
    fn the_long_form_carries_a_compare_byte() {
        // Compare from the seventh and ninth digits (2 and 6 here: $26), rotated right by two
        // ($89) and xor BA: $33. The eighth digit doesn't count.
        let Some(Code::Genie(g)) = parse("00A-17B-2A6") else { panic!("not read") };
        assert_eq!((g.addr, g.value, g.compare), (0x4A17, 0x00, Some(0x33)));
        assert_eq!(parse("00A-17B-2F6"), parse("00A-17B-2A6"));
        // Every compare byte comes out of some pair of digits.
        let seen: std::collections::BTreeSet<u8> = (0..16u8)
            .flat_map(|a| (0..16u8).map(move |b| format!("000-00F-{a:X}0{b:X}")))
            .filter_map(|c| match parse(&c) {
                Some(Code::Genie(g)) => g.compare,
                _ => None,
            })
            .collect();
        assert_eq!(seen.len(), 256);
    }

    #[test]
    fn pro_action_replay_and_raw_codes_write_a_byte() {
        assert_eq!(parse("00C0A0FF"), Some(Code::Write(0xC0A0, 0xFF)));
        assert_eq!(parse("00c0-a0ff"), Some(Code::Write(0xC0A0, 0xFF)));
        assert_eq!(parse("C000:FF"), Some(Code::Write(0xC000, 0xFF)));
        assert_eq!(parse("c001=1"), Some(Code::Write(0xC001, 0x01)));
    }

    #[test]
    fn anything_else_is_left_out() {
        for bad in ["", "12345", "1234567", "00C0A0FG", "C0000:FF", "C000:1FF", ":FF", "C000:", "XYZ-ABC"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
        let c = Cheats::from_codes(["nonsense", "C000:FF+3A6-5BF", "", "00C001-01"]);
        assert_eq!(c.writes, vec![(0xC000, 0xFF), (0xC001, 0x01)]);
        assert_eq!(c.genie.len(), 1);
    }

    #[test]
    fn a_game_genie_code_changes_its_address_only_while_the_compare_byte_is_there() {
        let c = Cheats::from_codes(["00A-17B-2A6"]);
        assert_eq!(c.read(0x4A17, 0x33), 0x00);
        assert_eq!(c.read(0x4A17, 0x34), 0x34);
        assert_eq!(c.read(0x4A18, 0x33), 0x33);
        let plain = Cheats::from_codes(["3A6-5BF"]);
        assert_eq!(plain.read(0x065B, 0x99), 0x3A);
        assert_eq!(plain.read(0x465B, 0x99), 0x99);
    }
}
