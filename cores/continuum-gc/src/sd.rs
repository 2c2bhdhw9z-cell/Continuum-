//! An SD card on an EXI adapter (SD2SP2 in serial port 2, or an SD Gecko in a memory card
//! slot), speaking the SD card's SPI protocol byte by byte. Backed by a raw card image (a FAT
//! volume, with or without a partition table). Homebrew uses it for its files.
//!
//! Written from the SD Association's simplified physical layer specification (SPI mode):
//! commands are six bytes (0x40 | index, four argument bytes, CRC); replies are R1 (one status
//! byte), R2, R3/R7 (R1 plus four bytes); data blocks start with token 0xFE and end with a
//! CRC16. The card is SDHC (block addressing).

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

const BLOCK: usize = 512;

enum Writing {
    No,
    /// Waiting for a start token (0xFE single, 0xFC multiple, 0xFD stop).
    Token { multi: bool, block: u64 },
    Data { multi: bool, block: u64, buf: Vec<u8> },
}

pub struct SdCard {
    file: File,
    blocks: u64,
    writable: bool,
    cmd: Vec<u8>,
    out: VecDeque<u8>,
    app: bool,
    idle: bool,
    /// The next block of a multiple-block read.
    reading: Option<u64>,
    writing: Writing,
}

/// CRC16-CCITT (XMODEM) over a data block, as SD data blocks carry it.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

fn crc7(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        for i in (0..8).rev() {
            let bit = (b >> i) & 1;
            let top = (crc >> 6) & 1;
            crc = (crc << 1) & 0x7F;
            if bit ^ top != 0 {
                crc ^= 0x09;
            }
        }
    }
    crc
}

impl SdCard {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let (file, writable) = match std::fs::OpenOptions::new().read(true).write(true).open(path) {
            Ok(f) => (f, true),
            Err(_) => (File::open(path)?, false),
        };
        let blocks = file.metadata()?.len() / BLOCK as u64;
        Ok(SdCard { file, blocks, writable, cmd: Vec::new(), out: VecDeque::new(), app: false, idle: true, reading: None, writing: Writing::No })
    }

    fn read_block(&mut self, block: u64) -> Vec<u8> {
        let mut buf = vec![0u8; BLOCK];
        if block < self.blocks && self.file.seek(SeekFrom::Start(block * BLOCK as u64)).is_ok() {
            let _ = self.file.read_exact(&mut buf);
        }
        buf
    }

    fn queue_block(&mut self, data: &[u8]) {
        self.out.push_back(0xFE);
        self.out.extend(data.iter().copied());
        let crc = crc16(data);
        self.out.push_back((crc >> 8) as u8);
        self.out.push_back(crc as u8);
    }

    fn r1(&self) -> u8 {
        if self.idle {
            1
        } else {
            0
        }
    }

    fn csd(&self) -> [u8; 16] {
        // CSD version 2.0 (SDHC). Command classes without class 10 (switch), 512-byte blocks.
        let c_size = (self.blocks / 1024).saturating_sub(1).min(0x3F_FFFF) as u32;
        let mut c = [0x40, 0x0E, 0x00, 0x32, 0x1B, 0x59, 0x00, (c_size >> 16) as u8 & 0x3F, (c_size >> 8) as u8, c_size as u8, 0x7F, 0x80, 0x0A, 0x40, 0x00, 0];
        c[15] = (crc7(&c[..15]) << 1) | 1;
        c
    }

    fn cid(&self) -> [u8; 16] {
        let mut c = [0x03, b'C', b'N', b'C', b'G', b'C', b'S', b'D', 0x10, 0x12, 0x34, 0x56, 0x78, 0x01, 0x6A, 0];
        c[15] = (crc7(&c[..15]) << 1) | 1;
        c
    }

    fn command(&mut self) {
        let index = self.cmd[0] & 0x3F;
        let arg = u32::from_be_bytes([self.cmd[1], self.cmd[2], self.cmd[3], self.cmd[4]]);
        let app = std::mem::take(&mut self.app);
        self.out.clear();
        match (app, index) {
            (_, 0) => {
                self.idle = true;
                self.reading = None;
                self.out.push_back(1);
            }
            (false, 8) => {
                self.out.extend([self.r1(), 0x00, 0x00, ((arg >> 8) & 0xF) as u8, arg as u8]);
            }
            (false, 9) => {
                self.out.push_back(self.r1());
                let c = self.csd();
                self.queue_block(&c);
            }
            (false, 10) => {
                self.out.push_back(self.r1());
                let c = self.cid();
                self.queue_block(&c);
            }
            (false, 12) => {
                self.reading = None;
                // A stuff byte, then R1.
                self.out.extend([0xFF, self.r1()]);
            }
            (false, 13) => self.out.extend([self.r1(), 0x00]),
            (false, 17) => {
                self.out.push_back(self.r1());
                let b = self.read_block(arg as u64);
                self.queue_block(&b);
            }
            (false, 18) => {
                self.out.push_back(self.r1());
                let b = self.read_block(arg as u64);
                self.queue_block(&b);
                self.reading = Some(arg as u64 + 1);
            }
            (false, 24) | (false, 25) => {
                self.out.push_back(self.r1());
                self.writing = Writing::Token { multi: index == 25, block: arg as u64 };
            }
            (false, 55) => {
                self.app = true;
                self.out.push_back(self.r1());
            }
            (false, 58) => {
                // OCR: powered up, high capacity, 2.7-3.6 V.
                self.out.extend([self.r1(), 0xC0, 0xFF, 0x80, 0x00]);
            }
            (true, 41) => {
                self.idle = false;
                self.out.push_back(0);
            }
            (true, 13) => {
                self.out.extend([self.r1(), 0x00]);
                self.queue_block(&[0u8; 64]);
            }
            (true, 51) => {
                // SCR: SD spec 2.0, 1 and 4 bit buses.
                self.out.push_back(self.r1());
                self.queue_block(&[0x02, 0x35, 0x80, 0x00, 0, 0, 0, 0]);
            }
            _ => self.out.push_back(self.r1()),
        }
    }

    /// One byte each way on the SPI bus.
    pub fn exchange(&mut self, input: u8) -> u8 {
        if self.out.is_empty() {
            if let Some(b) = self.reading {
                let data = self.read_block(b);
                self.queue_block(&data);
                self.reading = Some(b + 1);
            }
        }
        let reply = self.out.pop_front().unwrap_or(0xFF);
        match std::mem::replace(&mut self.writing, Writing::No) {
            Writing::Token { multi, block } => {
                match input {
                    0xFE | 0xFC => self.writing = Writing::Data { multi, block, buf: Vec::with_capacity(BLOCK + 2) },
                    0xFD => {
                        // Stop: a busy byte, then ready.
                        self.out.extend([0x00, 0xFF]);
                    }
                    _ => self.writing = Writing::Token { multi, block },
                }
                return reply;
            }
            Writing::Data { multi, block, mut buf } => {
                buf.push(input);
                if buf.len() == BLOCK + 2 {
                    if self.writable && block < self.blocks && self.file.seek(SeekFrom::Start(block * BLOCK as u64)).is_ok() {
                        let _ = self.file.write_all(&buf[..BLOCK]);
                    }
                    // Data accepted, busy, ready.
                    self.out.extend([0x05, 0x00, 0xFF]);
                    if multi {
                        self.writing = Writing::Token { multi, block: block + 1 };
                    }
                } else {
                    self.writing = Writing::Data { multi, block, buf };
                }
                return reply;
            }
            Writing::No => {}
        }
        if self.cmd.is_empty() {
            if input & 0xC0 == 0x40 {
                self.cmd.push(input);
            }
        } else {
            self.cmd.push(input);
            if self.cmd.len() == 6 {
                self.command();
                self.cmd.clear();
            }
        }
        reply
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_matches_the_spec_example() {
        // The SD specification's example: 512 bytes of 0xFF give CRC16 0x7FA1.
        assert_eq!(crc16(&[0xFF; 512]), 0x7FA1);
    }

    #[test]
    fn crc7_matches_the_spec_example() {
        // CMD0 with argument 0: CRC7 0x4A (the famous 0x95 with the end bit).
        assert_eq!((crc7(&[0x40, 0, 0, 0, 0]) << 1) | 1, 0x95);
    }
}
