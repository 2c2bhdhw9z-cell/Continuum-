//! cMini's own start-up program: the 4 KiB at $0000 that a console without its own `bios.min`
//! runs. Written for cMini (no Nintendo or PokeMini code in it), doing what games expect of the
//! console's program, as PokeMini's free one does them:
//!
//! - the start: interrupts off, the stack at $2000, the cartridge and LCD switched on, the I/O
//!   registers and the LCD set up, RAM and the LCD cleared, then the game entered at $2102 with
//!   B holding the buttons held at power-on and A $FF (no logo: the game starts at once);
//! - an interrupt's way to the cartridge: each of the console's interrupts goes to its slot in
//!   the cartridge's table ($2108 on, six bytes each), or to a handler the game put in RAM at
//!   $1FFD; the power button's goes to the cartridge only once the game has taken over;
//! - the services games call with CINT: suspend and shut down (sleeping until the power button),
//!   the LCD's contrast (kept in $2000's top six bits), switching the LCD on and off, the RAM
//!   handler, the cartridge's power, the PRC's rate, a short pulse on a line, and a structure
//!   test. Unused vectors return straight away.
//!
//! Where PokeMini's free program slips, this one does what was meant: resuming from a suspend
//! puts back the interrupt priorities it changed (PokeMini's clears the whole register), and
//! the structure test's subroutine is at its address's low, high and bank bytes in that order.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The image, built once.
pub fn image() -> Vec<u8> {
    static IMAGE: OnceLock<Vec<u8>> = OnceLock::new();
    IMAGE.get_or_init(build).clone()
}

/// A jump or call to fix up once its label is known.
enum Fix {
    /// A one-byte offset from the instruction's last byte.
    Short { last: usize },
    /// A two-byte offset from the instruction's last byte.
    Long { last: usize },
    /// A vector: the label's address.
    Word,
}

/// Just enough of a MINX assembler for the program below.
struct Asm {
    b: Vec<u8>,
    at: usize,
    labels: BTreeMap<&'static str, usize>,
    fixes: Vec<(usize, &'static str, Fix)>,
}

impl Asm {
    fn new() -> Self {
        Self { b: vec![0xFF; 0x1000], at: 0, labels: BTreeMap::new(), fixes: Vec::new() }
    }
    fn emit(&mut self, bytes: &[u8]) {
        self.b[self.at..self.at + bytes.len()].copy_from_slice(bytes);
        self.at += bytes.len();
    }
    fn label(&mut self, name: &'static str) {
        assert!(self.labels.insert(name, self.at).is_none(), "label {name} twice");
    }
    fn vector(&mut self, name: &'static str) {
        self.fixes.push((self.at, name, Fix::Word));
        self.emit(&[0, 0]);
    }
    fn word(&mut self, v: u16) {
        self.emit(&v.to_le_bytes());
    }
    /// A long relative jump or call (opcode, then the offset to `to`).
    fn far(&mut self, op: u8, to: &'static str) {
        self.fixes.push((self.at + 1, to, Fix::Long { last: self.at + 2 }));
        self.emit(&[op, 0, 0]);
    }
    /// The same to a fixed address.
    fn far_to(&mut self, op: u8, to: u16) {
        let off = i32::from(to) - (self.at as i32 + 2);
        let off = off as u16;
        self.emit(&[op, off as u8, (off >> 8) as u8]);
    }

    // The I/O registers through N (always $20 here).
    fn mov_n(&mut self, v: u8) {
        self.emit(&[0xB4, v]);
    }
    fn set(&mut self, reg: u8, v: u8) {
        self.emit(&[0xDD, reg, v]);
    }
    fn or(&mut self, reg: u8, v: u8) {
        self.emit(&[0xD9, reg, v]);
    }
    fn and(&mut self, reg: u8, v: u8) {
        self.emit(&[0xD8, reg, v]);
    }
    fn test(&mut self, reg: u8, v: u8) {
        self.emit(&[0xDC, reg, v]);
    }
    fn a_from(&mut self, reg: u8) {
        self.emit(&[0x44, reg]);
    }
    fn b_from(&mut self, reg: u8) {
        self.emit(&[0x4C, reg]);
    }
    fn a_to(&mut self, reg: u8) {
        self.emit(&[0x78, reg]);
    }
    fn b_to(&mut self, reg: u8) {
        self.emit(&[0x79, reg]);
    }
    /// LCD commands.
    fn lcd(&mut self, commands: &[u8]) {
        for &c in commands {
            self.set(0xFE, c);
        }
    }

    // Branches.
    fn jmp(&mut self, to: &'static str) {
        self.far(0xF3, to);
    }
    fn jmp_to(&mut self, to: u16) {
        self.far_to(0xF3, to);
    }
    fn call(&mut self, to: &'static str) {
        self.far(0xF2, to);
    }
    fn jz(&mut self, to: &'static str) {
        self.far(0xEE, to);
    }
    fn jnz(&mut self, to: &'static str) {
        self.far(0xEF, to);
    }
    fn callz(&mut self, to: &'static str) {
        self.far(0xEA, to);
    }
    fn jdbnz(&mut self, to: &'static str) {
        self.fixes.push((self.at + 1, to, Fix::Short { last: self.at + 1 }));
        self.emit(&[0xF5, 0]);
    }

    fn finish(mut self) -> Vec<u8> {
        for (at, name, fix) in std::mem::take(&mut self.fixes) {
            let to = *self.labels.get(name).unwrap_or_else(|| panic!("no label {name}"));
            match fix {
                Fix::Word => self.b[at..at + 2].copy_from_slice(&(to as u16).to_le_bytes()),
                Fix::Long { last } => {
                    let off = (to as i32 - last as i32) as u16;
                    self.b[at..at + 2].copy_from_slice(&off.to_le_bytes());
                }
                Fix::Short { last } => {
                    let off = to as i32 - last as i32;
                    assert!((-128..128).contains(&off), "{name} too far for a short branch");
                    self.b[at] = off as i8 as u8;
                }
            }
        }
        self.b
    }
}

// Opcodes used directly.
const PUSH_I: u8 = 0xA5;
const PUSH_N: u8 = 0xA4;
const POP_N: u8 = 0xAC;
const POP_I: u8 = 0xAD;
const PUSH_F: u8 = 0xA7;
const POP_F: u8 = 0xAF;
const PUSH_BA: u8 = 0xA0;
const POP_BA: u8 = 0xA8;
const PUSH_HL: u8 = 0xA1;
const POP_HL: u8 = 0xA9;
const PUSH_X: u8 = 0xA2;
const POP_X: u8 = 0xAA;
const PUSH_Y: u8 = 0xA3;
const POP_Y: u8 = 0xAB;
const PUSH_BANKS: u8 = 0xA6;
const POP_BANKS: u8 = 0xAE;
const RET: u8 = 0xF8;
const RETI: u8 = 0xF9;
/// Where a game's own interrupt handler goes in RAM.
const RAM_HANDLER: u16 = 0x1FFD;

/// Each interrupt's slot in the cartridge's table.
fn slot(n: u8) -> u16 {
    match n {
        0x03..=0x10 => 0x2108 + 6 * u16::from(n - 0x03),
        0x14 => 0x219E,
        _ => 0x215C + 6 * u16::from(n - 0x15),
    }
}

const IRQ_LABELS: [&str; 26] = [
    "irq03", "irq04", "irq05", "irq06", "irq07", "irq08", "irq09", "irq0a", "irq0b", "irq0c", "irq0d", "irq0e", "irq0f",
    "irq10", "irq14", "irq15", "irq16", "irq17", "irq18", "irq19", "irq1a", "irq1b", "irq1c", "irq1d", "irq1e", "irq1f",
];
const IRQS: [u8; 26] = [
    0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x14, 0x15, 0x16, 0x17, 0x18,
    0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
];

fn build() -> Vec<u8> {
    let mut a = Asm::new();

    // ---------------------------------------------------------------- the vectors
    for v in 0..0x4Du8 {
        match v {
            0x00 => a.vector("cold"),
            0x01 | 0x02 | 0x11 | 0x12 => a.vector("warm"),
            0x03..=0x10 | 0x14..=0x1F => a.vector(IRQ_LABELS[IRQS.iter().position(|&n| n == v).unwrap()]),
            // The game's own routine, at $FFF1 of its bank.
            0x20 => a.word(0xFFF1),
            0x21 => a.vector("suspend"),
            0x24 => a.vector("shut_down"),
            0x26 => a.vector("set_contrast"),
            0x27 => a.vector("step_contrast"),
            0x28 => a.vector("apply_contrast"),
            0x29 => a.vector("get_contrast"),
            0x2A => a.vector("show_contrast"),
            0x2B => a.vector("lcd_on"),
            0x2C => a.vector("lcd_init"),
            0x2D => a.vector("lcd_off"),
            0x2E => a.vector("ram_handler"),
            0x39 => a.vector("cart_off"),
            0x3A => a.vector("cart_on"),
            0x3D => a.vector("test_53"),
            0x3E => a.vector("structure"),
            0x3F => a.vector("set_rate"),
            0x40 => a.vector("get_rate"),
            0x41 => a.vector("test_01"),
            // Back to the start.
            0x4B => a.word(0x0000),
            0x4C => a.vector("pulse"),
            _ => a.vector("return"),
        }
    }

    // ---------------------------------------------------------------- the start
    a.at = 0xA0;
    a.label("cold");
    a.mov_n(0x20);
    a.set(0x08, 0x02); // the seconds counter cleared...
    a.or(0x08, 0x01); // ...and running
    a.set(0x00, 0x7C); // contrast $1F remembered; cartridge and LCD off
    a.set(0x02, 0x00); // a cold start: the clock's time isn't valid
    a.label("warm");
    a.emit(&[0x9F, 0xC0]); // MOV F,#$C0: interrupts off
    a.mov_n(0x20);
    a.emit(&[0x38, 0xCE, 0xCD, 0xCE, 0xCE, 0xCE, 0xCF]); // XOR A,A; MOV I,A; MOV XI,A; MOV YI,A
    a.emit(&[0xCF, 0x6E, 0x00, 0x20]); // MOV SP,#$2000
    a.set(0x80, 0x00); // the PRC off
    a.emit(&[0xCE, 0xC4, 0x01]); // MOV U,#$01: the code bank the game starts in
    a.jmp("started");
    a.label("started");
    a.or(0x00, 0x03); // cartridge and LCD on
    a.or(0x01, 0x30);
    a.or(0x02, 0xC0);
    a.call("io_init");
    for reg in 0x82..=0x89 {
        a.set(reg, 0x00); // the PRC's map and sprite bases at $000000
    }
    a.or(0x81, 0x01); // the PRC's frames counted (every third works)
    a.call("lcd_setup");
    a.lcd(&[0xAF]); // the display on
    a.call("lcd_clear");
    // A frame of the PRC's ends before the game starts, as on the console, whose start takes
    // a while: the game finds that frame's interrupt waiting.
    a.label("first_frame");
    a.test(0x27, 0x40);
    a.jz("first_frame");
    a.set(0x80, 0x00);
    a.set(0x81, 0x07);
    a.and(0x01, 0xEF);
    // RAM cleared last, the stack's top with it.
    a.emit(&[0x38, 0xC6, 0x00, 0x10]); // XOR A,A; MOV X,#$1000
    a.label("clear_ram");
    a.emit(&[0x60, 0x92, 0xD6, 0x00, 0x20]); // MOV [X],A; INC X; CMP X,#$2000
    a.jnz("clear_ram");
    // The game: registers clear, B the buttons held, A $FF, interrupts still off.
    a.emit(&[0xC4, 0x00, 0x00, 0xC5, 0x00, 0x00, 0xC6, 0x00, 0x00, 0xC7, 0x00, 0x00]);
    a.b_from(0x52);
    a.emit(&[0xB0, 0xFF, 0x9C, 0xC0]); // MOV A,#$FF; AND F,#$C0
    a.jmp_to(0x2102);

    // The I/O registers as the game finds them.
    a.label("io_init");
    a.emit(&[0xB1, 0x00]); // MOV B,#0
    a.set(0x10, 0x08); // the battery sensor
    a.and(0x19, 0xCF); // timer 1's oscillators stopped
    for (reg, v) in [(0x20, 0x00), (0x21, 0x30), (0x22, 0x02), (0x23, 0x00), (0x24, 0x02), (0x25, 0x00), (0x26, 0x00), (0x40, 0x00), (0x44, 0x00), (0x50, 0xFF), (0x51, 0x00), (0x54, 0x00), (0x55, 0x00)] {
        a.set(reg, v);
    }
    // The save chip's lines: both driven high, then let go, with the infrared off.
    a.or(0x60, 0x0C);
    a.and(0x61, 0xFB);
    a.or(0x61, 0x08);
    a.or(0x61, 0x04);
    a.set(0x61, 0x20);
    a.set(0x60, 0x32);
    a.set(0x62, 0x00);
    a.set(0x70, 0x00);
    a.set(0x71, 0x00); // the sound off
    a.lcd(&[0x81, 0x1F]); // contrast $1F
    a.emit(&[RET]);

    // The LCD controller's set-up.
    a.label("lcd_setup");
    a.lcd(&[0xE3, 0xA4, 0xAD, 0x00, 0x10, 0xEE, 0x40, 0xA2, 0xA0, 0xC0, 0xA6, 0x2F]);
    a.emit(&[RET]);

    // The LCD's eight pages cleared directly.
    a.label("lcd_clear");
    a.lcd(&[0xE3]);
    a.emit(&[0xB2, 0xB0]); // MOV L,#$B0: the page command
    a.label("lcd_page");
    a.emit(&[0x7A, 0xFE]); // MOV [N+$FE],L
    a.lcd(&[0x00, 0x10]);
    a.emit(&[0xB1, 0x60]); // MOV B,#96
    a.label("lcd_column");
    a.set(0xFF, 0x00);
    a.jdbnz("lcd_column");
    a.emit(&[0x82, 0x42, 0x32, 0xB8]); // INC L; MOV A,L; CMP A,#$B8
    a.jnz("lcd_page");
    a.emit(&[RET]);

    // ---------------------------------------------------------------- interrupts
    // The way through to the cartridge: with I and N saved and set for the registers, the game
    // takes over the first time (or its RAM handler does, if it set one up).
    for (k, &n) in IRQS.iter().enumerate() {
        a.label(IRQ_LABELS[k]);
        if n == 0x15 {
            a.jmp("power_button");
            continue;
        }
        a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
        a.mov_n(0x20);
        a.test(0x01, 0x20);
        a.callz("take_over");
        a.emit(&[POP_N, POP_I]);
        a.jmp_to(slot(n));
    }

    a.label("take_over");
    a.call("cart_power");
    a.test(0x01, 0x80);
    a.jz("no_ram_handler");
    // To the RAM handler instead: this call's return and the saved registers off the stack.
    a.emit(&[0xCF, 0x68, 0x03, 0x00, POP_N, POP_I]);
    a.jmp_to(RAM_HANDLER);
    a.label("no_ram_handler");
    a.or(0x01, 0x20); // the cartridge handles interrupts from now on
    a.emit(&[RET]);

    // The cartridge's power, when it was cut and the console says so.
    a.label("cart_power");
    a.test(0x71, 0x04);
    a.jz("powered");
    a.test(0x02, 0x40);
    a.jz("powered");
    a.and(0x71, 0xFB);
    a.or(0x02, 0x80);
    a.label("powered");
    a.emit(&[RET]);

    // The power button: the game's once it has taken over; before that a shut-down request
    // restarts, a RAM handler takes it, or it is the game's from now on.
    a.label("power_button");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.test(0x01, 0x20);
    a.jnz("power_to_cart");
    a.test(0x01, 0x10);
    a.jnz("warm");
    a.call("cart_power");
    a.test(0x01, 0x80);
    a.jz("power_no_handler");
    a.test(0x02, 0x20);
    a.jnz("power_done");
    a.emit(&[POP_N, POP_I]);
    a.jmp_to(RAM_HANDLER);
    a.label("power_to_cart");
    a.emit(&[POP_N, POP_I]);
    a.jmp_to(slot(0x15));
    a.label("power_no_handler");
    a.or(0x01, 0x20);
    a.test(0x02, 0x20);
    a.jnz("power_to_cart");
    a.label("power_done");
    a.set(0x29, 0x80); // the power button's interrupt answered
    a.emit(&[POP_N, POP_I, RETI]);

    a.label("return");
    a.emit(&[RETI]);

    // ---------------------------------------------------------------- services
    // Sleep until the power button: the LCD and PRC off, then the LCD back on.
    a.label("sleep");
    a.and(0x80, 0xF7);
    a.lcd(&[0xAE, 0xAC, 0x28, 0xA5]);
    a.and(0x02, 0xE3);
    a.and(0x81, 0xFE);
    a.or(0x02, 0x01);
    a.or(0x24, 0x02);
    a.or(0x21, 0x30);
    a.and(0x01, 0xDF);
    a.and(0x02, 0xE3);
    a.emit(&[PUSH_F, 0x9F, 0x80, 0xCE, 0xAE, POP_F]); // interrupts in, HALT, as before
    a.or(0x02, 0x10);
    a.or(0x81, 0x01);
    a.call("lcd_setup");
    a.lcd(&[0xAF]);
    a.and(0x02, 0xDF);
    a.or(0x00, 0x03);
    a.emit(&[RET]);

    // CINT $21: suspend. Only the power button can wake it; what it changes comes back.
    a.label("suspend");
    a.emit(&[PUSH_I, PUSH_N, PUSH_BA, 0x9F, 0xC0, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.b_from(0x80);
    a.a_from(0x21);
    a.emit(&[0x22, 0x0C, PUSH_BA]); // AND A,#$0C
    a.b_from(0x23);
    a.a_from(0x24);
    a.emit(&[PUSH_BA]);
    a.b_from(0x25);
    a.a_from(0x26);
    a.emit(&[PUSH_BA]);
    for (reg, v) in [(0x23, 0x00), (0x24, 0x00), (0x25, 0x80), (0x26, 0x00)] {
        a.set(reg, v);
    }
    a.or(0x21, 0x0C);
    a.set(0x29, 0x80);
    a.set(0x80, 0x00);
    a.call("sleep");
    a.emit(&[POP_BA]);
    a.b_to(0x25);
    a.a_to(0x26);
    a.emit(&[POP_BA]);
    a.b_to(0x23);
    a.a_to(0x24);
    a.emit(&[POP_BA]);
    a.b_to(0x80);
    // $2021's two bits back as they were, the rest as they are now.
    a.emit(&[0x48]); // MOV B,A
    a.a_from(0x21);
    a.emit(&[0x22, 0xF3, 0x29]); // AND A,#$F3; OR A,B
    a.a_to(0x21);
    a.emit(&[POP_BA, POP_N, POP_I, RETI]);

    // CINT $24: shut down, then start again when the power button wakes it.
    a.label("shut_down");
    a.emit(&[0x9F, 0xC0, 0xCF, 0x6E, 0x00, 0x20, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.call("io_init");
    for (reg, v) in [(0x23, 0x00), (0x24, 0x00), (0x25, 0x80), (0x26, 0x00), (0x29, 0x80), (0x80, 0x00), (0x21, 0x0C)] {
        a.set(reg, v);
    }
    a.call("sleep");
    a.jmp("warm");

    // The contrast services. CINT pushes F after the return address; those that end in RET
    // take F off again first, so they return as a call would, flags and all.
    // CINT $26: A (0-$3F) becomes the remembered contrast, and the LCD's.
    a.label("set_contrast");
    a.emit(&[POP_F]);
    a.label("set_contrast_call");
    a.emit(&[PUSH_I, PUSH_HL, 0xCE, 0xC5, 0x00, 0xC5, 0x00, 0x20]); // MOV HL,#$2000
    a.emit(&[0xCE, 0x84, 0xCE, 0x84]); // SHL A twice
    a.emit(&[0xCE, 0x25, 0x03, 0xCE, 0x2C]); // AND [HL],#3; OR [HL],A
    a.emit(&[POP_HL, POP_I]);
    a.jmp("apply_contrast_call");

    // CINT $27: the remembered contrast one step up (Z clear: darker) or down (Z set:
    // lighter); A is 0 when it moved, $FF at the end of its range.
    a.label("step_contrast");
    a.call("get_contrast_call");
    a.emit(&[POP_F, PUSH_F]); // the caller's flags, left on the stack for RETI
    a.jz("contrast_down");
    a.emit(&[0x32, 0x3F]); // CMP A,#$3F
    a.jz("contrast_end");
    a.emit(&[0x80]); // INC A
    a.jmp("contrast_moved");
    a.label("contrast_down");
    a.emit(&[0x28]); // OR A,A
    a.jz("contrast_end");
    a.emit(&[0x88]); // DEC A
    a.label("contrast_moved");
    a.call("set_contrast_call");
    a.emit(&[0x38, RETI]); // XOR A,A
    a.label("contrast_end");
    a.emit(&[0xB0, 0xFF, RETI]);

    // CINT $28: the remembered contrast to the LCD.
    a.label("apply_contrast");
    a.emit(&[POP_F]);
    a.label("apply_contrast_call");
    a.call("get_contrast_call");
    a.jmp("show_contrast_call");

    // CINT $29: A = the remembered contrast.
    a.label("get_contrast");
    a.emit(&[POP_F]);
    a.label("get_contrast_call");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.a_from(0x00);
    a.emit(&[0xCE, 0x8C, 0xCE, 0x8C, POP_N, POP_I, RET]); // SHR A twice

    // CINT $2A: A to the LCD's contrast now (with the PRC's copy held off meanwhile).
    a.label("show_contrast");
    a.emit(&[POP_F]);
    a.label("show_contrast_call");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.emit(&[0x48]); // MOV B,A
    a.test(0x80, 0x08);
    a.emit(&[PUSH_F]);
    a.and(0x80, 0xF7);
    a.set(0xFE, 0x81);
    a.b_to(0xFE);
    a.emit(&[POP_F]);
    a.jz("copy_was_off");
    a.or(0x80, 0x08);
    a.label("copy_was_off");
    a.emit(&[POP_N, POP_I, RET]);

    // CINT $2B: the LCD on.
    a.label("lcd_on");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.call("lcd_setup");
    a.lcd(&[0xAF]);
    a.or(0x80, 0x04);
    a.or(0x81, 0x01);
    a.emit(&[POP_N, POP_I, RETI]);

    // CINT $2C: the LCD controller set up again.
    a.label("lcd_init");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.call("lcd_setup");
    a.emit(&[POP_N, POP_I, RETI]);

    // CINT $2D: the LCD off.
    a.label("lcd_off");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.and(0x80, 0xF7);
    a.lcd(&[0xAE, 0xAC, 0x28, 0xA5]);
    a.and(0x81, 0xFE);
    a.emit(&[POP_N, POP_I, RETI]);

    // CINT $2E: the game's RAM handler takes the interrupts (unless it already does).
    a.label("ram_handler");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.emit(&[0x9F, 0xC0]);
    a.test(0x01, 0x80);
    a.jnz("ram_handler_done");
    a.or(0x01, 0x80);
    a.or(0x01, 0x40);
    a.and(0x01, 0xDF);
    a.label("ram_handler_done");
    a.emit(&[POP_N, POP_I, RETI]);

    // CINT $39 and $3A: the cartridge and LCD off and on.
    for (name, on) in [("cart_off", false), ("cart_on", true)] {
        a.label(name);
        a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
        a.mov_n(0x20);
        if on {
            a.or(0x00, 0x03);
        } else {
            a.and(0x00, 0xFC);
        }
        a.emit(&[POP_N, POP_I, RETI]);
    }

    // CINT $3D: Z set when $2053's bit 1 is.
    a.label("test_53");
    a.emit(&[POP_F, PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.test(0x53, 0x02);
    a.emit(&[0x9E, 0x01, POP_N, POP_I, RET]); // XOR F,#1

    // CINT $3E: the structure at X (bank XI): its type (bit 0: call its subroutine), the
    // address $FF is written to, the address compared, the value it must hold, and the
    // subroutine (low, high, bank). Without the call, A is 1 when the value matched. The
    // subroutine finds the comparison's flags under its return address.
    a.label("structure");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.emit(&[PUSH_BANKS, PUSH_X, PUSH_Y, PUSH_HL, 0xCF, 0xB1, 0x9F, 0xC0]); // ... PUSH B; MOV F,#$C0
    // The subroutine's address and bank, and the type, kept on the stack.
    a.emit(&[0xCE, 0x40, 0x08, 0x50, 0xCE, 0x40, 0x09, 0x58, PUSH_HL]); // L,H = [X+8],[X+9]
    a.emit(&[0xCE, 0x40, 0x0A, 0xCE, 0x48, 0x00, PUSH_BA]); // A = [X+10]; B = [X+0]
    // YI:Y the address compared, B the value, XI:X the address written.
    a.emit(&[0xCE, 0x40, 0x06, 0xCE, 0xCF]); // MOV A,[X+6]; MOV YI,A
    a.emit(&[0xCE, 0x40, 0x04, 0x50, 0xCE, 0x40, 0x05, 0x58, 0xCF, 0xED]); // L,H = [X+4],[X+5]; MOV Y,HL
    a.emit(&[0xCE, 0x48, 0x07]); // MOV B,[X+7]
    a.emit(&[0xCE, 0x40, 0x01, 0x50, 0xCE, 0x40, 0x02, 0x58]); // L,H = [X+1],[X+2]
    a.emit(&[0xCE, 0x40, 0x03, 0xCE, 0xCE, 0xCF, 0xE9]); // MOV A,[X+3]; MOV XI,A; MOV X,HL
    a.or(0x02, 0x80);
    a.emit(&[0xB6, 0xFF]); // MOV [X],#$FF
    a.or(0x02, 0x80);
    a.emit(&[0x47]); // MOV A,[Y]
    a.or(0x02, 0x80);
    a.emit(&[0x31, POP_BA, POP_HL, PUSH_F]); // CMP A,B; B = type, A = bank, HL = address; result kept
    a.emit(&[0x97, 0x01]); // TST B,#1
    a.jz("structure_done");
    a.emit(&[POP_F, PUSH_F]); // the comparison's flags in F too
    a.call("structure_call");
    a.label("structure_done");
    a.emit(&[POP_BA, 0x22, 0x01]); // A = the comparison's Zero; B back
    a.emit(&[POP_HL, POP_Y, POP_X, POP_BANKS, POP_N, POP_I, RETI]);
    a.label("structure_call");
    a.emit(&[0xCE, 0xCC, 0xF4]); // MOV U,A; JMP HL

    // CINT $3F: the PRC's rate (A, 0-7).
    a.label("set_rate");
    a.emit(&[PUSH_I, PUSH_HL, 0xCE, 0xC5, 0x00, 0xC5, 0x81, 0x20]); // MOV HL,#$2081
    // AND [HL],#$F1; AND A,#7; SHL A; OR [HL],A
    a.emit(&[0xCE, 0x25, 0xF1, 0x22, 0x07, 0xCE, 0x84, 0xCE, 0x2C]);
    a.emit(&[POP_HL, POP_I, RETI]);

    // CINT $40: A = the PRC's rate.
    a.label("get_rate");
    a.emit(&[PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.a_from(0x81);
    a.emit(&[0xCE, 0x8C, 0x22, 0x07, POP_N, POP_I, RETI]); // SHR A; AND A,#7

    // CINT $41: Z clear when $2001's bit 3 is set.
    a.label("test_01");
    a.emit(&[POP_F, PUSH_I, PUSH_N, 0xCE, 0xC5, 0x00]);
    a.mov_n(0x20);
    a.test(0x01, 0x08);
    a.emit(&[POP_N, POP_I, RET]);

    // CINT $4C: [Y] = 2, wait B times round, [Y] = 0.
    a.label("pulse");
    a.emit(&[0xB7, 0x02]);
    a.label("pulse_wait");
    a.jdbnz("pulse_wait");
    a.emit(&[0xB7, 0x00, RETI]);

    let end = a.at;
    let image = a.finish();
    assert!(end <= 0x1000, "the start-up program is too big");
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vector_points_into_the_program() {
        let b = image();
        for v in 0..0x4D {
            let to = u16::from_le_bytes([b[v * 2], b[v * 2 + 1]]);
            match v {
                0x20 => assert_eq!(to, 0xFFF1),
                0x4B => assert_eq!(to, 0),
                _ => assert!((0xA0..0x1000).contains(&to) && b[to as usize] != 0xFF, "vector {v:02X} at {to:04X}"),
            }
        }
    }
}
