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

pub struct Machine {
    pub cpu: Cpu,
    pub hw: Hw,
    /// The fast interpreter's block cache; None runs the plain interpreter.
    pub fast: Option<cpu::fast::FastCpu>,
    /// CPU cycles since the last video line.
    line_cycles: u64,
    pub cpu_hz: u64,
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

pub const STATE_MAGIC: &[u8; 4] = b"CGC2";

impl Machine {
    pub fn new(console: Console) -> Self {
        let wii = console == Console::Wii;
        Machine {
            cpu: Cpu::new(if wii { cpu::PVR_BROADWAY } else { cpu::PVR_GEKKO }),
            hw: Hw::new(wii),
            fast: Some(cpu::fast::FastCpu::new()),
            line_cycles: 0,
            cpu_hz: if wii { WII_CPU_HZ } else { GC_CPU_HZ },
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

    /// Starts a GameCube disc: its apploader runs on our CPU and loads the game.
    pub fn boot_disc(&mut self, mut disc: Box<dyn Disc>) -> Result<Header, String> {
        let header = Header::read(disc.as_mut())?;
        if header.console == Console::Wii {
            return Err("Wii discs aren't supported yet".into());
        }
        let mut boot = vec![0u8; 0x440];
        disc.read(0, &mut boot).map_err(|e| e.to_string())?;
        let mut bi2 = vec![0u8; 0x2000];
        disc.read(0x440, &mut bi2).map_err(|e| e.to_string())?;
        let apploader = Apploader::read(disc.as_mut())?;
        self.hw.disc = Some(disc);
        self.ipl_cpu_state();
        self.lowmem(Some(&boot));
        self.hw.dma_write(0x017F_E000, &bi2);
        self.w32(0x8000_00F4, 0x817F_E000);
        // The stub: "b ." to park on, and "blr" for the report function.
        self.w32(STUB, 0x4800_0000);
        self.w32(STUB_REPORT, 0x4E80_0020);
        self.hw.dma_write(APPLOADER_BASE & 0x1FFF_FFFF, &apploader.code);
        // entry(&init, &main, &close)
        let ptrs = STUB + 0x100;
        self.call(apploader.entry, &[ptrs, ptrs + 4, ptrs + 8])?;
        let init = self.hw.read32((ptrs) & 0x1FFF_FFFF);
        let main = self.hw.read32((ptrs + 4) & 0x1FFF_FFFF);
        let close = self.hw.read32((ptrs + 8) & 0x1FFF_FFFF);
        self.call(init, &[STUB_REPORT])?;
        let req = STUB + 0x200;
        for _ in 0..10_000 {
            let more = self.call(main, &[req, req + 4, req + 8])?;
            let dst = self.hw.read32(req & 0x1FFF_FFFF);
            let len = self.hw.read32((req + 4) & 0x1FFF_FFFF);
            let off = self.hw.read32((req + 8) & 0x1FFF_FFFF);
            if len > 0 {
                let mut buf = vec![0u8; len as usize];
                self.hw.disc.as_mut().unwrap().read(off as u64, &mut buf).map_err(|e| e.to_string())?;
                self.hw.dma_write(dst & 0x1FFF_FFFF, &buf);
                self.hw.invalidate_code(dst & 0x1FFF_FFFF, len);
            }
            if more == 0 {
                break;
            }
        }
        let entry = self.call(close, &[])?;
        // The IPL hands over with a fresh stack and translation as it left it.
        self.ipl_cpu_state();
        self.cpu.pc = entry;
        Ok(header)
    }

    /// Calls a function on our CPU and runs it until it returns to the stub. Returns r3.
    fn call(&mut self, func: u32, args: &[u32]) -> Result<u32, String> {
        for (k, a) in args.iter().enumerate() {
            self.cpu.gpr[3 + k] = *a;
        }
        self.cpu.lr = STUB;
        self.cpu.pc = func;
        for _ in 0..200_000_000u64 {
            if self.cpu.pc == STUB {
                return Ok(self.cpu.gpr[3]);
            }
            self.cpu.step(&mut self.hw);
        }
        Err(format!("the disc's loader never finished (stuck at {:08X})", self.cpu.pc))
    }

    // ------------------------------------------------------------------ running

    /// Runs until the video interface finishes a field (one picture), or `max_cycles` pass.
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
                let budget = (per_line - self.line_cycles.min(per_line - 1)).min(max_cycles - ran);
                let (c, built) = fast.run(&mut self.cpu, &mut self.hw, budget, &|h: &mut Hw| h.irq_dirty || !h.invalidated.is_empty());
                if let Some(page) = built {
                    self.hw.mark_code_page(page);
                }
                c
            } else {
                self.cpu.step(&mut self.hw)
            };
            ran += c;
            self.line_cycles += c;
            if self.line_cycles >= per_line {
                self.line_cycles -= per_line;
                self.hw.vi_line();
                // 48 kHz streaming audio: about 3 samples a line.
                self.hw.ai_samples(3);
                self.hw.audio_line();
            }
        }
    }

    // ------------------------------------------------------------------ save states

    pub fn save_state(&self) -> Vec<u8> {
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
        self.hw.gx = s.gx;
        self.hw.gather = s.gather;
        self.line_cycles = s.line_cycles;
        Ok(())
    }
}
