//! The Game Boy Printer (Pan Docs, "Game Boy Printer").
//!
//! A serial peripheral: the game sends it packets and then clocks out two `$00`
//! bytes to read back the ACK (`$81`) and a status byte. While a packet is being
//! sent the printer answers `$00` for every byte, which the game ignores.
//!
//! ```text
//! Packet:  $88 $33 | command | compression | length (LE) | data | checksum (LE)
//! Reply:   $00 for every packet byte, then $81, then the status byte.
//! ```
//!
//! The checksum is the sum of every byte except the magic and the checksum itself.
//! Commands: `$01` initialise (clears the buffer), `$02` print (4 bytes: sheets,
//! margins, palette, exposure), `$04` data (up to `$280` bytes = 160x16 pixels in
//! 2BPP tiles; several packets make a picture), `$08` break, `$0F` inquiry.
//! The image buffer holds 8 KiB, so a picture is at most 160x200 pixels.
//!
//! Status bits: 0 checksum error, 1 printing, 2 image data full, 3 unprocessed
//! data (ready to print), 4 packet error, 5 paper jam, 6 other error, 7 low battery.
//!
//! The printer is a peripheral, not part of the Game Boy, so it is `#[serde(skip)]`
//! on the bus and is carried over a state load instead of being saved in it.

/// The printer's image is 160 pixels wide.
pub const PRINTER_WIDTH: usize = 160;
/// One data packet holds 160x16 pixels: 20x2 tiles of 16 bytes.
pub const PRINTER_PACKET_BYTES: usize = 0x280;
/// The printer's buffer RAM: 8 KiB.
pub const PRINTER_BUFFER_BYTES: usize = 8192;

/// Where the printer is in a packet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    Magic1,
    Command,
    Compression,
    LenLo,
    LenHi,
    Data,
    ChecksumLo,
    ChecksumHi,
    /// The packet is complete; the next transfer clocks out `$81`.
    Ack,
    /// The ACK is out; the next transfer clocks out the status byte.
    Status,
}

#[derive(Clone, Debug, Default)]
pub struct Printer {
    /// Whether a printer is attached to the link port. Off by default, so the
    /// serial port behaves exactly as before (reads `$FF`) until one is attached.
    pub present: bool,
    state: State,
    command: u8,
    compressed: bool,
    len: usize,
    data: Vec<u8>,
    checksum: u16,
    checksum_recv: u16,
    /// The byte the next transfer clocks in (`$00` during a packet, `$81` for the
    /// ACK, the status byte after it).
    reply: u8,
    /// The byte after the ACK: the status byte.
    after_ack: u8,
    /// The image being built: raw 2BPP tile bytes, 20 tiles wide, in tile order.
    image: Vec<u8>,
    /// Whether the image holds data that has not been printed yet.
    unprocessed: bool,
    /// Whether a print is in progress (clears after an inquiry).
    printing: bool,
    /// Inquiries since the print started; the print finishes after one.
    print_ticks: u8,
    /// Whether the last print finished (the image is still in the buffer).
    printed: bool,
    /// The last print command's parameters.
    pub sheets: u8,
    pub margins: u8,
    pub palette: u8,
    pub exposure: u8,
    /// How many prints have run since the last initialise.
    pub print_count: u32,
    /// Sticky error bits (checksum error, packet error) until the next good packet.
    errors: u8,
    /// Whether an empty data packet arrived since the last print (Pan Docs: a print
    /// is ignored without one; the emulator still prints when there is data).
    terminated: bool,
}

impl Printer {
    /// Attach (or detach) a printer.
    pub fn set_present(&mut self, present: bool) {
        self.present = present;
        if !present {
            *self = Printer::default();
        }
    }

    /// The byte the next transfer clocks into SB. `$00` while a packet is coming,
    /// `$81` for the ACK, the status byte after it.
    pub fn response(&self) -> u8 {
        self.reply
    }

    /// The byte the game just clocked out (the one `serial_tx` held). Advances the
    /// packet state machine; the reply for the *next* transfer is set here.
    pub fn receive(&mut self, byte: u8) {
        match self.state {
            State::Ack => {
                // The game sent `$00` to clock out the ACK; the status comes next.
                self.reply = self.after_ack;
                self.state = State::Status;
            }
            State::Status => {
                // The status is out; back to waiting for a packet.
                self.reply = 0;
                self.state = State::Idle;
            }
            State::Idle => {
                if byte == 0x88 {
                    self.state = State::Magic1;
                }
            }
            State::Magic1 => {
                self.state = if byte == 0x33 {
                    State::Command
                } else if byte == 0x88 {
                    State::Magic1
                } else {
                    State::Idle
                };
            }
            State::Command => {
                self.command = byte;
                self.compressed = false;
                self.len = 0;
                self.data.clear();
                self.checksum = u16::from(byte);
                self.checksum_recv = 0;
                self.state = State::Compression;
            }
            State::Compression => {
                self.compressed = byte != 0;
                self.checksum = self.checksum.wrapping_add(u16::from(byte));
                self.state = State::LenLo;
            }
            State::LenLo => {
                self.len = usize::from(byte);
                self.checksum = self.checksum.wrapping_add(u16::from(byte));
                self.state = State::LenHi;
            }
            State::LenHi => {
                self.len |= usize::from(byte) << 8;
                self.checksum = self.checksum.wrapping_add(u16::from(byte));
                self.state = if self.len == 0 { State::ChecksumLo } else { State::Data };
            }
            State::Data => {
                self.data.push(byte);
                self.checksum = self.checksum.wrapping_add(u16::from(byte));
                if self.data.len() >= self.len {
                    self.state = State::ChecksumLo;
                }
            }
            State::ChecksumLo => {
                self.checksum_recv = u16::from(byte);
                self.state = State::ChecksumHi;
            }
            State::ChecksumHi => {
                self.checksum_recv |= u16::from(byte) << 8;
                self.finish_packet();
            }
        }
    }

    /// The packet is complete: check it, run the command, queue the ACK + status.
    fn finish_packet(&mut self) {
        let ok = self.checksum_recv == self.checksum;
        if ok {
            self.errors &= !0x01;
        } else {
            self.errors |= 0x01;
        }
        // The status the ACK carries is the state before this packet's command runs:
        // after a data packet the answer is still $00, and only the next inquiry
        // shows the data as unprocessed (Pan Docs' worked example).
        let status = self.status();
        if ok {
            self.run_command();
        }
        self.reply = 0x81;
        self.after_ack = status;
        self.state = State::Ack;
    }

    /// Run the command in the packet that just arrived.
    fn run_command(&mut self) {
        match self.command {
            // Initialise: clear the buffer RAM and the state.
            0x01 => {
                self.image.clear();
                self.unprocessed = false;
                self.printing = false;
                self.printed = false;
                self.terminated = false;
                self.errors = 0;
            }
            // Print: sheets, margins, palette, exposure. Needs data to print.
            0x02 => {
                if self.data.len() >= 4 {
                    self.sheets = self.data[0];
                    self.margins = self.data[1];
                    self.palette = self.data[2];
                    self.exposure = self.data[3];
                }
                if !self.image.is_empty() {
                    self.printing = true;
                    self.print_ticks = 0;
                    self.unprocessed = false;
                    self.printed = true;
                    self.print_count += 1;
                    self.terminated = false;
                }
            }
            // Data: tile bytes for the image, uncompressed or RLE.
            0x04 => {
                if self.data.is_empty() {
                    // An empty data packet ends the image before a print.
                    self.terminated = true;
                } else {
                    let mut bytes = std::mem::take(&mut self.data);
                    if self.compressed {
                        bytes = decompress_rle(&bytes);
                    }
                    let room = PRINTER_BUFFER_BYTES.saturating_sub(self.image.len());
                    let take = bytes.len().min(room);
                    self.image.extend_from_slice(&bytes[..take]);
                    if take < bytes.len() {
                        // The buffer is full.
                        self.errors |= 0x04;
                    }
                    self.unprocessed = !self.image.is_empty();
                }
            }
            // Break: stop printing, clear the buffer.
            0x08 => {
                self.printing = false;
                self.image.clear();
                self.unprocessed = false;
                self.printed = false;
                self.terminated = false;
            }
            // Inquiry: just reads the status; the print advances while it is polled.
            0x0F => {
                if self.printing {
                    self.print_ticks += 1;
                    if self.print_ticks >= 1 {
                        self.printing = false;
                    }
                }
            }
            _ => {
                self.errors |= 0x10;
            }
        }
    }

    /// The status byte: bit 0 checksum error, 1 printing, 2 image data full,
    /// 3 unprocessed data, 4 packet error. (No paper jam, other error or low
    /// battery in the emulator.)
    pub fn status(&self) -> u8 {
        let mut s = self.errors & 0x11;
        if self.printing {
            s |= 0x02;
        }
        if self.printed || self.image.len() >= PRINTER_BUFFER_BYTES {
            s |= 0x04;
        }
        if self.unprocessed {
            s |= 0x08;
        }
        s
    }

    /// Whether a picture is in the buffer.
    pub fn has_image(&self) -> bool {
        !self.image.is_empty()
    }

    /// Whether a print has run since the last initialise.
    pub fn printed(&self) -> bool {
        self.printed
    }

    /// The image as raw 2BPP tile bytes (20 tiles wide, in tile order).
    pub fn image_tiles(&self) -> &[u8] {
        &self.image
    }

    /// The image's height in pixels: each 640-byte chunk is 16 rows.
    pub fn image_height(&self) -> usize {
        (self.image.len() / PRINTER_PACKET_BYTES) * 16
    }

    /// The image decoded to 2-bit pixels, row-major, 160 wide. A partial final
    /// chunk is padded with zeroes.
    pub fn image_pixels(&self) -> Vec<u8> {
        let height = self.image_height();
        let mut px = vec![0u8; PRINTER_WIDTH * height];
        for (ti, tile) in self.image.chunks_exact(16).enumerate() {
            let tile_row = ti / 20;
            let tile_col = ti % 20;
            if tile_row * 16 >= height {
                break;
            }
            for y in 0..8 {
                let lo = tile[y * 2];
                let hi = tile[y * 2 + 1];
                for x in 0..8 {
                    let bit = 7 - x;
                    let v = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                    let row = tile_row * 8 + y;
                    let col = tile_col * 8 + x;
                    if row < height {
                        px[row * PRINTER_WIDTH + col] = v;
                    }
                }
            }
        }
        px
    }
}

/// The printer's RLE: a `$00` byte is followed by a count and a value, and the
/// value repeats that many times; any other byte is literal. A literal `$00` is
/// written as `$00 $01 $00`.
fn decompress_rle(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0 {
            if i + 2 >= data.len() {
                break;
            }
            let count = data[i + 1] as usize;
            let value = data[i + 2];
            out.resize(out.len() + count, value);
            i += 3;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Send a whole packet (plus the two `$00` reads) and return the ACK + status.
    fn exchange(p: &mut Printer, packet: &[u8]) -> (u8, u8) {
        for &b in packet {
            assert_eq!(p.response(), 0, "the printer answers $00 during a packet");
            p.receive(b);
        }
        assert_eq!(p.response(), 0x81, "the ACK after a packet");
        p.receive(0);
        let status = p.response();
        p.receive(0);
        assert_eq!(p.response(), 0, "idle again after the status");
        (0x81, status)
    }

    fn packet(command: u8, data: &[u8]) -> Vec<u8> {
        let mut p = vec![0x88, 0x33, command, 0, (data.len() & 0xFF) as u8, ((data.len() >> 8) & 0xFF) as u8];
        p.extend_from_slice(data);
        let sum: u16 = p[2..].iter().map(|&b| u16::from(b)).sum();
        p.extend_from_slice(&sum.to_le_bytes());
        p
    }

    #[test]
    fn printer_init_data_inquiry_print() {
        let mut p = Printer::default();
        p.set_present(true);
        // Initialise: status $00.
        let (_, s) = exchange(&mut p, &packet(0x01, &[]));
        assert_eq!(s, 0x00);
        // Data: 640 bytes of tiles. Status still $00 (the unprocessed bit only
        // shows on the next inquiry, after the data has landed).
        let tiles = vec![0xAAu8; 640];
        let (_, s) = exchange(&mut p, &packet(0x04, &tiles));
        assert_eq!(s, 0x00);
        assert!(p.has_image());
        assert_eq!(p.image_height(), 16);
        // Inquiry: ready to print ($08).
        let (_, s) = exchange(&mut p, &packet(0x0F, &[]));
        assert_eq!(s, 0x08);
        // Empty data packet, then print.
        let (_, s) = exchange(&mut p, &packet(0x04, &[]));
        assert_eq!(s, 0x08);
        let (_, s) = exchange(&mut p, &packet(0x02, &[1, 0x13, 0xE4, 0x40]));
        assert_eq!(s, 0x08);
        assert_eq!(p.print_count, 1);
        assert_eq!(p.sheets, 1);
        assert_eq!(p.palette, 0xE4);
        // Poll: printing ($06), then done ($04).
        let (_, s) = exchange(&mut p, &packet(0x0F, &[]));
        assert_eq!(s, 0x06);
        let (_, s) = exchange(&mut p, &packet(0x0F, &[]));
        assert_eq!(s, 0x04);
        // The image decodes: a tile of $AA/$AA is pixels 3,0,3,0...
        let px = p.image_pixels();
        assert_eq!(px.len(), 160 * 16);
        assert_eq!(px[0], 3);
        assert_eq!(px[1], 0);
    }

    #[test]
    fn printer_bad_checksum_sets_the_error_bit() {
        let mut p = Printer::default();
        p.set_present(true);
        let mut bad = packet(0x01, &[]);
        let last = bad.len() - 1;
        bad[last] = bad[last].wrapping_add(1);
        let (_, s) = exchange(&mut p, &bad);
        assert_eq!(s & 0x01, 0x01);
        // A good packet clears it.
        let (_, s) = exchange(&mut p, &packet(0x01, &[]));
        assert_eq!(s & 0x01, 0x00);
    }

    #[test]
    fn printer_rle_data_decompresses() {
        let mut p = Printer::default();
        p.set_present(true);
        // Compressed: $00 $03 $AA = three $AA bytes, then a literal $55.
        let mut pkt = vec![0x88, 0x33, 0x04, 1, 4, 0, 0x00, 0x03, 0xAA, 0x55];
        let sum: u16 = pkt[2..].iter().map(|&b| u16::from(b)).sum();
        pkt.extend_from_slice(&sum.to_le_bytes());
        exchange(&mut p, &pkt);
        assert_eq!(p.image_tiles(), &[0xAA, 0xAA, 0xAA, 0x55]);
    }
}
