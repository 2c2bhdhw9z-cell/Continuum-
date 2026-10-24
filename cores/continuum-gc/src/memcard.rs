//! The memory card in slot A (gamecube.json G9): a Nintendo 251-block card (16 Mbit, 2 MB) on
//! EXI channel 0, device 0.
//!
//! A card is a flash chip behind a small command set. While it is selected, the first byte is a
//! command and the bytes after it are its arguments and data:
//!
//! - `00` card ID: a dummy byte, then the ID (`00000010`: 16 Mbit, 8 KB sectors, 4 latency bytes).
//! - `52` read: a 4-byte address, 4 latency bytes, then the data from that address on.
//! - `F1` sector erase: a 2-byte address; the 8 KB sector becomes FF when the card is deselected.
//! - `F2` page program: a 4-byte address and up to 128 bytes, written when the card is deselected.
//! - `F4` chip erase: the whole card becomes FF.
//! - `83` status, `89` clear status, `81` interrupt on or off, `85` the chip's maker code.
//!
//! An erase or a program finishes a moment after the deselect: the card is busy until then, and
//! then raises its interrupt line (when the program switched it on with `81 01`), which the EXI
//! channel reports as EXIINT. The card library answers it by reading and clearing the status.
//!
//! The card starts unlocked. A real card is locked at power-on and the console unlocks it with a
//! challenge worked out on the DSP; for a card that is already unlocked, the library instead
//! checks the card's serial number against the flash ID the console keeps in SRAM. Here SRAM is
//! always given the flash ID that the card in the slot was formatted with ([`flash_id_of`]), so
//! any card passes: a blank one made here, or a card image copied from a console.
//!
//! The card's bytes are the game's libretro save RAM: the app keeps one card per game, as the
//! game's .srm file. Save states keep the card's command state ([`Port`]), never its bytes.

use serde::{Deserialize, Serialize};

/// Bytes on the card: 16 Mbit.
pub const CARD_BYTES: usize = 2 << 20;
/// The card ID: the size in megabits (bits 2-7), sector size code 0 (8 KB, bits 11-13) and
/// latency code 0 (4 bytes, bits 8-10).
pub const CARD_ID: u32 = 0x0000_0010;
/// An erase sector, which is also a block of the card's file system.
pub const SECTOR_BYTES: usize = 8192;
/// Blocks on the card; the first five are the file system's own, so 251 are free for saves.
pub const BLOCKS: usize = CARD_BYTES / SECTOR_BYTES;
const SYSTEM_BLOCKS: usize = 5;
/// Reads and programs move through a 512-byte page, wrapping inside it.
const PAGE_BYTES: u32 = 512;
const PROGRAM_BYTES: usize = 128;
/// The bytes between a read command's address and its data (latency code 0).
const READ_LATENCY: u32 = 4;
/// What the maker code command (`85`) answers: Macronix, the chip in Nintendo's cards.
const MAKER_ID: u16 = 0xC221;

pub const ST_BUSY: u8 = 0x80;
pub const ST_UNLOCKED: u8 = 0x40;
pub const ST_ERASE_ERROR: u8 = 0x10;
pub const ST_PROGRAM_ERROR: u8 = 0x08;
pub const ST_READY: u8 = 0x01;

/// Video lines an erase or a program takes, about 64 microseconds each. Far quicker than a real
/// chip (saving is not made to wait), and well inside the library's timeouts (100 ms for a
/// program, 2 s for an erase).
const DONE_LINES: u32 = 1;

/// The card's state between bytes: the part of the card a save state keeps.
#[derive(Clone, Serialize, Deserialize)]
pub struct Port {
    /// The command being received, and how many bytes of it have passed (0: the next byte is a
    /// new command).
    command: u8,
    position: u32,
    address: u32,
    status: u8,
    /// The interrupt switched on (command `81`), and the interrupt line itself.
    irq_on: bool,
    irq_line: bool,
    /// A page program's data, written when the card is deselected.
    program: Vec<u8>,
    /// Video lines until the erase or program in progress is done (0: none is).
    busy_lines: u32,
}

impl Default for Port {
    fn default() -> Self {
        Port {
            command: 0,
            position: 0,
            address: 0,
            status: ST_UNLOCKED | ST_READY,
            irq_on: false,
            irq_line: false,
            program: Vec::new(),
            busy_lines: 0,
        }
    }
}

impl Port {
    /// The card was selected: the next byte is a command.
    pub fn select(&mut self) {
        self.position = 0;
    }

    /// One byte each way while the card is selected: `byte` in, the card's answer out.
    pub fn exchange(&mut self, card: &[u8], byte: u8) -> u8 {
        let pos = self.position;
        self.position = pos.saturating_add(1);
        if pos == 0 {
            self.command = byte;
            self.program.clear();
            if byte == 0x89 {
                // Clear status: the error bits off, ready, and the interrupt line down.
                self.status &= !(ST_ERASE_ERROR | ST_PROGRAM_ERROR);
                self.status |= ST_READY;
                self.irq_line = false;
            }
            return 0xFF;
        }
        match self.command {
            // A dummy byte, then the ID, then zeros.
            0x00 => match pos {
                1 => 0x80,
                2..=5 => (CARD_ID >> (8 * (5 - pos))) as u8,
                _ => 0,
            },
            0x52 => {
                if pos <= 4 {
                    self.address_byte(pos, byte);
                    0xFF
                } else if pos < 5 + READ_LATENCY {
                    0xFF
                } else {
                    let b = card.get(self.address as usize & (CARD_BYTES - 1)).copied().unwrap_or(0xFF);
                    self.address = next_in_page(self.address);
                    b
                }
            }
            0x83 => self.status,
            // A dummy byte, then the code, high byte first.
            0x85 => {
                if pos > 1 && pos & 1 == 1 {
                    MAKER_ID as u8
                } else {
                    (MAKER_ID >> 8) as u8
                }
            }
            0x81 => {
                if pos == 1 {
                    self.irq_on = byte & 1 != 0;
                }
                0xFF
            }
            0xF1 => {
                if pos <= 2 {
                    self.address_byte(pos, byte);
                }
                0xFF
            }
            0xF2 => {
                if pos <= 4 {
                    self.address_byte(pos, byte);
                } else {
                    // The page buffer is 128 bytes; more wrap around onto its start.
                    let k = (pos as usize - 5) % PROGRAM_BYTES;
                    if k < self.program.len() {
                        self.program[k] = byte;
                    } else {
                        self.program.push(byte);
                    }
                }
                0xFF
            }
            _ => 0xFF,
        }
    }

    /// Address bytes: 7 bits of 128 KB, 8 of 512 bytes, 2 of 128 bytes, 7 bits of the byte.
    fn address_byte(&mut self, pos: u32, byte: u8) {
        let b = byte as u32;
        match pos {
            1 => self.address = (b & 0x7F) << 17,
            2 => self.address |= b << 9,
            3 => self.address |= (b & 3) << 7,
            _ => self.address |= b & 0x7F,
        }
    }

    /// The card was deselected: an erase or program that was sent whole happens now, and the
    /// card is busy until it is done.
    pub fn deselect(&mut self, card: &mut [u8]) {
        let pos = self.position;
        let started = match self.command {
            0xF1 if pos >= 3 => {
                let at = self.address as usize & (CARD_BYTES - 1) & !(SECTOR_BYTES - 1);
                if let Some(sector) = card.get_mut(at..at + SECTOR_BYTES) {
                    sector.fill(0xFF);
                }
                true
            }
            0xF2 if pos >= 5 => {
                let mut at = self.address;
                for b in &self.program {
                    if let Some(c) = card.get_mut(at as usize & (CARD_BYTES - 1)) {
                        *c = *b;
                    }
                    at = next_in_page(at);
                }
                true
            }
            0xF4 if pos >= 3 => {
                card.fill(0xFF);
                true
            }
            _ => false,
        };
        // Done with this command: bytes after a deselect start a new one.
        self.command = 0;
        self.program.clear();
        if started {
            self.status = (self.status | ST_BUSY) & !ST_READY;
            self.busy_lines = DONE_LINES;
        }
    }

    /// One video line passed: an erase or program in progress may be done.
    pub fn line(&mut self) {
        if self.busy_lines == 0 {
            return;
        }
        self.busy_lines -= 1;
        if self.busy_lines == 0 {
            self.status = (self.status | ST_READY) & !ST_BUSY;
            self.irq_line = true;
        }
    }

    /// Whether the card is asking for an interrupt: its line is up and switched on.
    pub fn irq(&self) -> bool {
        self.irq_on && self.irq_line
    }

    pub fn status(&self) -> u8 {
        self.status
    }
}

/// The next address of a read or program, which wraps inside its 512-byte page.
fn next_in_page(a: u32) -> u32 {
    (a & !(PAGE_BYTES - 1)) | (a.wrapping_add(1) & (PAGE_BYTES - 1))
}

/// The card library's number sequence for serial numbers (`x * 1103515245 + 12345`, top bits).
/// The library works in signed 64-bit numbers; only bits that both ways agree on are ever used.
fn next_rand(x: u64) -> u64 {
    x.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039) >> 16
}

/// The card's serial number for a flash ID and format time, as the library makes it.
fn serial_of(flash_id: &[u8; 12], time: u64) -> [u8; 12] {
    let mut serial = [0u8; 12];
    let mut rand = time;
    for (s, f) in serial.iter_mut().zip(flash_id) {
        rand = next_rand(rand);
        *s = f.wrapping_add(rand as u8);
        rand = next_rand(rand) & 0x7FFF;
    }
    serial
}

/// The flash ID a console must hold in SRAM for this card to check out: the card's serial
/// number is the flash ID plus a sequence seeded by its format time, so taking the sequence
/// away gives the ID back.
pub fn flash_id_of(card: &[u8]) -> [u8; 12] {
    let mut id = [0u8; 12];
    if card.len() < 20 {
        return id;
    }
    let time = u64::from_be_bytes(card[12..20].try_into().unwrap());
    let mut rand = time;
    for (k, f) in id.iter_mut().enumerate() {
        rand = next_rand(rand);
        *f = card[k].wrapping_sub(rand as u8);
        rand = next_rand(rand) & 0x7FFF;
    }
    id
}

/// The card library's checksum: the sum of the big-endian halfwords, and the sum of each one
/// inverted; a sum of FFFF is stored as 0.
pub fn checksum(bytes: &[u8]) -> (u16, u16) {
    let (mut sum, mut inv) = (0u16, 0u16);
    for w in bytes.chunks_exact(2) {
        let v = u16::from_be_bytes([w[0], w[1]]);
        sum = sum.wrapping_add(v);
        inv = inv.wrapping_add(!v);
    }
    let fix = |v: u16| if v == 0xFFFF { 0 } else { v };
    (fix(sum), fix(inv))
}

/// What a blank card is formatted with.
pub struct Format {
    /// The format time, in time-base ticks.
    pub time: u64,
    /// From SRAM, as the library copies them into the header.
    pub counter_bias: u32,
    pub language: u8,
    /// The video interface's DTV status (bit 1: a Japanese console).
    pub dtv: u16,
    /// 0 for ANSI, 1 for Shift-JIS (a Japanese console): the library refuses a card formatted
    /// for the other kind.
    pub encoding: u16,
}

/// A blank card, formatted the way the card library formats one: the header (block 0), two
/// copies of the empty directory (blocks 1 and 2) and two of the empty block table (blocks 3
/// and 4), all with their checksums; the rest erased (FF). The serial number is made from a
/// flash ID of zeros.
pub fn blank(f: &Format) -> Vec<u8> {
    let mut card = vec![0xFFu8; CARD_BYTES];
    let h = &mut card[..SECTOR_BYTES];
    h[..12].copy_from_slice(&serial_of(&[0; 12], f.time));
    h[12..20].copy_from_slice(&f.time.to_be_bytes());
    h[20..24].copy_from_slice(&f.counter_bias.to_be_bytes());
    h[24..28].copy_from_slice(&(f.language as u32).to_be_bytes());
    h[28..32].copy_from_slice(&(f.dtv as u32).to_be_bytes());
    h[0x20..0x22].copy_from_slice(&0u16.to_be_bytes()); // device ID
    h[0x22..0x24].copy_from_slice(&((CARD_ID & 0xFC) as u16).to_be_bytes()); // megabits
    h[0x24..0x26].copy_from_slice(&f.encoding.to_be_bytes());
    let (sum, inv) = checksum(&h[..0x1FC]);
    h[0x1FC..0x1FE].copy_from_slice(&sum.to_be_bytes());
    h[0x1FE..0x200].copy_from_slice(&inv.to_be_bytes());
    for copy in 0..2 {
        // The directory: 127 empty entries (FF) and, in its last bytes, which copy is newer.
        let d = &mut card[(1 + copy) * SECTOR_BYTES..(2 + copy) * SECTOR_BYTES];
        d[0x1FFA..0x1FFC].copy_from_slice(&(copy as u16).to_be_bytes());
        let (sum, inv) = checksum(&d[..0x1FFC]);
        d[0x1FFC..0x1FFE].copy_from_slice(&sum.to_be_bytes());
        d[0x1FFE..].copy_from_slice(&inv.to_be_bytes());
    }
    for copy in 0..2 {
        // The block table: checksums, which copy is newer, the free blocks, the last block
        // handed out, then one entry per block (0: free).
        let t = &mut card[(3 + copy) * SECTOR_BYTES..(4 + copy) * SECTOR_BYTES];
        t.fill(0);
        t[4..6].copy_from_slice(&(copy as u16).to_be_bytes());
        t[6..8].copy_from_slice(&((BLOCKS - SYSTEM_BLOCKS) as u16).to_be_bytes());
        t[8..10].copy_from_slice(&((SYSTEM_BLOCKS - 1) as u16).to_be_bytes());
        let (sum, inv) = checksum(&t[4..]);
        t[0..2].copy_from_slice(&sum.to_be_bytes());
        t[2..4].copy_from_slice(&inv.to_be_bytes());
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be16(b: &[u8], at: usize) -> u16 {
        u16::from_be_bytes([b[at], b[at + 1]])
    }

    /// Sends `bytes` with the card selected and returns its answers, then deselects it.
    fn command(port: &mut Port, card: &mut [u8], bytes: &[u8]) -> Vec<u8> {
        port.select();
        let out = bytes.iter().map(|b| port.exchange(card, *b)).collect();
        port.deselect(card);
        out
    }

    fn fmt(time: u64, encoding: u16) -> Format {
        Format { time, counter_bias: 0, language: 0, dtv: 0, encoding }
    }

    /// The checks the card library makes when it mounts a card (VerifyID, VerifyDir,
    /// VerifyFAT), written from its description, against the flash ID in SRAM and the
    /// console's font encoding. Ok(free blocks) or the reason it would refuse.
    fn library_accepts(card: &[u8], sram_flash_id: &[u8; 12], encoding: u16) -> Result<u16, String> {
        let h = &card[..512];
        if be16(h, 0x20) != 0 || be16(h, 0x22) != 16 {
            return Err("device or size".into());
        }
        if checksum(&h[..0x1FC]) != (be16(h, 0x1FC), be16(h, 0x1FE)) {
            return Err("header checksum".into());
        }
        // The serial number, signed 64-bit arithmetic like the library's own.
        let mut rand = i64::from_be_bytes(h[12..20].try_into().unwrap());
        for (i, f) in sram_flash_id.iter().enumerate() {
            rand = rand.wrapping_mul(1_103_515_245).wrapping_add(12_345) >> 16;
            if h[i] != f.wrapping_add(rand as u8) {
                return Err(format!("serial byte {i}"));
            }
            rand = (rand.wrapping_mul(1_103_515_245).wrapping_add(12_345) >> 16) & 0x7FFF;
        }
        if be16(h, 0x24) != encoding {
            return Err("encoding".into());
        }
        for copy in 0..2 {
            let d = &card[(1 + copy) * SECTOR_BYTES..(2 + copy) * SECTOR_BYTES];
            if checksum(&d[..0x1FFC]) != (be16(d, 0x1FFC), be16(d, 0x1FFE)) {
                return Err(format!("directory {copy} checksum"));
            }
        }
        let mut free = None;
        for copy in 0..2 {
            let t = &card[(3 + copy) * SECTOR_BYTES..(4 + copy) * SECTOR_BYTES];
            if checksum(&t[4..]) != (be16(t, 0), be16(t, 2)) {
                return Err(format!("block table {copy} checksum"));
            }
            let counted = (SYSTEM_BLOCKS..BLOCKS).filter(|b| be16(t, 2 * b) == 0).count() as u16;
            if counted != be16(t, 6) {
                return Err(format!("block table {copy} free count"));
            }
            free = Some(counted);
        }
        Ok(free.unwrap())
    }

    #[test]
    fn answers_its_id_like_a_nintendo_card() {
        let (mut p, mut card) = (Port::default(), blank(&fmt(0, 0)));
        // The library: two bytes written (00 00), four read.
        let out = command(&mut p, &mut card, &[0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(out[1], 0x80);
        assert_eq!(u32::from_be_bytes(out[2..6].try_into().unwrap()), CARD_ID);
        assert_eq!(&out[6..], &[0, 0]);
        let out = command(&mut p, &mut card, &[0x85, 0, 0, 0]);
        assert_eq!(&out[2..], &[0xC2, 0x21]);
    }

    #[test]
    fn status_starts_unlocked_and_ready() {
        let (mut p, mut card) = (Port::default(), blank(&fmt(0, 0)));
        assert_eq!(command(&mut p, &mut card, &[0x83, 0])[1], ST_UNLOCKED | ST_READY);
    }

    #[test]
    fn reads_after_the_latency_bytes_and_wraps_in_the_page() {
        let mut card = vec![0u8; CARD_BYTES];
        for (k, b) in card.iter_mut().enumerate() {
            *b = (k * 7 + k / 512) as u8;
        }
        let mut p = Port::default();
        let addr = 0x12_3400 + 0x1F0;
        let cmd = [0x52, ((addr >> 17) & 0x7F) as u8, ((addr >> 9) & 0xFF) as u8, ((addr >> 7) & 3) as u8, (addr & 0x7F) as u8];
        let mut send = cmd.to_vec();
        send.extend([0u8; 4]);
        send.extend([0u8; 32]);
        let out = command(&mut p, &mut card, &send);
        let page = addr & !0x1FF;
        for k in 0..32 {
            let want = page + ((addr + k) & 0x1FF);
            assert_eq!(out[9 + k], card[want], "byte {k}");
        }
    }

    #[test]
    fn erase_then_program_then_read_back() {
        let mut card = vec![0x55u8; CARD_BYTES];
        let mut p = Port::default();
        command(&mut p, &mut card, &[0x81, 1]);
        let sector = 9 * SECTOR_BYTES;
        command(&mut p, &mut card, &[0xF1, (sector >> 17) as u8, (sector >> 9) as u8]);
        assert!(card[sector..sector + SECTOR_BYTES].iter().all(|b| *b == 0xFF), "erased");
        assert_eq!(card[sector - 1], 0x55, "the sector before untouched");
        assert_eq!(card[sector + SECTOR_BYTES], 0x55, "the sector after untouched");
        // Busy until the next video line, then ready and asking for an interrupt.
        assert_eq!(p.status() & (ST_BUSY | ST_READY), ST_BUSY);
        assert!(!p.irq());
        p.line();
        assert_eq!(p.status() & (ST_BUSY | ST_READY), ST_READY);
        assert!(p.irq());
        // Clear status puts the line down.
        command(&mut p, &mut card, &[0x89]);
        assert!(!p.irq());
        let at = sector + 0x280;
        let mut send = vec![0xF2, (at >> 17) as u8 & 0x3F, (at >> 9) as u8, (at >> 7) as u8 & 3, at as u8 & 0x7F];
        let data: Vec<u8> = (0..128).map(|k| (k * 3 + 1) as u8).collect();
        send.extend(&data);
        command(&mut p, &mut card, &send);
        assert_eq!(&card[at..at + 128], &data[..]);
        assert_eq!(card[at - 1], 0xFF);
        assert_eq!(card[at + 128], 0xFF);
        p.line();
        assert!(p.irq());
    }

    #[test]
    fn no_interrupt_unless_switched_on() {
        let mut card = vec![0u8; CARD_BYTES];
        let mut p = Port::default();
        command(&mut p, &mut card, &[0xF1, 0, 0x10]);
        p.line();
        assert!(!p.irq(), "off");
        // Switched on while the line is still up: now it asks.
        command(&mut p, &mut card, &[0x81, 1]);
        assert!(p.irq());
        command(&mut p, &mut card, &[0x81, 0]);
        assert!(!p.irq());
    }

    #[test]
    fn a_command_cut_short_changes_nothing() {
        let mut card = vec![0x11u8; CARD_BYTES];
        let mut p = Port::default();
        command(&mut p, &mut card, &[0xF1, 0]);
        command(&mut p, &mut card, &[0xF2, 0, 0, 0]);
        assert!(card.iter().all(|b| *b == 0x11));
        assert_eq!(p.status() & ST_BUSY, 0);
    }

    #[test]
    fn a_blank_card_mounts_with_251_free_blocks() {
        for (time, enc) in [(0u64, 0u16), (0x0123_4567_89AB_CDEF, 0), (0x8765_4321_0FED_CBA9, 1)] {
            let card = blank(&fmt(time, enc));
            assert_eq!(flash_id_of(&card), [0; 12]);
            assert_eq!(library_accepts(&card, &[0; 12], enc), Ok(251), "time {time:X}");
            assert!(library_accepts(&card, &[0; 12], enc ^ 1).is_err(), "the other encoding is refused");
            let mut wrong = [0u8; 12];
            wrong[5] = 1;
            assert!(library_accepts(&card, &wrong, enc).is_err(), "another console's flash ID is refused");
        }
    }

    #[test]
    fn any_card_checks_out_with_the_flash_id_taken_from_it() {
        // A card a console with some other flash ID formatted.
        let flash = *b"\x12\x34\x56\x78\x9A\xBC\xDE\xF0\x0F\x1E\x2D\x3C";
        let time = 0x0000_1234_5678_9ABC;
        let mut card = blank(&fmt(time, 0));
        card[..12].copy_from_slice(&serial_of(&flash, time));
        let (sum, inv) = checksum(&card[..0x1FC]);
        card[0x1FC..0x1FE].copy_from_slice(&sum.to_be_bytes());
        card[0x1FE..0x200].copy_from_slice(&inv.to_be_bytes());
        assert!(library_accepts(&card, &[0; 12], 0).is_err());
        assert_eq!(flash_id_of(&card), flash);
        assert_eq!(library_accepts(&card, &flash_id_of(&card), 0), Ok(251));
    }

    // ---------------------------------------------------------------- through the EXI registers

    use crate::bus::Bus;
    use crate::hw::Hw;

    /// Channel 0's registers: parameters (CSR), DMA address and length, control, data.
    const EXI: u32 = 0x0C00_6800;

    /// The library's EXISelect: chip select for `dev`, 16 MHz, the interrupt masks kept.
    fn select(hw: &mut Hw, dev: u32) {
        let csr = hw.read32(EXI) & 0x405;
        hw.write32(EXI, csr | (0x80 << dev) | (4 << 4));
    }

    fn deselect(hw: &mut Hw) {
        let csr = hw.read32(EXI) & 0x405;
        hw.write32(EXI, csr);
    }

    /// EXIImmEx writing: at most four bytes a transfer.
    fn imm_write(hw: &mut Hw, bytes: &[u8]) {
        for chunk in bytes.chunks(4) {
            let mut w = [0u8; 4];
            w[..chunk.len()].copy_from_slice(chunk);
            hw.write32(EXI + 0x10, u32::from_be_bytes(w));
            hw.write32(EXI + 0x0C, ((chunk.len() as u32 - 1) << 4) | (1 << 2) | 1);
        }
    }

    fn imm_read(hw: &mut Hw, len: usize) -> Vec<u8> {
        hw.write32(EXI + 0x0C, ((len as u32 - 1) << 4) | 1);
        hw.read32(EXI + 0x10).to_be_bytes()[..len].to_vec()
    }

    fn dma(hw: &mut Hw, addr: u32, len: u32, write: bool) {
        hw.write32(EXI + 0x04, addr);
        hw.write32(EXI + 0x08, len);
        let rw = if write { 1 } else { 0 };
        hw.write32(EXI + 0x0C, (rw << 2) | 2 | 1);
    }

    fn address(cmd: u8, a: usize) -> Vec<u8> {
        vec![cmd, ((a >> 17) & 0x7F) as u8, ((a >> 9) & 0xFF) as u8, ((a >> 7) & 3) as u8, (a & 0x7F) as u8]
    }

    #[test]
    fn the_card_answers_on_the_expansion_bus_as_the_library_expects() {
        let mut hw = Hw::new(false);
        assert_ne!(hw.read32(EXI) & 0x1000, 0, "slot A has a card in it");
        assert_eq!(hw.read32(EXI + 0x14) & 0x1000, 0, "slot B is empty");
        // EXIGetID.
        select(&mut hw, 0);
        imm_write(&mut hw, &[0, 0]);
        assert_eq!(imm_read(&mut hw, 4), CARD_ID.to_be_bytes());
        deselect(&mut hw);
        // __CARDReadSegment: the command, the latency bytes, then a 512-byte DMA.
        select(&mut hw, 0);
        imm_write(&mut hw, &address(0x52, 0x200));
        imm_write(&mut hw, &[0; 4]);
        dma(&mut hw, 0x1000, 512, false);
        deselect(&mut hw);
        assert_eq!(&hw.mem1[0x1000..0x1200], &hw.card[0x200..0x400]);
        // __CARDEnableInterrupt, then the EXI interrupt unmasked.
        select(&mut hw, 0);
        imm_write(&mut hw, &[0x81, 1]);
        deselect(&mut hw);
        hw.write32(EXI, 1);
        // __CARDEraseSector: done (EXIINT) one video line after the deselect.
        let sector = 6 * SECTOR_BYTES;
        select(&mut hw, 0);
        imm_write(&mut hw, &address(0xF1, sector)[..3]);
        deselect(&mut hw);
        assert_eq!(hw.read32(EXI) & 2, 0, "not done yet");
        hw.vi_line();
        assert_ne!(hw.read32(EXI) & 2, 0, "EXIINT");
        assert_ne!(hw.cause() & crate::hw::INT_EXI, 0, "the EXI interrupt is asked for");
        hw.write32(EXI, 1 | 2);
        assert_eq!(hw.read32(EXI) & 2, 0, "cleared by writing 1");
        assert!(hw.card[sector..sector + SECTOR_BYTES].iter().all(|b| *b == 0xFF));
        // __CARDExiHandler: status, then clear status.
        select(&mut hw, 0);
        imm_write(&mut hw, &[0x83]);
        assert_eq!(imm_read(&mut hw, 1)[0] & (ST_READY | ST_BUSY | ST_ERASE_ERROR | ST_PROGRAM_ERROR), ST_READY);
        deselect(&mut hw);
        select(&mut hw, 0);
        imm_write(&mut hw, &[0x89]);
        deselect(&mut hw);
        // __CARDWritePage: the command, then 128 bytes by DMA.
        let data: Vec<u8> = (0..128).map(|k| (k as u8).wrapping_mul(37)).collect();
        hw.dma_write(0x3000, &data);
        select(&mut hw, 0);
        imm_write(&mut hw, &address(0xF2, sector + 0x180));
        dma(&mut hw, 0x3000, 128, true);
        deselect(&mut hw);
        hw.vi_line();
        assert_eq!(&hw.card[sector + 0x180..sector + 0x200], &data[..]);
        assert_ne!(hw.read32(EXI) & 2, 0, "EXIINT again");
    }

    #[test]
    fn sram_carries_the_flash_id_of_the_card_in_the_slot() {
        let mut hw = Hw::new(false);
        let flash = [9u8, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0xAA, 0x55];
        let time = 0x0000_0BAD_CAFE_F00D;
        hw.card[..12].copy_from_slice(&serial_of(&flash, time));
        hw.card[12..20].copy_from_slice(&time.to_be_bytes());
        // ReadSram: the address word, then 64 bytes by DMA.
        select(&mut hw, 1);
        imm_write(&mut hw, &0x2000_0100u32.to_be_bytes());
        dma(&mut hw, 0x2000, 64, false);
        deselect(&mut hw);
        let sram = hw.mem1[0x2000..0x2040].to_vec();
        assert_eq!(&sram[0x14..0x20], &flash);
        let sum = flash.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        assert_eq!(sram[0x3A], !sum, "slot A's flash ID checksum");
        assert_eq!(sram[0x3B], 0xFF, "slot B: zeros, checksum FF");
        assert_eq!((sram[0x12], sram[0x13]), (0, 0x2C), "English; stereo");
        let (mut s, mut i) = (0u16, 0u16);
        for w in sram[0x0C..0x14].chunks(2) {
            let v = u16::from_be_bytes([w[0], w[1]]);
            s = s.wrapping_add(v);
            i = i.wrapping_add(!v);
        }
        assert_eq!((be16(&sram, 0), be16(&sram, 2)), (s, i), "SRAM checksums");
    }

    #[test]
    fn checksum_never_stores_ffff() {
        assert_eq!(checksum(&[0xFF, 0xFF]), (0, 0));
        assert_eq!(checksum(&[0x00, 0x00]), (0, 0));
        assert_eq!(checksum(&[0x12, 0x34, 0x00, 0x01]), (0x1235, 0xEDC9));
    }
}
