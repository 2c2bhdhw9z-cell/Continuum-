//! The whole machine on our own test programs (assembled here, never anyone else's code):
//! booting a DOL, the video interface showing the program's framebuffer, VI interrupts through
//! PI to the CPU's exception handler, and save states.

use continuum_gc::disc::Console;
use continuum_gc::dol::Executable;
use continuum_gc::hw::yuv_to_rgb;
use continuum_gc::machine::Machine;

fn d(op: u32, t: u32, a: u32, imm: i32) -> u32 {
    (op << 26) | (t << 21) | (a << 16) | (imm as u32 & 0xFFFF)
}
fn x(t: u32, a: u32, b: u32, xo: u32) -> u32 {
    (31 << 26) | (t << 21) | (a << 16) | (b << 11) | (xo << 1)
}
fn li(t: u32, v: i32) -> u32 {
    d(14, t, 0, v)
}
fn lis(t: u32, v: u32) -> u32 {
    d(15, t, 0, v as i32)
}
fn ori(a: u32, s: u32, v: u32) -> u32 {
    d(24, s, a, v as i32)
}
fn addi(t: u32, a: u32, v: i32) -> u32 {
    d(14, t, a, v)
}
fn stw(s: u32, a: u32, off: i32) -> u32 {
    d(36, s, a, off)
}
fn sth(s: u32, a: u32, off: i32) -> u32 {
    d(44, s, a, off)
}
fn lwz(t: u32, a: u32, off: i32) -> u32 {
    d(32, t, a, off)
}
fn mtctr(s: u32) -> u32 {
    (31 << 26) | (s << 21) | (9 << 16) | (467 << 1)
}
const BDNZ_BACK_8: u32 = (16 << 26) | (16 << 21) | (0xFFF8 & 0xFFFC);
const RFI: u32 = 0x4C00_0064;
const HANG: u32 = 0x4800_0000;

/// Red in the framebuffer's YUV: two pixels per word (Y0 U Y1 V).
const RED_YUYV: u32 = 0x525A_52F0;
const COUNTER: u32 = 0x8000_2000;
const XFB: u32 = 0x8010_0000;

/// A DOL with sections at the given addresses.
fn dol(sections: &[(u32, Vec<u32>)], entry: u32) -> Vec<u8> {
    let mut b = vec![0u8; 0x100];
    let mut body = Vec::new();
    for (k, (addr, words)) in sections.iter().enumerate() {
        let off = 0x100 + body.len() as u32;
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
        b[k * 4..k * 4 + 4].copy_from_slice(&off.to_be_bytes());
        b[0x48 + k * 4..0x4C + k * 4].copy_from_slice(&addr.to_be_bytes());
        b[0x90 + k * 4..0x94 + k * 4].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        body.extend(bytes);
    }
    b[0xE0..0xE4].copy_from_slice(&entry.to_be_bytes());
    b.extend(body);
    b
}

/// Our VI test program: 640x240 progressive, framebuffer filled red, VI display interrupt 0 on
/// line 200 counted by the external-interrupt handler.
fn vi_program() -> Vec<u8> {
    // Exceptions run with translation off, so the handler uses physical addresses.
    let handler = vec![
        lis(20, 0x0C00),
        ori(20, 20, 0x2000),
        stw(21, 20, 0x30), // DI0 with the status bit clear: acknowledge
        lis(22, (COUNTER & 0x01FF_FFFF) >> 16),
        lwz(23, 22, (COUNTER & 0xFFFF) as i32),
        addi(23, 23, 1),
        stw(23, 22, (COUNTER & 0xFFFF) as i32),
        RFI,
    ];
    let main = vec![
        lis(3, 0xCC00),
        ori(3, 3, 0x2000),
        li(4, (240 << 4) | 6),
        sth(4, 3, 0x00), // VTR: 240 active lines
        li(4, (40 << 8) | 40),
        sth(4, 3, 0x48), // HSW: 640 pixels, 1280-byte stride (both in 32-byte units)
        lis(4, 0x1000),
        ori(4, 4, (XFB & 0x00FF_FFFF) >> 5),
        stw(4, 3, 0x1C), // TFBL, in 32-byte units
        lis(21, 0x1000 | 200),
        stw(21, 3, 0x30), // DI0: enabled, line 200
        li(4, 5),
        sth(4, 3, 0x02), // DCR: enable, progressive
        lis(5, 0xCC00),
        ori(5, 5, 0x3000),
        li(4, 0x100),
        stw(4, 5, 0x04), // PI mask: VI
        lis(6, XFB >> 16),
        lis(8, RED_YUYV >> 16),
        ori(8, 8, RED_YUYV & 0xFFFF),
        lis(7, 1),
        ori(7, 7, 0x2C00), // 76800 words = 640 x 240 / 2
        mtctr(7),
        stw(8, 6, 0),
        addi(6, 6, 4),
        BDNZ_BACK_8,
        x(9, 0, 0, 83),        // mfmsr r9
        ori(9, 9, 0x8000),     // EE
        x(9, 0, 0, 146),       // mtmsr r9
        HANG,
    ];
    dol(&[(0x8000_0500, handler), (0x8000_3100, main)], 0x8000_3100)
}

fn booted(program: &[u8]) -> Machine {
    let mut m = Machine::new(Console::GameCube);
    m.boot_executable(&Executable::parse(program).unwrap());
    m
}

fn counter(m: &mut Machine) -> u32 {
    use continuum_gc::bus::Bus;
    m.hw.read32(COUNTER & 0x01FF_FFFF)
}

#[test]
fn shows_the_programs_framebuffer() {
    let mut m = booted(&vi_program());
    for _ in 0..3 {
        m.run_field(20_000_000);
    }
    let (w, h, px) = m.hw.vi_picture().expect("the display is on");
    assert_eq!((w, h), (640, 240));
    let red = yuv_to_rgb(0x52, 0x5A, 0xF0);
    assert!(red >> 16 > 250 && red & 0xFFFF < 0x0200, "{red:06x}");
    assert!(px.iter().all(|&p| p == red));
}

#[test]
fn vi_interrupts_reach_the_cpu_once_a_frame() {
    let mut m = booted(&vi_program());
    // Two fields make a frame; line 200 comes once a frame.
    for _ in 0..8 {
        m.run_field(20_000_000);
    }
    let n = counter(&mut m);
    assert!((3..=5).contains(&n), "{n} interrupts in 4 frames");
}

#[test]
fn nothing_is_shown_before_the_program_turns_the_display_on() {
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    m.run_field(20_000_000);
    assert!(m.hw.vi_picture().is_none());
}

#[test]
fn save_states_round_trip_mid_run() {
    let mut m = booted(&vi_program());
    for _ in 0..3 {
        m.run_field(20_000_000);
    }
    let state = m.save_state();
    for _ in 0..4 {
        m.run_field(20_000_000);
    }
    let later = counter(&mut m);
    let mut again = Machine::new(Console::GameCube);
    again.load_state(&state).unwrap();
    for _ in 0..4 {
        again.run_field(20_000_000);
    }
    assert_eq!(counter(&mut again), later);
    assert_eq!(again.cpu.pc, m.cpu.pc);
    assert!(again.load_state(b"nope").is_err());
}

/// A program that waits in a loop for a flag its decrementer handler sets, counts the rounds
/// and waits again: the fast interpreter fast-forwards the waiting and must still end up in
/// exactly the plain interpreter's state.
fn waiting_program() -> Vec<u8> {
    const FLAG: u32 = 0x8000_2100;
    // Decrementer handler (translation off): flag = 1, DEC = 20000, rfi.
    let handler = vec![
        lis(20, (FLAG & 0x01FF_FFFF) >> 16),
        li(21, 1),
        stw(21, 20, (FLAG & 0xFFFF) as i32),
        lis(21, 0),
        ori(21, 21, 20000),
        (31 << 26) | (21 << 21) | (22 << 16) | (467 << 1), // mtdec r21
        RFI,
    ];
    let main = vec![
        lis(3, FLAG >> 16),
        ori(3, 3, FLAG & 0xFFFF),
        lis(21, 0),
        ori(21, 21, 20000),
        (31 << 26) | (21 << 21) | (22 << 16) | (467 << 1), // mtdec r21
        x(9, 0, 0, 83),    // mfmsr r9
        ori(9, 9, 0x8000), // EE
        x(9, 0, 0, 146),   // mtmsr r9
        // wait: lwz r4,0(r3); cmpwi r4,0; beq wait
        lwz(4, 3, 0),
        d(11, 0, 4, 0),
        (16 << 26) | (12 << 21) | (2 << 16) | (0xFFF8 & 0xFFFC),
        // flag = 0; rounds += 1; back to waiting
        li(4, 0),
        stw(4, 3, 0),
        lwz(5, 3, 4),
        addi(5, 5, 1),
        stw(5, 3, 4),
        (18 << 26) | ((-32i32 as u32) & 0x03FF_FFFC),
    ];
    dol(&[(0x8000_0900, handler), (0x8000_3100, main)], 0x8000_3100)
}

#[test]
fn fast_interpreter_fast_forwards_waiting_exactly() {
    let mut plain = booted(&waiting_program());
    plain.fast = None;
    let mut fast = booted(&waiting_program());
    for _ in 0..20 {
        plain.run_field(8_000_000);
        fast.run_field(8_000_000);
    }
    use continuum_gc::bus::Bus;
    let rounds = |m: &mut Machine| m.hw.read32(0x2104);
    assert!(rounds(&mut plain) > 100, "the handler ran {} times", rounds(&mut plain));
    assert_eq!(rounds(&mut plain), rounds(&mut fast));
    assert_eq!(plain.cpu.gpr, fast.cpu.gpr);
    assert_eq!((plain.cpu.pc, plain.cpu.cycles, plain.cpu.tb, plain.cpu.dec), (fast.cpu.pc, fast.cpu.cycles, fast.cpu.tb, fast.cpu.dec));
    assert!(fast.fast.as_ref().unwrap().stats.idle_skips > 1000, "waiting was not fast-forwarded");
}
