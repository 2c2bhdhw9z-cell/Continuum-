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

#[test]
fn the_clock_starts_from_the_phone_and_counts_on() {
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    // 24 October 2026, 12:00 local: seconds since 2000.
    let now = 846_244_800;
    m.set_clock(now);
    // The time base is what games read the date from: 40.5 million ticks a second.
    assert_eq!(m.cpu.tb, now as u64 * 40_500_000);
    assert_eq!(m.hw.exi.rtc, now);
    // A second of video lines (NTSC: 525 lines, 29.97 frames) moves the clock on by one.
    for _ in 0..15_734 {
        m.hw.vi_line();
    }
    assert_eq!(m.hw.exi.rtc, now);
    m.hw.vi_line();
    assert_eq!(m.hw.exi.rtc, now + 1);
}

#[test]
fn a_memory_card_is_in_slot_a_and_survives_save_states() {
    let mut m = booted(&vi_program());
    assert_eq!(m.hw.card.len(), 2 << 20);
    let blank = m.hw.card.clone();
    m.run_field(20_000_000);
    let state = m.save_state();
    // A game saves after the state was made: loading the state keeps the save (the card is the
    // game's save file, not part of the state).
    m.hw.card[0x6000] = 0x42;
    m.load_state(&state).unwrap();
    assert_eq!(m.hw.card[0x6000], 0x42);
    assert_eq!(&m.hw.card[..0x6000], &blank[..0x6000]);
    // A state from an older layout is refused with a reason.
    let mut old = state.clone();
    old[3] = b'2';
    assert!(m.load_state(&old).unwrap_err().contains("older version"));
}

#[test]
fn controller_input_arrives_with_each_poll() {
    use continuum_gc::bus::Bus;
    use continuum_gc::hw::{pad, INT_SI};
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    m.hw.si.pads[0].connected = true;
    m.hw.si.pads[0].buttons = pad::A | pad::START;
    // SIPOLL: every 7 lines, twice a field, channels 0 and 1 (channel 1 has no controller).
    m.hw.write32(0x0C00_6430, (7 << 16) | (2 << 8) | (0xC << 4));
    let sr = |m: &mut Machine| m.hw.read32(0x0C00_6438);
    assert_eq!(sr(&mut m) & 0x2000_0000, 0, "no input before the first poll");
    for _ in 0..7 {
        m.hw.vi_line();
    }
    assert_ne!(sr(&mut m) & 0x2000_0000, 0, "RDST: channel 0 has new input");
    assert_ne!(sr(&mut m) & 0x0008_0000 & 0x00FF_0000, 0, "NOREP: nobody on channel 1");
    let word = m.hw.read32(0x0C00_6404);
    assert_eq!((word >> 16) as u16 & (pad::A | pad::START), pad::A | pad::START);
    assert_eq!(sr(&mut m) & 0x2000_0000, 0, "reading the buffer takes the input");
    // RDSTINT with its mask asks for the SI interrupt; reading the input stops it.
    m.hw.write32(0x0C00_6434, 1 << 27);
    for _ in 0..7 {
        m.hw.vi_line();
    }
    assert_ne!(m.hw.read32(0x0C00_6434) & (1 << 28), 0, "RDSTINT");
    assert_ne!(m.hw.cause() & INT_SI, 0);
    m.hw.read32(0x0C00_6404);
    assert_eq!(m.hw.cause() & INT_SI, 0);
    // Writing 1 to NOREP clears it.
    m.hw.write32(0x0C00_6438, 0x0008_0000);
    assert_eq!(sr(&mut m) & 0x0008_0000, 0);
}

#[test]
fn an_immediate_exi_transfer_takes_its_time_on_the_wire() {
    use continuum_gc::bus::Bus;
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    // Channel 1 (an empty slot), device 0 selected at 1 MHz; a 4-byte write is 32 microseconds.
    m.hw.write32(0x0C00_6814, 0x80);
    m.hw.write32(0x0C00_6814 + 0x10, 0);
    m.hw.write32(0x0C00_6814 + 0x0C, (3 << 4) | (1 << 2) | 1);
    assert_eq!(m.hw.stall, 32 * 486);
    // The same at 32 MHz: one microsecond.
    m.hw.stall = 0;
    m.hw.write32(0x0C00_6814, 0x80 | (5 << 4));
    m.hw.write32(0x0C00_6814 + 0x0C, (3 << 4) | (1 << 2) | 1);
    assert_eq!(m.hw.stall, 486);
    // The machine counts it as time passing.
    let tb = m.cpu.tb;
    m.run_field(1);
    assert!(m.cpu.tb >= tb + 486 / 12, "time base moved on by the transfer");
    assert_eq!(m.hw.stall, 0);
    // The registers repeat every 64 bytes: channel 0's control register at 0x8C.
    assert_eq!(m.hw.read32(0x0C00_688C), m.hw.read32(0x0C00_680C));
}

/// A disc that is just bytes.
struct DiscBytes(Vec<u8>);
impl continuum_gc::disc::Disc for DiscBytes {
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        buf.fill(0);
        let start = (offset as usize).min(self.0.len());
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(())
    }
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
}

#[test]
fn the_drive_streams_music_into_the_sound_output() {
    use continuum_gc::bus::Bus;
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    // 50 blocks at 0x8000: left code 1 and right code 2, filter 0, shift 0 (4096 and 8192).
    let mut disc = vec![0u8; 0x1_0000];
    for b in 0..50 {
        let at = 0x8000 + 32 * b;
        disc[at..at + 4].fill(0x00);
        disc[at + 4..at + 32].fill(0x21);
    }
    m.hw.disc = Some(Box::new(DiscBytes(disc)));
    let di = |m: &mut Machine, cmd: [u32; 3]| {
        m.hw.write32(0x0C00_6008, cmd[0]);
        m.hw.write32(0x0C00_600C, cmd[1]);
        m.hw.write32(0x0C00_6010, cmd[2]);
        m.hw.write32(0x0C00_601C, 1);
        m.hw.read32(0x0C00_6020)
    };
    di(&mut m, [0xE100_0000, 0x8000 >> 2, 32 * 50]);
    assert_eq!(di(&mut m, [0xE200_0000, 0, 0]), 1, "playing");
    assert_eq!(di(&mut m, [0xE202_0000, 0, 0]), 0x8000 >> 2, "track start");
    assert_eq!(di(&mut m, [0xE203_0000, 0, 0]), 32 * 50, "track length");
    // The audio interface plays the stream at 48 kHz, full volume on the left, half on the right.
    m.hw.write32(0x0C00_6C04, 0x80FF);
    m.hw.write32(0x0C00_6C00, 0x3);
    m.hw.audio_out.clear();
    for _ in 0..10 {
        m.hw.audio_line();
    }
    let out = std::mem::take(&mut m.hw.audio_out);
    assert!(out.len() >= 2 * 28);
    let (left, right) = ((4096 * 255 / 256) as i16, (8192 * 128 / 256) as i16);
    assert!(out.chunks(2).all(|f| f[0] == left && f[1] == right), "{:?}", &out[..8]);
    // Stopped: silence.
    di(&mut m, [0xE101_0000, 0, 0]);
    assert_eq!(di(&mut m, [0xE200_0000, 0, 0]), 0);
    for _ in 0..3 {
        m.hw.audio_line();
    }
    assert!(m.hw.audio_out.iter().all(|s| *s == 0));
}

// ---------------------------------------------------------------------- booting a disc

const NOP: u32 = 0x6000_0000;
const BLR: u32 = 0x4E80_0020;
fn cmpwi(a: u32, v: i32) -> u32 {
    d(11, 0, a, v)
}
/// bne: branch `off` bytes on (bc 4,2).
fn bne(off: i32) -> u32 {
    (16 << 26) | (4 << 21) | (2 << 16) | (off as u32 & 0xFFFC)
}

/// A disc whose apploader is written the way Nintendo's are: its globals (the report function,
/// how far it got) live in the boot ROM's small-data area, through r13, and it prints a line.
/// It asks for one piece of the game (16 bytes of code at disc offset 0x10000, loaded at
/// 0x80003100), then hands over to it. The game stores 0x77 at 0x80002000 and waits.
fn sdk_style_disc(region: u32) -> Vec<u8> {
    const APP: u32 = 0x8120_0000;
    let mut app = vec![
        // entry(&init, &main, &close)
        lis(6, APP >> 16), ori(6, 6, 0x40), stw(6, 3, 0),
        lis(6, APP >> 16), ori(6, 6, 0x80), stw(6, 4, 0),
        lis(6, APP >> 16), ori(6, 6, 0xE0), stw(6, 5, 0),
        BLR,
    ];
    app.resize(0x40 / 4, NOP);
    // init(report): remember it, clear the state, report a line.
    app.extend([
        stw(3, 13, -0x7FF0), li(0, 0), stw(0, 13, -0x7FEC),
        0x7C08_02A6, // mflr r0
        stw(0, 1, 4), d(37, 1, 1, -16), // stwu r1,-16(r1)
        mtctr(3), lis(3, APP >> 16), ori(3, 3, 0x100),
        0x4E80_0421, // bctrl
        lwz(0, 1, 20), 0x7C08_03A6, addi(1, 1, 16), BLR,
    ]);
    app.resize(0x80 / 4, NOP);
    // main(&dst, &len, &offset): one request, then done.
    app.extend([
        lwz(6, 13, -0x7FEC), cmpwi(6, 0), bne(0xBC - 0x88),
        lis(7, 0x8000), ori(7, 7, 0x3100), stw(7, 3, 0),
        li(7, 16), stw(7, 4, 0),
        lis(7, 1), ori(7, 7, 0), stw(7, 5, 0),
        li(6, 1), stw(6, 13, -0x7FEC),
        li(3, 1), BLR,
        li(3, 0), BLR,
    ]);
    app.resize(0xE0 / 4, NOP);
    // close(): the game's entry point.
    app.extend([lis(3, 0x8000), ori(3, 3, 0x3100), BLR]);
    app.resize(0x100 / 4, NOP);
    let mut code: Vec<u8> = app.iter().flat_map(|w| w.to_be_bytes()).collect();
    code.extend(b"Apploader test\0\0");
    code.resize(0x120, 0);

    let mut img = vec![0u8; 0x10100];
    img[0..6].copy_from_slice(b"GTSE01");
    img[0x1C..0x20].copy_from_slice(&0xC233_9F3Du32.to_be_bytes());
    img[0x20..0x2A].copy_from_slice(b"Boot test\0");
    img[0x458..0x45C].copy_from_slice(&region.to_be_bytes());
    img[0x2440..0x244A].copy_from_slice(b"2002/01/01");
    img[0x2450..0x2454].copy_from_slice(&(APP).to_be_bytes());
    img[0x2454..0x2458].copy_from_slice(&(code.len() as u32).to_be_bytes());
    img[0x2460..0x2460 + code.len()].copy_from_slice(&code);
    let game = [li(3, 0x77), lis(4, 0x8000), stw(3, 4, 0x2000), HANG];
    for (k, w) in game.iter().enumerate() {
        img[0x10000 + 4 * k..0x10004 + 4 * k].copy_from_slice(&w.to_be_bytes());
    }
    img
}

#[test]
fn a_disc_boots_through_an_apploader_that_keeps_its_globals_in_small_data() {
    use continuum_gc::bus::Bus;
    let mut m = Machine::new(Console::GameCube);
    let disc = continuum_gc::disc::MemoryImage(sdk_style_disc(1));
    let header = m.boot_disc(Box::new(disc)).expect("the disc boots");
    assert_eq!(header.game_id, "GTSE01");
    assert_eq!(m.cpu.pc, 0x8000_3100);
    assert_eq!(m.apploader_log, vec!["Apploader test".to_string()]);
    // What the boot ROM leaves: the disc ID, the console type, rfi at three vectors, the IPL's
    // stack and small-data pointers; the arena is the game's own business.
    assert_eq!(&m.hw.mem1[0..6], b"GTSE01");
    assert_eq!(m.hw.read32(0x2C), continuum_gc::machine::DISC_CONSOLE_TYPE);
    for v in [0x300, 0x800, 0xC00] {
        assert_eq!(m.hw.read32(v), RFI);
    }
    assert_eq!(m.hw.read32(0x34), 0);
    assert_eq!((m.cpu.gpr[1], m.cpu.gpr[2], m.cpu.gpr[13]), (0x8156_6550, 0x8146_5CC0, 0x8146_5320));
    m.run_field(2_000_000);
    assert_eq!(m.hw.read32(0x2000), 0x77, "the game ran");
}

#[test]
fn a_pal_disc_gets_the_pal_boot_rom_pointers_and_video_mode() {
    use continuum_gc::bus::Bus;
    let mut m = Machine::new(Console::GameCube);
    m.boot_disc(Box::new(continuum_gc::disc::MemoryImage(sdk_style_disc(2)))).expect("boots");
    assert_eq!((m.cpu.gpr[1], m.cpu.gpr[2], m.cpu.gpr[13]), (0x815E_DCA8, 0x814B_5B20, 0x814B_4FC0));
    assert_eq!(m.hw.read32(0xCC), 1);
}

#[test]
fn an_apploader_that_faults_is_refused_at_once_saying_where() {
    let mut img = sdk_style_disc(1);
    // entry's first instruction: lwz r6, 0(0), an access to unmapped memory.
    img[0x2460..0x2464].copy_from_slice(&lwz(6, 0, 0).to_be_bytes());
    let mut m = Machine::new(Console::GameCube);
    let t = std::time::Instant::now();
    let err = m.boot_disc(Box::new(continuum_gc::disc::MemoryImage(img))).unwrap_err();
    assert!(err.contains("apploader entry faulted") && err.contains("81200000"), "{err}");
    assert!(t.elapsed().as_secs() < 2, "refused at once");
    let line = continuum_gc::libretro::refusal(&err);
    assert!(line.starts_with("Continuum GC could not start the game: this game's start-up program stopped."), "{line}");
}

#[test]
fn a_disc_read_completes_a_moment_after_it_starts() {
    use continuum_gc::bus::Bus;
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    let mut disc = vec![0u8; 0x1000];
    disc[0x800..0x820].copy_from_slice(&[0x5A; 32]);
    m.hw.disc = Some(Box::new(DiscBytes(disc)));
    // DISR: the transfer-complete interrupt unmasked. Then a 32-byte read from 0x800 to 0x4000.
    m.hw.write32(0x0C00_6000, 0x08);
    m.hw.write32(0x0C00_6008, 0xA800_0000);
    m.hw.write32(0x0C00_600C, 0x800 >> 2);
    m.hw.write32(0x0C00_6010, 32);
    m.hw.write32(0x0C00_6014, 0x4000);
    m.hw.write32(0x0C00_6018, 32);
    m.hw.write32(0x0C00_601C, 3);
    // Still going right after the write: the library sets its timeout next.
    assert_eq!(m.hw.read32(0x0C00_601C) & 1, 1, "TSTART still set");
    assert_eq!(m.hw.read32(0x0C00_6000) & 0x10, 0, "no TCINT yet");
    assert_eq!(m.hw.cause() & continuum_gc::hw::INT_DI, 0);
    for _ in 0..continuum_gc::hw::DI_COMMAND_LINES {
        m.hw.vi_line();
    }
    assert_eq!(m.hw.read32(0x0C00_601C) & 1, 0, "done");
    assert_ne!(m.hw.read32(0x0C00_6000) & 0x10, 0, "TCINT");
    assert_ne!(m.hw.cause() & continuum_gc::hw::INT_DI, 0, "the interrupt is asked for");
    assert_eq!(&m.hw.mem1[0x4000..0x4020], &[0x5A; 32]);
}

/// The sound chip's start-up exactly as Nintendo's OS runs it (OSInit's __OSInitAudioSystem,
/// as decompiled by the Animal Crossing project), register by register: every wait it makes
/// must end.
#[test]
fn the_os_audio_start_up_sequence_never_waits_forever() {
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    os_audio_init(&mut m);
}

/// DSP registers by halfword (0: CPU mailbox high, 2-3: the DSP's mailbox, 5: control).
fn r16(m: &mut Machine, i: u32) -> u16 {
    use continuum_gc::bus::Bus;
    m.hw.read16(0x0C00_5000 + 2 * i)
}
fn w16(m: &mut Machine, i: u32, v: u16) {
    use continuum_gc::bus::Bus;
    m.hw.write16(0x0C00_5000 + 2 * i, v)
}
fn w32(m: &mut Machine, i: u32, v: u32) {
    use continuum_gc::bus::Bus;
    m.hw.write32(0x0C00_5000 + 2 * i, v)
}
fn wait(m: &mut Machine, what: &str, done: &dyn Fn(&mut Machine) -> bool) {
    for _ in 0..10_000 {
        if done(m) {
            return;
        }
        m.hw.vi_line();
    }
    panic!("waits forever: {what}");
}

/// OSInit's sound-chip start-up (__OSInitAudioSystem), register by register.
fn os_audio_init(m: &mut Machine) {
    w16(m, 9, 0x43);
    w16(m, 5, 0x8AC);
    let v = r16(m, 5);
    w16(m, 5, v | 1);
    wait(m, "reset", &|m| r16(m, 5) & 1 == 0);
    w16(m, 0, 0);
    wait(m, "mailbox from the DSP empty", &|m| ((r16(m, 2) as u32) << 16 | r16(m, 3) as u32) & 0x8000_0000 == 0);
    for _ in 0..2 {
        w32(m, 16, 0x0100_0000);
        w32(m, 18, 0);
        w32(m, 20, 0x20);
        wait(m, "ARAM DMA interrupt", &|m| r16(m, 5) & 0x20 != 0);
        let v = r16(m, 5);
        w16(m, 5, v);
    }
    let v = r16(m, 5);
    w16(m, 5, v & !0x800);
    wait(m, "DSP DMA idle", &|m| r16(m, 5) & 0x400 == 0);
    let v = r16(m, 5);
    w16(m, 5, v & !4);
    wait(m, "the init program's mail", &|m| r16(m, 2) & 0x8000 != 0);
    let v = r16(m, 5);
    w16(m, 5, v | 4);
    w16(m, 5, 0x8AC);
    let v = r16(m, 5);
    w16(m, 5, v | 1);
    wait(m, "second reset", &|m| r16(m, 5) & 1 == 0);
}

/// The OS's ARAM size check (ARInit's __ARChecksize, as decompiled by the Animal Crossing
/// project): a retail console has 16 MB and no expansion, so nothing it writes past 16 MB
/// reads back.
#[test]
fn aram_sizing_finds_16_mb_and_no_expansion() {
    use continuum_gc::bus::Bus;
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    let (test, dummy, buffer) = (0x0010_0000u32, 0x0010_0020u32, 0x0010_0040u32);
    m.hw.dma_write(test, &[0xDE, 0xAD, 0xBE, 0xEF].repeat(8));
    m.hw.dma_write(dummy, &[0xBA, 0xD0, 0xBA, 0xD0].repeat(8));
    let dma = |m: &mut Machine, mm: u32, aram: u32, to_main: bool| {
        m.hw.write32(0x0C00_5020, mm);
        m.hw.write32(0x0C00_5024, aram);
        m.hw.write32(0x0C00_5028, if to_main { 0x8000_0000 } else { 0 } | 0x20);
    };
    let size = 0x0100_0000;
    for off in [0, 0x20_0000, 0x100_0000, 0x200, 0x40_0000] {
        dma(&mut m, dummy, size + off, false);
    }
    dma(&mut m, test, size, false);
    m.hw.dma_write(buffer, &[0; 32]);
    dma(&mut m, buffer, size, true);
    assert_ne!(m.hw.read32(buffer), 0xDEAD_BEEF, "nothing past 16 MB reads back");
    // ARAM itself is untouched by writes past its end, and reads back what it holds.
    dma(&mut m, test, 0x1000, false);
    dma(&mut m, buffer, 0x1000, true);
    assert_eq!(m.hw.read32(buffer), 0xDEAD_BEEF);
    assert_eq!(&m.hw.aram[0..4], &[0, 0, 0, 0], "a write past the end did not wrap to the start");
}

// ---------------------------------------------------------------------- the audio interface's clock

/// A small assembler for the test programs below: instruction words, and labels for branches.
struct Prog {
    code: Vec<u32>,
    labels: Vec<(&'static str, usize)>,
    fixups: Vec<(usize, &'static str)>,
}

impl Prog {
    fn new() -> Self {
        Prog { code: Vec::new(), labels: Vec::new(), fixups: Vec::new() }
    }
    fn op(&mut self, w: u32) -> &mut Self {
        self.code.push(w);
        self
    }
    fn label(&mut self, name: &'static str) -> &mut Self {
        self.labels.push((name, self.code.len()));
        self
    }
    /// bc BO,BI,label.
    fn bc(&mut self, bo: u32, bi: u32, to: &'static str) -> &mut Self {
        self.fixups.push((self.code.len(), to));
        self.op((16 << 26) | (bo << 21) | (bi << 16))
    }
    fn b(&mut self, to: &'static str) -> &mut Self {
        self.fixups.push((self.code.len(), to));
        self.op(18 << 26)
    }
    fn words(&self) -> Vec<u32> {
        let mut code = self.code.clone();
        for &(at, to) in &self.fixups {
            let target = self.labels.iter().find(|l| l.0 == to).expect("label").1;
            let off = ((target as i64 - at as i64) * 4) as u32;
            code[at] |= if code[at] >> 26 == 16 { off & 0xFFFC } else { off & 0x03FF_FFFC };
        }
        code
    }
}

fn mftb(t: u32) -> u32 {
    (31 << 26) | (t << 21) | (392 << 11) | (371 << 1)
}
fn cmplw(a: u32, b: u32) -> u32 {
    x(0, a, b, 32)
}
fn cmplwi(a: u32, v: u32) -> u32 {
    d(10, 0, a, v as i32)
}
fn rlwinm(a: u32, s: u32, sh: u32, mb: u32, me: u32) -> u32 {
    (21 << 26) | (s << 21) | (a << 16) | (sh << 11) | (mb << 6) | (me << 1)
}
/// beq, blt, bdnz.
const EQ: (u32, u32) = (12, 2);
const LT: (u32, u32) = (12, 0);
const AI: u32 = 0xCC00_6C00;
const RESULTS: u32 = 0x8000_2000;

/// AICR |= bits, read-modify-write as Nintendo's library does (r3 = the AI registers).
fn ai_set(p: &mut Prog, bits: u32) {
    p.op(lwz(0, 3, 0)).op(ori(0, 0, bits)).op(stw(0, 3, 0));
}
/// AICR &= !bit (bit 0 or 1).
fn ai_clear(p: &mut Prog, bit: u32) {
    let (mb, me) = if bit == 1 { (0, 30) } else { (31, 29) };
    p.op(lwz(0, 3, 0)).op(rlwinm(0, 0, 0, mb, me)).op(stw(0, 3, 0));
}

/// The audio interface's start-up timing check exactly as Nintendo's library runs it
/// (__AI_SRC_INIT, as decompiled by the Animal Crossing project; AIInit and the first
/// AISetStreamPlayState call it). Animal Crossing sat in it forever on build 219: the sample
/// counter stepped once a video line, so every step measured 63.5 microseconds and none
/// passed. It starts the stream at 32 kHz, waits for the counter to step, switches to 48 kHz,
/// waits for the next step and times it with the time base, and tries again until the time is
/// under 28.5 microseconds or between 34.5 and 39 (in time-base ticks at 40.5 MHz: under 1155,
/// or 1397 to 1579). Done four times; each round's time and attempts go to RESULTS.
fn ai_src_init_program() -> Vec<u8> {
    let mut p = Prog::new();
    p.op(lis(3, AI >> 16)).op(ori(3, 3, AI & 0xFFFF));
    p.op(lis(8, RESULTS >> 16)).op(ori(8, 8, RESULTS & 0xFFFF));
    p.op(li(30, 0));
    p.label("round").op(li(31, 0));
    p.label("attempt").op(addi(31, 31, 1));
    ai_set(&mut p, 0x20); // SCRESET
    ai_clear(&mut p, 1); // 32 kHz
    ai_set(&mut p, 1); // play
    p.op(lwz(4, 3, 8));
    p.label("w1").op(lwz(0, 3, 8)).op(cmplw(0, 4)).bc(EQ.0, EQ.1, "w1");
    p.op(mftb(5));
    ai_set(&mut p, 2); // 48 kHz
    ai_set(&mut p, 1);
    p.op(lwz(4, 3, 8));
    p.label("w2").op(lwz(0, 3, 8)).op(cmplw(0, 4)).bc(EQ.0, EQ.1, "w2");
    p.op(mftb(6));
    p.op(x(7, 5, 6, 40)); // subf r7, r5, r6: the time between the steps
    ai_clear(&mut p, 1);
    ai_clear(&mut p, 0); // stop
    p.op(rlwinm(9, 30, 3, 0, 28)).op(x(9, 9, 8, 266)); // r9 = RESULTS + 8 * round
    p.op(stw(7, 9, 0)).op(stw(31, 9, 4));
    p.op(cmplwi(7, 1155)).bc(LT.0, LT.1, "accepted");
    p.op(cmplwi(7, 1397)).bc(LT.0, LT.1, "attempt");
    p.op(cmplwi(7, 1580)).bc(LT.0, LT.1, "accepted");
    p.b("attempt");
    p.label("accepted").op(addi(30, 30, 1)).op(stw(30, 8, 0x100));
    p.op(cmplwi(30, 4)).bc(LT.0, LT.1, "round");
    p.op(HANG);
    dol(&[(0x8000_3100, p.words())], 0x8000_3100)
}

/// The same program on the plain interpreter, the fast interpreter and (where this machine can
/// run it) our JIT, for `fields` fields; their results.
fn on_every_cpu(program: &[u8], fields: usize, results: u32) -> Vec<(&'static str, Vec<u32>)> {
    use continuum_gc::bus::Bus;
    let mut out = Vec::new();
    for mode in ["plain", "fast", "jit"] {
        let mut m = booted(program);
        match mode {
            "plain" => m.fast = None,
            "jit" if !m.enable_jit() => {
                eprintln!("(no JIT code memory on this machine: build with --features unicorn to check the JIT too)");
                continue;
            }
            _ => {}
        }
        for _ in 0..fields {
            m.run_field(20_000_000);
        }
        let words = (0..results).map(|k| m.hw.read32((RESULTS & 0x01FF_FFFF) + 4 * k)).collect();
        if mode == "jit" {
            let f = m.fast.as_ref().unwrap();
            assert!(f.stats.jit_instructions > 0, "the JIT ran nothing");
        }
        out.push((mode, words));
    }
    out
}

#[test]
fn the_audio_start_up_timing_check_finishes_on_every_cpu() {
    use continuum_gc::bus::Bus;
    for (mode, r) in on_every_cpu(&ai_src_init_program(), 2, 4 * 2) {
        for round in 0..4 {
            let (ticks, attempts) = (r[2 * round], r[2 * round + 1]);
            // One 48 kHz step (10,116 CPU cycles, 843 ticks) and the few instructions around it.
            assert!((843..900).contains(&ticks), "{mode}: round {round} measured {ticks} ticks");
            assert_eq!(attempts, 1, "{mode}: round {round} needed {attempts} attempts");
        }
    }
    let mut m = booted(&ai_src_init_program());
    m.run_field(20_000_000);
    assert_eq!(m.hw.read32((RESULTS & 0x01FF_FFFF) + 0x100), 4, "all four rounds done within a field");
}

/// Restarts the 48 kHz stream, waits a different number of instructions each time (a loop the
/// JIT runs as one chain of compiled blocks), then reads the sample counter: 24 readings.
fn ai_counter_program() -> Vec<u8> {
    let mut p = Prog::new();
    p.op(lis(3, AI >> 16)).op(ori(3, 3, AI & 0xFFFF));
    p.op(lis(8, RESULTS >> 16)).op(ori(8, 8, RESULTS & 0xFFFF));
    p.op(li(30, 0));
    p.label("next").op(li(0, 0x23)).op(stw(0, 3, 0)); // SCRESET, 48 kHz, play
    p.op(d(7, 10, 30, 517)).op(addi(10, 10, 3000)).op(mtctr(10)); // ctr = 3000 + 517 * k
    p.label("delay").op(addi(11, 11, 1)).bc(16, 0, "delay");
    p.op(lwz(4, 3, 8));
    p.op(rlwinm(9, 30, 2, 0, 29)).op(x(9, 9, 8, 266)).op(stw(4, 9, 0));
    p.op(addi(30, 30, 1)).op(cmplwi(30, 24)).bc(LT.0, LT.1, "next");
    p.op(HANG);
    dol(&[(0x8000_3100, p.words())], 0x8000_3100)
}

#[test]
fn the_sample_counter_reads_the_same_on_every_cpu() {
    let runs = on_every_cpu(&ai_counter_program(), 1, 24);
    let (_, plain) = &runs[0];
    // From the restart: the store itself, three instructions setting up the loop, two cycles a
    // round of it; 10,116 cycles a sample.
    for (k, &n) in plain.iter().enumerate() {
        let elapsed = 4 + 2 * (3000 + 517 * k as u64);
        assert_eq!(n as u64, elapsed / 10_116, "reading {k}");
    }
    assert!(plain.contains(&2), "{plain:?}");
    for (mode, r) in &runs[1..] {
        assert_eq!(r, plain, "{mode} reads the counter differently");
    }
}

#[test]
fn the_sample_counter_counts_at_the_streams_rate_and_interrupts_at_aiit() {
    use continuum_gc::bus::Bus;
    use continuum_gc::hw::{AI_AIINT, INT_AI};
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    let at = |m: &mut Machine, cycles: u64| m.hw.now = cycles;
    // 48 kHz for one second of CPU time: 54 MHz / 1124 samples.
    m.hw.write32(0x0C00_6C00, 0x23);
    at(&mut m, 486_000_000);
    assert_eq!(m.hw.read32(0x0C00_6C08), 48_042);
    // Stopped, it holds; 32 kHz for one more second: 54 MHz / 1686.
    m.hw.write32(0x0C00_6C00, 0);
    at(&mut m, 2 * 486_000_000);
    assert_eq!(m.hw.read32(0x0C00_6C08), 48_042);
    m.hw.write32(0x0C00_6C00, 0x21);
    assert_eq!(m.hw.read32(0x0C00_6C08), 0, "SCRESET");
    at(&mut m, 3 * 486_000_000);
    assert_eq!(m.hw.read32(0x0C00_6C08), 32_028);
    // AIINT when the counter reaches AIIT, asked for when unmasked; writing 1 clears it.
    m.hw.write32(0x0C00_6C0C, 32_100);
    m.hw.write32(0x0C00_6C00, 0x01 | 0x04);
    at(&mut m, 3 * 486_000_000 + 71 * 15_174);
    m.hw.ai_line();
    assert_eq!(m.hw.read32(0x0C00_6C00) & AI_AIINT, 0, "one sample short");
    at(&mut m, 3 * 486_000_000 + 72 * 15_174);
    m.hw.ai_line();
    assert_ne!(m.hw.read32(0x0C00_6C00) & AI_AIINT, 0, "reached AIIT");
    assert_ne!(m.hw.cause() & INT_AI, 0);
    m.hw.write32(0x0C00_6C00, 0x01 | 0x04 | AI_AIINT);
    assert_eq!(m.hw.cause() & INT_AI, 0, "cleared");
    // Save states keep the counter's place.
    let state = m.save_state();
    let mut again = Machine::new(Console::GameCube);
    again.load_state(&state).unwrap();
    again.hw.now = m.hw.now + 10 * 15_174;
    m.hw.now += 10 * 15_174;
    assert_eq!(again.hw.read32(0x0C00_6C08), m.hw.read32(0x0C00_6C08));
}

// ---------------------------------------------------------------------- Animal Crossing's sound start-up

/// Bytes whose Ector checksum (the rotate-xor the community identifies sound programs by) is
/// `target`: our own filler, never anyone's code, `len` bytes long.
fn bytes_with_ector(target: u32, len: usize) -> Vec<u8> {
    let mut tail = Vec::new();
    let mut s = target;
    while s != 0 {
        let x = s.rotate_right(3);
        tail.push((x & 0xFF) as u8);
        s = x & !0xFF;
    }
    tail.reverse();
    let mut out = vec![0u8; len - tail.len()];
    out.extend(tail);
    out
}

fn dsp_mail_waiting(m: &mut Machine) -> bool {
    r16(m, 2) & 0x8000 != 0
}
fn dsp_take_mail(m: &mut Machine) -> u32 {
    wait(m, "a mail from the DSP", &dsp_mail_waiting);
    ((r16(m, 2) as u32) << 16) | r16(m, 3) as u32
}
fn dsp_send(m: &mut Machine, mail: u32) {
    w16(m, 0, (mail >> 16) as u16);
    w16(m, 1, mail as u16);
    wait(m, "the DSP takes the mail", &|m| r16(m, 0) & 0x8000 == 0);
}
/// JAudio's DSPSendCommands and DSPWaitFinish: both mailboxes must be empty first (else the
/// library sends nothing and then waits for ever), then an answer other than 0x88881357.
fn jaudio_command(m: &mut Machine, words: &[u32]) -> u32 {
    assert!(!dsp_mail_waiting(m), "a mail from the DSP is still waiting: the library would not send {:08X}", words[0]);
    for &w in words {
        dsp_send(m, w);
    }
    loop {
        let mail = dsp_take_mail(m);
        if mail != 0x8888_1357 {
            return mail;
        }
    }
}

/// Animal Crossing's sound start-up as its JAudio library runs it (as decompiled by the Animal
/// Crossing project): the OS's sound-chip start-up, then its own program booted through the
/// boot ROM's upload protocol (DSPInit2, __DSPCheckMXICBoot2), the hello, DwaitFrame,
/// DsetupTable, and frames of seven subframes, each released by a zero mail and answered with
/// the DSP interrupt (UpdateDSP, __DspSync). The memory-card decode command (DiplSec) takes
/// its one word without leaving a stray answer behind.
#[test]
fn animal_crossings_sound_program_starts_and_renders_frames() {
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    os_audio_init(&mut m);
    // The program's code at 0x00200000: 6656 bytes with the checksum of Animal Crossing's.
    let code = bytes_with_ector(0x4BE6_A5CB, 6656);
    m.hw.dma_write(0x0020_0000, &code);
    // DSPInit2.
    let v = r16(&mut m, 5);
    w16(&mut m, 5, (v & !0xA8) | 0x800);
    let v = r16(&mut m, 5);
    w16(&mut m, 5, v & !0xAC);
    assert_eq!(dsp_take_mail(&mut m), 0x8071_FEED, "the boot ROM is ready");
    for w in [0x80F3_A001, 0x8020_0000, 0x80F3_A002, 6656, 0x80F3_C002, 0, 0x80F3_B002, 0, 0x80F3_D001, 0] {
        dsp_send(&mut m, w);
    }
    assert_eq!(m.hw.dsp.hle.kind, continuum_gc::dsp_hle::Kind::Zelda);
    // DspBoot: the hello, then DwaitFrame.
    assert_eq!(dsp_take_mail(&mut m), 0x8888_1111);
    assert_eq!(jaudio_command(&mut m, &[0x8000_0000]), 0x8000_0062);
    // DsetupTable: 64 voice blocks (all silent), the resampling and ADPCM tables, the effects.
    for a in [0x0030_0000u32, 0x0031_0000, 0x0032_0000, 0x0033_0000] {
        m.hw.dma_write(a, &vec![0; 0x8000]);
    }
    assert_eq!(jaudio_command(&mut m, &[0x8100_0040, 0x8030_0000, 0x8031_0000, 0x8032_0000, 0x8033_0000]), 0x8000_0064);
    // DiplSec: one word, one answer.
    jaudio_command(&mut m, &[0x8B00_0008, 0x8034_0000]);
    for frame in 0..3 {
        // DsyncFrame: seven subframes into two buffers, then UpdateDSP releases each one.
        jaudio_command(&mut m, &[0x8207_4000, 0x8035_0000, 0x8036_0000]);
        for sub in 0..7 {
            wait(&mut m, "the DSP takes the release", &|m| r16(m, 0) & 0x8000 == 0);
            dsp_send(&mut m, 0);
            if dsp_mail_waiting(&mut m) {
                dsp_take_mail(&mut m);
            }
            let csr = r16(&mut m, 5);
            assert_ne!(csr & 0x80, 0, "frame {frame} subframe {sub}: the DSP interrupt");
            // __DspSync acknowledges it.
            w16(&mut m, 5, (csr & !0x28) | 0x80);
            assert_eq!(r16(&mut m, 5) & 0x80, 0);
        }
    }
    assert!(!dsp_mail_waiting(&mut m), "nothing left over");
}

/// One emulated second through `run_field`, the way retro_run drives it, with the audio DMA
/// running at the DSP rate `cr` selects. The core tells the app its sound is 48000 Hz, so it must
/// hand over ~48000 frames for each second, at either DMA rate. On the owner's iPhone (227, 228)
/// Animal Crossing got ~32031 a second, a third short, filled with silence by the app.
fn sound_frames_in_one_second(fast: bool, cr: u32) -> usize {
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    if !fast {
        m.fast = None;
    }
    m.hw.ai.cr = cr;
    m.hw.ai.vr = 0xFFFF;
    let mut frames = 0;
    for field in 0..120 {
        m.hw.audio_out.clear();
        m.run_one_field();
        if field >= 60 {
            frames += m.hw.audio_out.len() / 2;
        }
    }
    frames
}

#[test]
fn the_machine_hands_over_48000_sound_frames_a_second_at_either_dma_rate() {
    for fast in [false, true] {
        for (cr, what) in [(0x01u32, "48 kHz DMA"), (0x41, "32 kHz DMA"), (0x43, "32 kHz DMA, 48 kHz stream")] {
            let got = sound_frames_in_one_second(fast, cr);
            eprintln!("fast {fast}, {what}: {got} frames in one emulated second");
            assert!((got as i64 - 48_000).abs() < 800, "fast {fast}, {what}: {got} frames, not ~48000");
        }
    }
}

#[test]
fn every_call_runs_one_whole_field() {
    let mut m = booted(&dol(&[(0x8000_3100, vec![HANG])], 0x8000_3100));
    let mut lines = Vec::new();
    for _ in 0..12 {
        let l0 = m.hw.vi_line_count();
        m.run_one_field();
        lines.push(m.hw.vi_line_count() - l0);
    }
    // 262 and 263 lines in turn (an NTSC frame is 525), never the one-line call that used to
    // come every third time.
    assert!(lines[2..].iter().all(|&n| n == 262 || n == 263), "lines per call: {lines:?}");
    assert_eq!(lines[2..].iter().sum::<u64>(), 525 * 5);
}
