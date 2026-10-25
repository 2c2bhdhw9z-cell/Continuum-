//! The whole console: CPU, hardware, the clock that ties them together, and booting.
//!
//! Booting never needs Nintendo's boot ROM (IPL): like a direct boot, the machine sets up what
//! the IPL would have left behind (memory translation, the low-memory globals) and starts the
//! program. For a disc, the disc's own apploader runs on our CPU to load the game, exactly as on
//! the console. A boot picture only ever comes from the player's own IPL file (owner rule; not
//! implemented yet).

use crate::bus::Bus;
use crate::cpu::{self, Cpu, HID2_LSQE, HID2_PSE, HID2_WPE, MSR_DR, MSR_FP, MSR_IR};
use crate::disc::{Apploader, Console, Disc, Header};
use crate::dol::Executable;
use crate::hw::{Hw, GC_CPU_HZ, WII_CPU_HZ};
use serde::{Deserialize, Serialize};

/// Where the boot code parks the CPU between calls into the apploader.
const STUB: u32 = 0x8130_0000;
const STUB_REPORT: u32 = STUB + 0x10;
const APPLOADER_BASE: u32 = 0x8120_0000;
const BOOT_STACK: u32 = 0x816F_FFF0;
/// The console type a disc boots with: the latest development hardware (see `disc_lowmem`).
pub const DISC_CONSOLE_TYPE: u32 = 0x1000_0006;

/// Code memory asked for the JIT: room for the compiled code of a whole game (it is address
/// space until written; when full, everything is compiled again as it runs).
pub const JIT_CODE_BYTES: usize = 64 * 1024 * 1024;

pub struct Machine {
    pub cpu: Cpu,
    pub hw: Hw,
    /// The fast interpreter's block cache; None runs the plain interpreter.
    pub fast: Option<cpu::fast::FastCpu>,
    /// CPU cycles since the last video line.
    line_cycles: u64,
    pub cpu_hz: u64,
    /// What the disc's apploader printed while it loaded the game (development aid, and part of
    /// the reason given when a disc does not start).
    pub apploader_log: Vec<String>,
    /// The game's ID and title (from the disc), for the status screen.
    pub title: String,
    /// Where the CPU is at the end of each video line, by 256-byte block of code, while the
    /// speed read-out is on (None: not sampled).
    pub hot: Option<std::collections::HashMap<u32, u32>>,
}

/// Everything a save state holds (the disc is not part of it).
#[derive(Serialize, Deserialize)]
struct State {
    cpu: Cpu,
    mem1: Vec<u8>,
    mem2: Vec<u8>,
    aram: Vec<u8>,
    pi: crate::hw::Pi,
    vi: crate::hw::Vi,
    si: crate::hw::Si,
    exi: crate::hw::Exi,
    di: crate::hw::Di,
    dsp: crate::hw::Dsp,
    ai: crate::hw::Ai,
    gx: crate::gx::Gx,
    gather: Vec<u8>,
    line_cycles: u64,
}

/// "CGC" and the state's layout version (3: the memory card's command state and the ticking
/// clock in Exi; 4: SI polling, the drive's streamed music; 5: drive commands take time;
/// 6: the audio interface's sample counter counts from the CPU's clock).
pub const STATE_MAGIC: &[u8; 4] = b"CGC6";

impl Machine {
    pub fn new(console: Console) -> Self {
        let wii = console == Console::Wii;
        Machine {
            cpu: Cpu::new(if wii { cpu::PVR_BROADWAY } else { cpu::PVR_GEKKO }),
            hw: Hw::new(wii),
            fast: Some(cpu::fast::FastCpu::new()),
            line_cycles: 0,
            cpu_hz: if wii { WII_CPU_HZ } else { GC_CPU_HZ },
            apploader_log: Vec::new(),
            title: String::new(),
            hot: None,
        }
    }

    /// CPU cycles per video line (NTSC: 525 lines a frame, 29.97 frames a second).
    pub fn cycles_per_line(&self) -> u64 {
        let lines = self.hw.vi_lines_per_frame() as u64;
        let frames_x1000 = if lines == 625 { 25_000 } else { 29_970 };
        self.cpu_hz * 1000 / (lines * frames_x1000)
    }

    // ------------------------------------------------------------------ booting

    /// What the IPL leaves the CPU in: BATs mapping 0x80000000 (cached) and 0xC0000000
    /// (uncached) onto the first 256 MB, translation and FP on, paired singles on.
    fn ipl_cpu_state(&mut self) {
        let c = &mut self.cpu;
        c.ibat[0] = [0x8000_1FFF, 0x0000_0002];
        c.dbat[0] = [0x8000_1FFF, 0x0000_0002];
        c.dbat[1] = [0xC000_1FFF, 0x0000_002A];
        if self.hw.wii {
            // MEM2 at 0x90000000 / 0xD0000000.
            c.ibat[4] = [0x9000_1FFF, 0x1000_0002];
            c.dbat[4] = [0x9000_1FFF, 0x1000_0002];
            c.dbat[5] = [0xD000_1FFF, 0x1000_002A];
            c.hid4 = cpu::HID4_SBE | 0x8200_0000;
        }
        c.msr = MSR_IR | MSR_DR | MSR_FP;
        c.hid0 = 0x0011_0C64;
        c.hid2 = HID2_PSE | HID2_LSQE | HID2_WPE;
        c.gpr[1] = BOOT_STACK;
        c.tlb.flush_all();
    }

    fn w32(&mut self, ea: u32, v: u32) {
        self.hw.write32(ea & 0x1FFF_FFFF, v);
    }

    /// The low-memory globals the OS reads (YAGCD, "the first 0x100 bytes of RAM").
    fn lowmem(&mut self, header: Option<&[u8]>) {
        if let Some(h) = header {
            self.hw.dma_write(0, &h[..0x20]);
        }
        self.w32(0x8000_0020, 0x0D15_EA5E); // boot magic: booted normally
        self.w32(0x8000_0024, 1);
        self.w32(0x8000_0028, crate::hw::MEM1_SIZE as u32);
        self.w32(0x8000_002C, 1); // retail console
        self.w32(0x8000_0034, 0x8170_0000); // arena high
        self.w32(0x8000_00CC, 0); // NTSC
        self.w32(0x8000_00D0, crate::hw::ARAM_SIZE as u32);
        self.w32(0x8000_00F0, crate::hw::MEM1_SIZE as u32);
        self.w32(0x8000_00F8, (self.cpu_hz / 3) as u32); // bus clock
        self.w32(0x8000_00FC, self.cpu_hz as u32);
    }

    /// Starts a DOL or ELF (homebrew) directly.
    pub fn boot_executable(&mut self, exe: &Executable) {
        self.ipl_cpu_state();
        self.lowmem(None);
        self.load(exe);
        self.cpu.pc = exe.entry;
    }

    /// Hands a homebrew program its command line and environment, the way homebrew loaders do:
    /// programs built with the homebrew SDK keep two small blocks right after their entry point,
    /// marked "_arg" and "_env". Each gets a pointer to NUL-separated strings, which the program
    /// copies away first thing. Returns false when the program has no such blocks.
    pub fn set_args(&mut self, entry: u32, argv: &[&str], envp: &[&str]) -> bool {
        const ARGV_MAGIC: u32 = 0x5F61_7267;
        const ENVP_MAGIC: u32 = 0x5F65_6E76;
        let pa = |ea: u32| ea & 0x01FF_FFFF;
        if self.hw.read32(pa(entry + 4)) != ARGV_MAGIC {
            return false;
        }
        let put = |m: &mut Machine, block: u32, magic: u32, strings: &[&str], at: u32| {
            let mut z = Vec::new();
            for s in strings {
                z.extend_from_slice(s.as_bytes());
                z.push(0);
            }
            m.hw.dma_write(pa(at), &z);
            for (k, w) in [magic, at, z.len() as u32, 0, 0, 0].iter().enumerate() {
                m.w32(block + 4 * k as u32, *w);
            }
        };
        if !argv.is_empty() {
            put(self, entry + 8, ARGV_MAGIC, argv, 0x817F_0000);
        }
        if !envp.is_empty() && self.hw.read32(pa(entry + 36)) == ENVP_MAGIC {
            put(self, entry + 40, ENVP_MAGIC, envp, 0x817F_8000);
        }
        true
    }

    fn load(&mut self, exe: &Executable) {
        for s in &exe.sections {
            self.hw.dma_write(s.addr & 0x1FFF_FFFF, &s.data);
        }
        for &(addr, len) in &exe.bss {
            self.hw.dma_write(addr & 0x1FFF_FFFF, &vec![0; len as usize]);
        }
    }

    /// What the boot ROM (IPL) leaves in low memory before it runs a disc's apploader: the disc's
    /// ID, the boot magic, memory and ARAM sizes, the console type, the video mode, the clocks,
    /// and an `rfi` at the DSI, FPU and system call vectors. Nothing else: the apploader and the
    /// game's own OS set the rest (the FST, the arena, the bi2 pointer). The same set as Dolphin's
    /// boot ROM stand-in, which countless commercial games boot with.
    fn disc_lowmem(&mut self, boot: &[u8], pal: bool) {
        self.hw.dma_write(0, &boot[..0x20]);
        self.w32(0x8000_0020, 0x0D15_EA5E); // boot magic: booted normally
        self.w32(0x8000_0024, 1);
        self.w32(0x8000_0028, crate::hw::MEM1_SIZE as u32);
        // A development console's type, as Dolphin uses: with a retail type some games take
        // other paths through the expansion bus and stop (Dolphin names Ikaruga).
        self.w32(0x8000_002C, DISC_CONSOLE_TYPE);
        self.w32(0x8000_00CC, pal as u32); // video mode: 0 NTSC, 1 PAL
        self.w32(0x8000_00D0, crate::hw::ARAM_SIZE as u32);
        self.w32(0x8000_00F8, (self.cpu_hz / 3) as u32); // bus clock
        self.w32(0x8000_00FC, self.cpu_hz as u32);
        for vector in [0x8000_0300, 0x8000_0800, 0x8000_0C00] {
            self.w32(vector, 0x4C00_0064); // rfi
        }
        // The last three post-transform matrices are the identity, as the boot ROM leaves them
        // (some discs use dual texture-coordinate transforms without setting them).
        self.hw.gx.xf_write(0x500 + 0x3D * 4, 1.0f32.to_bits());
        self.hw.gx.xf_write(0x500 + 0x3E * 4 + 1, 1.0f32.to_bits());
        self.hw.gx.xf_write(0x500 + 0x3F * 4 + 2, 1.0f32.to_bits());
    }

    /// The registers the boot ROM runs an apploader with: its own stack and its two small-data
    /// pointers (r2, r13), which apploaders use for their globals (Luigi's Mansion's does).
    /// With them 0, an apploader's first global is an access to unmapped memory.
    fn ipl_registers(&mut self, pal: bool) {
        let (sp, sda2, sda) = if pal { (0x815E_DCA8, 0x814B_5B20, 0x814B_4FC0) } else { (0x8156_6550, 0x8146_5CC0, 0x8146_5320) };
        self.cpu.gpr[1] = sp;
        self.cpu.gpr[2] = sda2;
        self.cpu.gpr[13] = sda;
    }

    /// Starts a GameCube disc: its apploader runs on our CPU and loads the game.
    pub fn boot_disc(&mut self, mut disc: Box<dyn Disc>) -> Result<Header, String> {
        let header = Header::read(disc.as_mut())?;
        if header.console == Console::Wii {
            return Err("Wii discs aren't supported yet".into());
        }
        // A Japanese disc plays on a Japanese console: the DTV status says so (bit 1), which is
        // what makes the library pick Shift-JIS for fonts and memory cards, so the blank card is
        // formatted for it.
        if header.region == 0 {
            self.hw.vi.regs[crate::hw::VI_DTV / 2] |= 2;
            self.hw.insert_blank_card(self.cpu.tb);
        }
        let pal = header.region == 2;
        self.title = format!("{} {}", header.game_id, header.title);
        let mut boot = vec![0u8; 0x440];
        disc.read(0, &mut boot).map_err(|e| e.to_string())?;
        let apploader = Apploader::read(disc.as_mut())?;
        self.hw.disc = Some(disc);
        self.ipl_cpu_state();
        self.ipl_registers(pal);
        self.disc_lowmem(&boot, pal);
        // The stub: "b ." to park on, and "blr" for the report function.
        self.w32(STUB, 0x4800_0000);
        self.w32(STUB_REPORT, 0x4E80_0020);
        self.hw.dma_write(APPLOADER_BASE & 0x1FFF_FFFF, &apploader.code);
        self.apploader_log.clear();
        // entry(&init, &main, &close)
        let ptrs = STUB + 0x100;
        self.call(apploader.entry, &[ptrs, ptrs + 4, ptrs + 8], "entry")?;
        let init = self.hw.read32((ptrs) & 0x1FFF_FFFF);
        let main = self.hw.read32((ptrs + 4) & 0x1FFF_FFFF);
        let close = self.hw.read32((ptrs + 8) & 0x1FFF_FFFF);
        self.call(init, &[STUB_REPORT], "init")?;
        let req = STUB + 0x200;
        for _ in 0..10_000 {
            let more = self.call(main, &[req, req + 4, req + 8], "main")?;
            let dst = self.hw.read32(req & 0x1FFF_FFFF);
            let len = self.hw.read32((req + 4) & 0x1FFF_FFFF);
            let off = self.hw.read32((req + 8) & 0x1FFF_FFFF);
            if len > 0 && len < (32 << 20) {
                let mut buf = vec![0u8; len as usize];
                self.hw.disc.as_mut().unwrap().read(off as u64, &mut buf).map_err(|e| e.to_string())?;
                self.hw.dma_write(dst & 0x1FFF_FFFF, &buf);
                self.hw.invalidate_code(dst & 0x1FFF_FFFF, len);
            }
            if more == 0 {
                break;
            }
        }
        let entry = self.call(close, &[], "close")?;
        self.hw.stall = 0;
        // The IPL hands over with translation as it left it; the game's own start code sets its
        // stack and small-data pointers.
        self.ipl_cpu_state();
        self.ipl_registers(pal);
        self.cpu.pc = entry;
        Ok(header)
    }

    /// Calls an apploader function on our CPU and runs it until it returns to the stub. Returns
    /// r3. Lines it prints through the report function are kept (`apploader_log`). Fails at once,
    /// saying where, if the code faults: nothing an apploader does should (interrupts are off).
    fn call(&mut self, func: u32, args: &[u32], what: &str) -> Result<u32, String> {
        for (k, a) in args.iter().enumerate() {
            self.cpu.gpr[3 + k] = *a;
        }
        self.cpu.lr = STUB;
        self.cpu.pc = func;
        let faults = self.cpu.fault_log.len();
        for _ in 0..200_000_000u64 {
            if self.cpu.pc == STUB {
                return Ok(self.cpu.gpr[3]);
            }
            if self.cpu.pc == STUB_REPORT {
                let line = self.report_line();
                if self.apploader_log.len() < 64 && !line.is_empty() {
                    self.apploader_log.push(line);
                }
            }
            self.cpu.step(&mut self.hw);
            if self.cpu.fault_log.len() > faults {
                let (vector, at, addr) = self.cpu.fault_log[faults];
                return Err(format!("apploader {what} faulted: vector {vector:X} at {at:08X}, address {addr:08X}"));
            }
        }
        Err(format!("apploader {what} never finished (at {:08X})", self.cpu.pc))
    }

    /// What the machine is doing, in short lines for the status screen a game that shows
    /// nothing gets (notice.rs): where the CPU is, what it has asked of the hardware.
    /// `hot` is where the CPU was at the end of recent fields, most often first.
    pub fn status_lines(&mut self, seconds: u64, hot: &[(u32, u32)]) -> Vec<String> {
        let mut l = vec![
            "CONTINUUM GC: NO PICTURE FROM THE GAME YET".to_string(),
            format!("AFTER {seconds} SECONDS. PLEASE SEND KIRO A"),
            "SCREENSHOT OF THIS SCREEN.".to_string(),
            String::new(),
        ];
        l.extend(crate::notice::wrap(&self.title, 50));
        let c = &self.cpu;
        l.push(format!("PC {:08X} LR {:08X} MSR {:08X}", c.pc, c.lr, c.msr));
        l.push(format!("SRR0 {:08X} DEC {:08X} TB {:X}", c.srr0, c.dec, c.tb));
        let ex: Vec<String> = c.exceptions.iter().enumerate().filter(|(_, n)| **n > 0).map(|(v, n)| format!("{v:X}00:{n}")).collect();
        l.extend(crate::notice::wrap(&format!("EXC {}", ex.join(" ")), 50));
        for (v, at, addr) in c.fault_log.iter().take(2) {
            l.push(format!("FAULT {v:X} AT {at:08X} ADDR {addr:08X}"));
        }
        let hot: Vec<String> = hot.iter().take(4).map(|(pc, n)| format!("{pc:08X}:{n}")).collect();
        l.push(format!("HOT {}", hot.join(" ")));
        let h = &self.hw;
        let di: Vec<String> = h.di_commands.iter().map(|(k, n)| format!("{k:02X}:{n}")).collect();
        l.extend(crate::notice::wrap(&format!("DISC {}", di.join(" ")), 50));
        l.push(format!("DSP {:?} {:08X} CSR {:04X}", h.dsp.hle.kind, h.dsp.hle.crc, h.dsp.csr));
        l.push(format!("AI {:08X} DI {:08X} SI {:08X}", h.ai.cr, h.di.sr, h.si.comcsr));
        l.push(format!("EXI {:08X} {:08X} {:08X}", h.exi.ch[0].csr, h.exi.ch[1].csr, h.exi.ch[2].csr));
        l.push(format!("VI {:04X} PI {:08X} CAUSE {:08X}", h.vi.regs[1], h.pi.mask, h.cause()));
        let g = &h.gx.stats;
        l.push(format!("GX PRIM {} EFB {} XFB {}", g.primitives, g.efb_copies, g.xfb_copies));
        let unknown: Vec<String> = h.unknown.iter().take(6).map(|(a, n)| format!("{a:08X}:{n}")).collect();
        if !unknown.is_empty() {
            l.extend(crate::notice::wrap(&format!("UNKNOWN {}", unknown.join(" ")), 50));
        }
        if let Some(last) = self.apploader_log.last() {
            l.extend(crate::notice::wrap(&format!("LOADER: {last}"), 50));
        }
        l
    }

    /// The line an apploader's report call prints: its printf-style format (r3) filled from the
    /// integer arguments (r4-r10): %d %i %u %x %X %c %s %p and %%, widths and zero padding.
    fn report_line(&mut self) -> String {
        let fmt: Vec<char> = self.c_string(self.cpu.gpr[3]).chars().collect();
        let mut out = String::new();
        let mut arg = 4;
        let mut i = 0;
        while i < fmt.len() {
            let c = fmt[i];
            i += 1;
            if c != '%' {
                out.push(c);
                continue;
            }
            let zero = fmt.get(i) == Some(&'0');
            let mut width = 0usize;
            while let Some(d) = fmt.get(i).and_then(|c| c.to_digit(10)) {
                width = width * 10 + d as usize;
                i += 1;
            }
            while matches!(fmt.get(i), Some('l' | 'h')) {
                i += 1;
            }
            let Some(&conv) = fmt.get(i) else { break };
            i += 1;
            if conv == '%' {
                out.push('%');
                continue;
            }
            let v = if arg <= 10 { self.cpu.gpr[arg] } else { 0 };
            arg += 1;
            let text = match conv {
                'd' | 'i' => (v as i32).to_string(),
                'u' => v.to_string(),
                'x' => format!("{v:x}"),
                'X' => format!("{v:X}"),
                'p' => format!("{v:08x}"),
                'c' => ((v & 0xFF) as u8 as char).to_string(),
                's' => self.c_string(v),
                _ => String::new(),
            };
            let pad = if zero { '0' } else { ' ' };
            for _ in text.chars().count()..width {
                out.push(pad);
            }
            out.push_str(&text);
        }
        out.trim().to_string()
    }

    /// A NUL-terminated string in memory (at most 200 bytes), for the apploader's report lines.
    fn c_string(&mut self, ea: u32) -> String {
        let mut bytes = Vec::new();
        for k in 0..200 {
            let b = self.hw.read8((ea + k) & 0x01FF_FFFF);
            if b == 0 {
                break;
            }
            bytes.push(b);
        }
        String::from_utf8_lossy(&bytes).trim().to_string()
    }

    /// Sets the console's clock to `seconds` since 1 January 2000 (the phone's own local time:
    /// the console's clock has no time zone, it reads what it was set to) and starts the time
    /// base from it, as the boot ROM does (clock plus SRAM's counter bias, in time-base ticks),
    /// which is the date and time games see. The blank memory card is formatted at that moment.
    /// Called when a game is loaded, before it runs; the test tools leave the clock at 0.
    pub fn set_clock(&mut self, seconds: u32) {
        self.hw.exi.rtc = seconds;
        self.hw.exi.rtc_frac = 0;
        let s = &self.hw.exi.sram;
        let at = crate::hw::SRAM_COUNTER_BIAS;
        let bias = u32::from_be_bytes([s[at], s[at + 1], s[at + 2], s[at + 3]]);
        let ticks_per_second = self.cpu_hz / cpu::CYCLES_PER_TIMEBASE_TICK;
        self.cpu.tb = seconds.wrapping_add(bias) as u64 * ticks_per_second;
        self.hw.insert_blank_card(self.cpu.tb);
    }

    /// Turns our JIT on (gamecube.json G8) when this process can run generated code: on the
    /// phone when the app gives the core code memory. Returns whether it is on.
    pub fn enable_jit(&mut self) -> bool {
        let Some(fast) = self.fast.as_mut() else {
            return false;
        };
        match crate::jit::backend(JIT_CODE_BYTES) {
            Some(b) => {
                fast.enable_jit(b);
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------------------ running

    /// Runs until the video interface finishes a field (one picture), or `max_cycles` pass.
    /// One whole video field: what the frontend calls once per frame.
    ///
    /// Runs until the video interface reaches the next field, with two fields' worth of cycles
    /// only as a safety cap. The cap used to be cpu_hz / 60, which is a hair SHORT of one NTSC
    /// field (262.5 lines at 59.94 a second): every call stopped one line before the field's end,
    /// so every third call ran just that one line. The machine ran at two thirds of real time,
    /// and the sound came out a third short (owner's iPhone, builds 227 and 228: 32031 sound
    /// frames a second for 48000 Hz; the app filled the rest with silence).
    pub fn run_one_field(&mut self) {
        self.run_field(self.cpu_hz / 30);
    }

    pub fn run_field(&mut self, max_cycles: u64) {
        self.hw.vi.field_done = false;
        let per_line = self.cycles_per_line();
        let mut ran = 0;
        while !self.hw.vi.field_done && ran < max_cycles {
            if self.hw.irq_dirty {
                self.cpu.external_line = self.hw.irq();
                self.hw.irq_dirty = false;
            }
            let wake = self.cpu.msr & cpu::MSR_EE != 0 && (self.cpu.external_line || self.cpu.dec_pending);
            let c = if self.cpu.msr & cpu::MSR_POW != 0 && !wake {
                // Power-save: the CPU sleeps until an interrupt, so time jumps to the next
                // event (the end of this video line, or the decrementer).
                let skip = (per_line - self.line_cycles.min(per_line - 1)).min(self.cpu.cycles_to_decrementer()).max(1);
                self.cpu.advance_time(skip);
                skip
            } else if let Some(fast) = self.fast.as_mut() {
                if !self.hw.invalidated.is_empty() {
                    for (pa, len) in self.hw.invalidated.drain(..) {
                        fast.invalidate(pa, len);
                    }
                }
                let budget = (per_line - self.line_cycles.min(per_line - 1)).min(max_cycles - ran).min(self.cpu.cycles_to_decrementer());
                fast.run(&mut self.cpu, &mut self.hw, budget).max(1)
            } else {
                self.cpu.step(&mut self.hw)
            };
            // Time the program spent waiting on the hardware (immediate EXI transfers), counted
            // now: the write that started it ended the slice.
            let stall = std::mem::take(&mut self.hw.stall);
            if stall > 0 {
                self.cpu.advance_time(stall);
            }
            let c = c + stall;
            ran += c;
            self.line_cycles += c;
            if self.line_cycles >= per_line {
                // Every instruction so far is counted in the CPU's clock now.
                self.hw.now = self.cpu.cycles;
            }
            while self.line_cycles >= per_line {
                self.line_cycles -= per_line;
                if let Some(hot) = self.hot.as_mut() {
                    if hot.len() < 65_536 {
                        *hot.entry(self.cpu.pc & !0xFF).or_default() += 1;
                    }
                }
                self.hw.vi_line();
                self.hw.ai_line();
                self.hw.audio_line();
            }
        }
    }

    // ------------------------------------------------------------------ save states

    pub fn save_state(&self) -> Vec<u8> {
        // The EFB must hold everything drawn so far.
        self.hw.gx.wait_idle();
        let s = State {
            cpu: self.cpu.clone(),
            mem1: self.hw.mem1.clone(),
            mem2: self.hw.mem2.clone(),
            aram: self.hw.aram.clone(),
            pi: self.hw.pi.clone(),
            vi: self.hw.vi.clone(),
            si: self.hw.si.clone(),
            exi: self.hw.exi.clone(),
            di: self.hw.di.clone(),
            dsp: self.hw.dsp.clone(),
            ai: self.hw.ai.clone(),
            gx: self.hw.gx.clone(),
            gather: self.hw.gather.clone(),
            line_cycles: self.line_cycles,
        };
        let mut out = STATE_MAGIC.to_vec();
        out.extend(bincode::serialize(&s).expect("state serialises"));
        out
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        if !data.starts_with(STATE_MAGIC) {
            if data.starts_with(b"CGC") {
                return Err("a save state from an older version of this emulator".into());
            }
            return Err("not a save state from this emulator".into());
        }
        let s: State = bincode::deserialize(&data[4..]).map_err(|e| e.to_string())?;
        if s.mem1.len() != self.hw.mem1.len() || s.mem2.len() != self.hw.mem2.len() {
            return Err("a save state from the other console".into());
        }
        self.cpu = s.cpu;
        self.cpu.tlb.flush_all();
        if let Some(f) = self.fast.as_mut() {
            f.clear();
        }
        self.hw.mem1 = s.mem1;
        self.hw.mem2 = s.mem2;
        self.hw.aram = s.aram;
        self.hw.pi = s.pi;
        self.hw.vi = s.vi;
        self.hw.si = s.si;
        self.hw.exi = s.exi;
        self.hw.di = s.di;
        self.hw.dsp = s.dsp;
        self.hw.ai = s.ai;
        let threads = self.hw.gx.draw_threads();
        self.hw.gx = s.gx;
        self.hw.gx.set_draw_threads(threads);
        self.hw.gather = s.gather;
        self.line_cycles = s.line_cycles;
        Ok(())
    }
}
