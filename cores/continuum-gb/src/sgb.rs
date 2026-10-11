//! Super Game Boy: the command packets a game sends through the joypad register, the SNES-side
//! palettes and attribute map that colour the Game Boy picture, multiplayer reads, screen masks,
//! and the border (tiles and map sent by VRAM transfer). Written from the public descriptions of
//! the SGB command set; the SNES program itself is not emulated, so sound commands are ignored.

use serde::{Deserialize, Serialize};

use crate::ppu::{H, W};

pub const BW: usize = 256;
pub const BH: usize = 224;
/// Where the Game Boy picture sits inside the border.
pub const GX: usize = 48;
pub const GY: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Transfer {
    None,
    Palettes,
    Attributes,
    TilesLow,
    TilesHigh,
    Border,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sgb {
    // Packet reception.
    bits: Vec<u8>,
    receiving: bool,
    bit_count: u16,
    byte: u8,
    packet: Vec<u8>,
    packets: Vec<u8>,
    packets_left: u8,
    last_write: u8,
    // Multiplayer.
    pub players: u8,
    pub player: u8,
    // Colour.
    /// The four palettes in use, 4 colours each, BGR555.
    pub pal: [[u16; 4]; 4],
    /// 20x18 cells, palette 0-3 each.
    pub attr: Vec<u8>,
    /// 512 palettes from PAL_TRN.
    pub sys_pal: Vec<u16>,
    /// 45 attribute files from ATTR_TRN, 90 bytes each.
    pub attr_files: Vec<u8>,
    /// 0 off, 1 freeze, 2 black, 3 colour 0.
    pub mask: u8,
    // Border.
    pub border_tiles: Vec<u8>,
    pub border_map: Vec<u16>,
    pub border_pal: Vec<u16>,
    pub has_border: bool,
    /// A VRAM transfer waiting for the next frame.
    pub pending: Transfer,
    /// Frames until a pending transfer is taken (a game shows the data for a frame or two).
    pending_wait: u8,
}

impl Default for Sgb {
    fn default() -> Self {
        // The palettes the SGB starts with before a game sends any (a warm cream-to-brown).
        let start = [0x67BF, 0x265B, 0x10B5, 0x2866];
        Sgb {
            bits: Vec::new(),
            receiving: false,
            bit_count: 0,
            byte: 0,
            packet: Vec::new(),
            packets: Vec::new(),
            packets_left: 0,
            last_write: 0x30,
            players: 1,
            player: 0,
            pal: [start; 4],
            attr: vec![0; 20 * 18],
            sys_pal: vec![0; 512 * 4],
            attr_files: vec![0; 45 * 90],
            mask: 0,
            border_tiles: vec![0; 256 * 32],
            border_map: vec![0; 32 * 32],
            border_pal: vec![0; 4 * 16],
            has_border: false,
            pending: Transfer::None,
            pending_wait: 0,
        }
    }
}

fn u16le(d: &[u8], i: usize) -> u16 {
    u16::from(d[i]) | u16::from(d[i + 1]) << 8
}

impl Sgb {
    /// A write to $FF00 (bits 4-5). Returns a log line for a finished command.
    pub fn joyp_write(&mut self, v: u8) -> Option<String> {
        let v = v & 0x30;
        let prev = self.last_write;
        self.last_write = v;
        // Multiplayer: the next controller is selected when P15 comes back up after a read of
        // the buttons, the way games step through the pads.
        if self.players > 1 && v == 0x30 && prev & 0x20 == 0 {
            self.player = (self.player + 1) % self.players;
        }
        match v {
            0x00 => {
                // Reset pulse: a packet begins.
                self.receiving = true;
                self.bit_count = 0;
                self.byte = 0;
                self.packet.clear();
                None
            }
            0x10 | 0x20 if self.receiving && prev == 0x30 || self.receiving && prev == 0x00 => {
                let bit = u8::from(v == 0x10);
                if self.bit_count == 128 {
                    // The stop bit (0) ends the packet.
                    self.receiving = false;
                    return self.packet_done();
                }
                self.byte |= bit << (self.bit_count % 8);
                self.bit_count += 1;
                if self.bit_count % 8 == 0 {
                    self.packet.push(self.byte);
                    self.byte = 0;
                }
                None
            }
            _ => None,
        }
    }

    /// The low nibble of $FF00 when neither button group is selected, in multiplayer mode.
    pub fn joyp_idle(&self) -> u8 {
        if self.players > 1 { 0x0F - self.player } else { 0x0F }
    }

    fn packet_done(&mut self) -> Option<String> {
        if self.packet.len() != 16 {
            return None;
        }
        if self.packets_left == 0 {
            let len = (self.packet[0] & 7).max(1);
            self.packets.clear();
            self.packets_left = len;
        }
        self.packets.extend_from_slice(&self.packet);
        self.packets_left -= 1;
        if self.packets_left == 0 {
            let data = std::mem::take(&mut self.packets);
            return self.command(&data);
        }
        None
    }

    fn set_pair(&mut self, a: usize, b: usize, d: &[u8]) {
        let c0 = u16le(d, 1);
        for p in &mut self.pal {
            p[0] = c0;
        }
        for i in 0..3 {
            self.pal[a][i + 1] = u16le(d, 3 + i * 2);
            self.pal[b][i + 1] = u16le(d, 9 + i * 2);
        }
    }

    fn command(&mut self, d: &[u8]) -> Option<String> {
        let cmd = d[0] >> 3;
        match cmd {
            0x00 => self.set_pair(0, 1, d),
            0x01 => self.set_pair(2, 3, d),
            0x02 => self.set_pair(0, 3, d),
            0x03 => self.set_pair(1, 2, d),
            0x04 => {
                // ATTR_BLK
                let n = usize::from(d[1]).min((d.len() - 2) / 6);
                for k in 0..n {
                    let b = &d[2 + k * 6..8 + k * 6];
                    let (ctl, pals) = (b[0], b[1]);
                    let (x1, y1, x2, y2) = (usize::from(b[2] & 31), usize::from(b[3] & 31), usize::from(b[4] & 31), usize::from(b[5] & 31));
                    let (mut ctl_in, mut ctl_line, ctl_out) = (ctl & 1 != 0, ctl & 2 != 0, ctl & 4 != 0);
                    // Inside only or outside only also colours the border line.
                    if ctl & 7 == 1 { ctl_line = true; }
                    if ctl & 7 == 4 { ctl_line = true; }
                    if ctl & 7 == 2 { ctl_in = false; }
                    for y in 0..18 {
                        for x in 0..20 {
                            let inside = x > x1 && x < x2 && y > y1 && y < y2;
                            let on_line = !inside && x >= x1 && x <= x2 && y >= y1 && y <= y2;
                            let p = if inside && ctl_in {
                                Some(pals & 3)
                            } else if on_line && ctl_line {
                                Some(if ctl & 7 == 1 { pals & 3 } else if ctl & 7 == 4 { (pals >> 4) & 3 } else { (pals >> 2) & 3 })
                            } else if !inside && !on_line && ctl_out {
                                Some((pals >> 4) & 3)
                            } else {
                                None
                            };
                            if let Some(p) = p {
                                self.attr[y * 20 + x] = p;
                            }
                        }
                    }
                }
            }
            0x05 => {
                // ATTR_LIN
                let n = usize::from(d[1]).min(d.len() - 2);
                for &b in &d[2..2 + n] {
                    let line = usize::from(b & 31);
                    let p = (b >> 5) & 3;
                    if b & 0x80 != 0 {
                        if line < 18 { (0..20).for_each(|x| self.attr[line * 20 + x] = p); }
                    } else if line < 20 {
                        (0..18).for_each(|y| self.attr[y * 20 + line] = p);
                    }
                }
            }
            0x06 => {
                // ATTR_DIV
                let b = d[1];
                let at = usize::from(d[2] & 31);
                let (low, high, on) = (b & 3, (b >> 2) & 3, (b >> 4) & 3);
                for y in 0..18 {
                    for x in 0..20 {
                        let c = if b & 0x40 != 0 { y } else { x };
                        self.attr[y * 20 + x] = match c.cmp(&at) {
                            std::cmp::Ordering::Less => high,
                            std::cmp::Ordering::Equal => on,
                            std::cmp::Ordering::Greater => low,
                        };
                    }
                }
            }
            0x07 => {
                // ATTR_CHR
                let (mut x, mut y) = (usize::from(d[1]).min(19), usize::from(d[2]).min(17));
                let n = usize::from(u16le(d, 3)).min(360);
                let vertical = d[5] & 1 != 0;
                for i in 0..n {
                    let Some(&byte) = d.get(6 + i / 4) else { break };
                    self.attr[y * 20 + x] = (byte >> (6 - (i % 4) * 2)) & 3;
                    if vertical {
                        y += 1;
                        if y == 18 { y = 0; x = (x + 1) % 20; }
                    } else {
                        x += 1;
                        if x == 20 { x = 0; y = (y + 1) % 18; }
                    }
                }
            }
            0x0A => {
                // PAL_SET
                for p in 0..4 {
                    let n = usize::from(u16le(d, 1 + p * 2) & 0x1FF);
                    for c in 0..4 {
                        self.pal[p][c] = self.sys_pal[n * 4 + c];
                    }
                }
                let c0 = self.pal[0][0];
                for p in &mut self.pal { p[0] = c0; }
                if d[9] & 0x80 != 0 {
                    self.apply_attr_file(usize::from(d[9] & 0x3F));
                }
                if d[9] & 0x40 != 0 {
                    self.mask = 0;
                }
            }
            0x0B => self.start(Transfer::Palettes),
            0x11 => {
                self.players = match d[1] & 3 { 1 => 2, 3 => 4, _ => 1 };
                self.player = 0;
            }
            0x13 => self.start(if d[1] & 1 != 0 { Transfer::TilesHigh } else { Transfer::TilesLow }),
            0x14 => self.start(Transfer::Border),
            0x15 => self.start(Transfer::Attributes),
            0x16 => {
                self.apply_attr_file(usize::from(d[1] & 0x3F));
                if d[1] & 0x40 != 0 {
                    self.mask = 0;
                }
            }
            0x17 => self.mask = d[1] & 3,
            _ => return Some(format!("SGB command {cmd:02X} ignored")),
        }
        None
    }

    fn start(&mut self, t: Transfer) {
        self.pending = t;
        self.pending_wait = 2;
    }

    fn apply_attr_file(&mut self, n: usize) {
        if n >= 45 {
            return;
        }
        let f = &self.attr_files[n * 90..n * 90 + 90];
        for i in 0..360 {
            self.attr[i] = (f[i / 4] >> (6 - (i % 4) * 2)) & 3;
        }
    }

    /// Called at each VBlank with the 4 KiB the screen shows (tile data in screen order).
    pub fn wants_transfer(&mut self) -> bool {
        if self.pending == Transfer::None {
            return false;
        }
        if self.pending_wait > 0 {
            self.pending_wait -= 1;
            return false;
        }
        true
    }

    pub fn take_transfer(&mut self, data: &[u8]) {
        match self.pending {
            Transfer::Palettes => {
                for i in 0..2048 {
                    self.sys_pal[i] = u16le(data, i * 2);
                }
            }
            Transfer::Attributes => self.attr_files.copy_from_slice(&data[..45 * 90]),
            Transfer::TilesLow => self.border_tiles[..4096].copy_from_slice(&data[..4096]),
            Transfer::TilesHigh => self.border_tiles[4096..].copy_from_slice(&data[..4096]),
            Transfer::Border => {
                for i in 0..32 * 32 {
                    self.border_map[i] = u16le(data, i * 2);
                }
                for i in 0..64 {
                    self.border_pal[i] = u16le(data, 0x800 + i * 2);
                }
                self.has_border = true;
            }
            Transfer::None => {}
        }
        self.pending = Transfer::None;
    }

    /// Colours the Game Boy picture (`shade` 0-3 per pixel) into `out` (160x144 XRGB).
    pub fn colour(&self, shade: &[u8], out: &mut [u32], to_rgb: impl Fn(u16) -> u32) {
        match self.mask {
            1 => return,
            2 => { out.iter_mut().for_each(|p| *p = 0); return; }
            3 => { let c = to_rgb(self.pal[0][0]); out.iter_mut().for_each(|p| *p = c); return; }
            _ => {}
        }
        for y in 0..H {
            for x in 0..W {
                let p = usize::from(self.attr[(y / 8) * 20 + x / 8]);
                out[y * W + x] = to_rgb(self.pal[p][usize::from(shade[y * W + x] & 3)]);
            }
        }
    }

    /// Draws the 256x224 picture: the backdrop, the Game Boy picture at (48, 40), and the border
    /// over it.
    pub fn compose(&self, game: &[u32], out: &mut Vec<u32>, to_rgb: impl Fn(u16) -> u32) {
        out.clear();
        out.resize(BW * BH, to_rgb(self.pal[0][0]));
        for y in 0..H {
            out[(GY + y) * BW + GX..(GY + y) * BW + GX + W].copy_from_slice(&game[y * W..(y + 1) * W]);
        }
        if !self.has_border {
            return;
        }
        for ty in 0..28 {
            for tx in 0..32 {
                let e = self.border_map[ty * 32 + tx];
                let tile = usize::from(e & 0xFF);
                let pal = usize::from((e >> 10) & 7).saturating_sub(4).min(3);
                let (hf, vf) = (e & 0x4000 != 0, e & 0x8000 != 0);
                let t = &self.border_tiles[tile * 32..tile * 32 + 32];
                for r in 0..8 {
                    let row = if vf { 7 - r } else { r };
                    for c in 0..8 {
                        let bit = if hf { c } else { 7 - c };
                        let idx = ((t[row * 2] >> bit) & 1)
                            | ((t[row * 2 + 1] >> bit) & 1) << 1
                            | ((t[16 + row * 2] >> bit) & 1) << 2
                            | ((t[16 + row * 2 + 1] >> bit) & 1) << 3;
                        if idx == 0 {
                            continue;
                        }
                        let (x, y) = (tx * 8 + c, ty * 8 + r);
                        out[y * BW + x] = to_rgb(self.border_pal[pal * 16 + usize::from(idx)]);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(s: &mut Sgb, bytes: &[u8]) {
        for chunk in bytes.chunks(16) {
            s.joyp_write(0x00);
            s.joyp_write(0x30);
            for &b in chunk {
                for i in 0..8 {
                    s.joyp_write(if b >> i & 1 != 0 { 0x10 } else { 0x20 });
                    s.joyp_write(0x30);
                }
            }
            s.joyp_write(0x20);
            s.joyp_write(0x30);
        }
    }

    #[test]
    fn pal01_and_mlt_req_and_attr_blk() {
        let mut s = Sgb::default();
        let mut p = [0u8; 16];
        p[0] = 0x00 << 3 | 1;
        p[1] = 0x1F; // colour 0 = red
        p[3] = 0xE0; p[4] = 0x03; // pal 0 colour 1 = green
        send(&mut s, &p);
        assert_eq!(s.pal[0][0], 0x001F);
        assert_eq!(s.pal[3][0], 0x001F);
        assert_eq!(s.pal[0][1], 0x03E0);
        let mut m = [0u8; 16];
        m[0] = 0x11 << 3 | 1;
        m[1] = 1;
        send(&mut s, &m);
        assert_eq!(s.players, 2);
        let first = s.joyp_idle();
        s.joyp_write(0x10);
        s.joyp_write(0x30);
        assert_ne!(s.joyp_idle(), first);
        let mut b = [0u8; 16];
        b[0] = 0x04 << 3 | 1;
        b[1] = 1;
        b[2] = 1; // inside
        b[3] = 2; // palette 2
        b[4] = 0; b[5] = 0; b[6] = 5; b[7] = 5;
        send(&mut s, &b);
        assert_eq!(s.attr[2 * 20 + 2], 2);
        assert_eq!(s.attr[10 * 20 + 10], 0);
    }
}
