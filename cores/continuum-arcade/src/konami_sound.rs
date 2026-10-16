//! Konami's sound board for the Scramble family (Scramble, The End, Super Cobra, Frogger, Amidar
//! and their clones): a Z80 of its own at 1.79 MHz (the 14.318 MHz crystal / 8) with 8 KB of
//! program and 1 KB of RAM, two AY-3-8910s (Frogger's board has one), and a filter on each of the
//! six channels.
//!
//! - **The command**: the main board writes a byte to a latch (its second PPI's port A), which
//!   the first AY reads through its port A, and interrupts the sound Z80 by taking bit 3 of its
//!   port B low. The interrupt is held until the Z80 takes it. Bit 4 of that port silences the
//!   sound.
//! - **The timer**: the first AY's port B reads a chain of counters on the 14.318 MHz clock
//!   (÷256, ÷2, ÷8, ÷5, ÷2: 40,960 clocks round), which the programs use for their tempo.
//!   Frogger's board has two of its bits the other way round.
//! - **The AYs** sit in the Z80's I/O space, selected by address bits 4-7 (the first AY's
//!   address and data on A6 and A7, the second's on A4 and A5); a read with A7 or A5 reads them.
//! - **The filters**: each channel's output (the AY's output transistor, modelled as a resistor
//!   per volume level) goes through 1 kΩ to a node that a CD4066 switch can tie to 0.22 µF and
//!   another to 0.047 µF, then through 5 kΩ into an op-amp that sums all six; a write anywhere in
//!   9000h-9FFFh sets the twelve switches from the address's low twelve bits. The op-amp's
//!   output is divided down (4.7 kΩ over 200 Ω) and coupled through 0.15 µF to the amplifier.
//!
//! The digital side (the Z80, the latch, the interrupt, the timer and the AYs) is exact; the
//! filters and amplifier are an APPROXIMATION of the circuit (MAME models them with its netlist
//! simulator, src/mame/galaxian/nl_konami.cpp, whose component values these are), so the sound
//! comes out close to, not equal to, MAME's (.kiro/notes/docs/ARCADE_CORE.md).
//!
//! Timing: the main board hands over what it did, in time order, each with the moment it
//! happened; the sound Z80 then runs up to the end of the main board's frame. A command or an
//! interrupt the main board sent is seen from the first instruction the sound Z80 starts at or
//! after that moment, as in MAME, where the write stops the main CPU's timeslice there and the
//! sound CPU runs up to it before the latch changes.

use serde::{Deserialize, Serialize};

use crate::ay8910::{self, Ay8910};
use crate::z80::{Bus, Z80};

/// The sound Z80's clock in whole hertz, as MAME has it (14,318,181 Hz / 8).
pub const CPU_HZ: u32 = 1_789_772;
/// One sound CPU cycle, in attoseconds.
pub const AS_PER_CYCLE: i128 = 1_000_000_000_000_000_000 / CPU_HZ as i128;
/// The sound handed out.
pub const SAMPLE_RATE: u32 = 48_000;
const AS_PER_SECOND: i128 = 1_000_000_000_000_000_000;

/// Called before each of the sound CPU's instructions with the CPU and its cycles since reset.
pub type Trace<'a> = &'a mut dyn FnMut(&Z80, u64);

/// Which sound board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Two AY-3-8910s (Scramble, The End, Super Cobra, Amidar, Turtles).
    TwoAy,
    /// Frogger's: one AY-3-8910, RAM at 4000h, the filters at 6000h, the AY's address and data
    /// on A7 and A6, the first ROM's D0 and D1 swapped and two of the timer's bits the other way.
    Frogger,
}

/// Something the main board did, for the sound board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// A byte written to the command latch.
    Latch(u8),
    /// The second PPI's port B: bit 3 falling interrupts the sound Z80, bit 4 silences the sound.
    Control(u8),
    /// The watchdog reset the machine: the sound Z80 and the AYs reset too.
    Reset,
}

// The filter network (per channel): the AY's output resistor from 5 V, 1 kΩ to the node, 5 kΩ
// from the node into the op-amp's virtual ground, and two capacitors each behind a 270 Ω switch.
const R_IN: f64 = 1000.0;
const R_OUT: f64 = 5000.0;
const R_SWITCH: f64 = 270.0;
const C_LOW: f64 = 0.22e-6;
const C_HIGH: f64 = 0.047e-6;
/// The op-amp's feedback resistor, and how far its output can swing on its ±5 V supply.
const R_FEEDBACK: f64 = 2200.0;
const SWING: f64 = 3.5;
/// The divider into the amplifier (4.7 kΩ over 200 Ω), the 0.15 µF coupling into its 100 kΩ
/// input, and MAME's scaling of that input to full scale (1 / 0.05).
const DIVIDER: f64 = 200.0 / 4900.0;
const COUPLING_TAU: f64 = 0.15e-6 * 100.2e3;
const SCALE: f64 = 20.0;

/// The AY's output transistor, modelled as MAME's AY model does it: a MOSFET source follower
/// (gate at 4.9 V, threshold 1.465 V) whose transconductance each volume level sets, into a 1 kΩ
/// load; the resistor that would give the same voltage from 5 V stands in for it. The
/// transconductances (µA/V²) are the ones MAME fitted to a real chip's output.
const KN: [f64; 16] = [
    0.00076, 0.80536, 1.13106, 1.65952, 2.42261, 3.60536, 5.34893, 8.96871, 10.97202, 19.32370,
    29.01935, 38.82026, 55.50539, 78.44395, 109.49257, 153.72985,
];

fn level_resistance(level: usize) -> f64 {
    let (vg, rd) = (4.9 - 1.465_385_778, 1000.0);
    let kn = KN[level] / 1e6;
    let p2 = 1.0 / (2.0 * kn * rd) + vg;
    // Vs = p2 - sqrt(p2² - vg²), written so that it stays exact when p2 is huge (level 0).
    let vs = vg * vg / (p2 + (p2 * p2 - vg * vg).sqrt());
    rd * (5.0 / vs - 1.0)
}

/// One channel's two filter capacitors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Filter {
    low: f64,
    high: f64,
}

impl Filter {
    /// One AY tick (`dt` seconds) with the channel's resistance and switches; returns the current
    /// into the op-amp. Stepped implicitly (backward Euler), which stays stable however small the
    /// switches' resistance makes the capacitors' time constants.
    fn step(&mut self, r_ay: f64, low_on: bool, high_on: bool, dt: f64) -> f64 {
        let gs = 1.0 / (r_ay + R_IN);
        let g = 1.0 / R_SWITCH;
        let n = f64::from(u8::from(low_on) + u8::from(high_on));
        let d = gs + 1.0 / R_OUT + n * g;
        let s = 5.0 * gs;
        let (a1, a2) = (dt * g / C_LOW, dt * g / C_HIGH);
        match (low_on, high_on) {
            (false, false) => {}
            (true, false) => self.low = (self.low + a1 * s / d) / (1.0 + a1 - a1 * g / d),
            (false, true) => self.high = (self.high + a2 * s / d) / (1.0 + a2 - a2 * g / d),
            (true, true) => {
                let (m11, m12) = (1.0 + a1 - a1 * g / d, -a1 * g / d);
                let (m21, m22) = (-a2 * g / d, 1.0 + a2 - a2 * g / d);
                let (r1, r2) = (self.low + a1 * s / d, self.high + a2 * s / d);
                let det = m11 * m22 - m12 * m21;
                self.low = (r1 * m22 - m12 * r2) / det;
                self.high = (m11 * r2 - m21 * r1) / det;
            }
        }
        let mut i = s;
        if low_on {
            i += g * self.low;
        }
        if high_on {
            i += g * self.high;
        }
        (i / d) / R_OUT
    }
}

/// The analogue side: the AYs ticking, the filters, the op-amp and the output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Analog {
    ay: [Ay8910; 2],
    /// The filter switches as the last write to the filter latch set them (address bits 0-11).
    switches: u16,
    filters: [Filter; 6],
    /// AY ticks done since power-on (one every 8 sound CPU cycles).
    ticks: u64,
    /// The 48 kHz sample being built: the op-amp's output summed over the ticks in it.
    sum: f64,
    count: u32,
    /// Samples finished since power-on.
    samples: u64,
    coupling: f64,
    muted: bool,
    #[serde(skip)]
    out: Vec<i16>,
}

impl Analog {
    fn new() -> Analog {
        Analog {
            ay: [Ay8910::new(), Ay8910::new()],
            switches: 0,
            filters: [Filter::default(); 6],
            ticks: 0,
            sum: 0.0,
            count: 0,
            samples: 0,
            coupling: 0.0,
            muted: false,
            out: Vec::new(),
        }
    }

    /// Ticks the AYs and the filters up to sound CPU cycle `cycle` (ticks that start before it).
    fn run_to(&mut self, kind: Kind, cycle: u64, table: &[f64; 16]) {
        let dt = 8.0 / f64::from(CPU_HZ);
        let chips = if kind == Kind::Frogger { 1 } else { 2 };
        while self.ticks * 8 < cycle {
            let mut current = 0.0;
            for chip in 0..chips {
                let levels = self.ay[chip].tick();
                for (ch, &level) in levels.iter().enumerate() {
                    // The first AY's switches are address bits 6-11, the second's 0-5: two a
                    // channel, the 0.22 µF one first.
                    let shift = if chip == 0 { 6 } else { 0 } + 2 * ch;
                    let (low, high) = (self.switches >> shift & 1 != 0, self.switches >> (shift + 1) & 1 != 0);
                    current += self.filters[chip * 3 + ch].step(table[usize::from(level)], low, high, dt);
                }
            }
            // The op-amp sums the currents into its virtual ground through 2.2 kΩ.
            let v = (-R_FEEDBACK * current).clamp(-SWING, SWING);
            // Which 48 kHz sample this tick falls in.
            let at = i128::from(self.ticks * 8) * AS_PER_CYCLE;
            let sample = (at * i128::from(SAMPLE_RATE) / AS_PER_SECOND) as u64;
            while sample > self.samples {
                self.finish_sample();
            }
            self.sum += v;
            self.count += 1;
            self.ticks += 1;
        }
    }

    fn finish_sample(&mut self) {
        let v = if self.count > 0 { self.sum / f64::from(self.count) } else { 0.0 };
        let vr = v * DIVIDER;
        self.coupling += (vr - self.coupling) * (1.0 - (-1.0 / (f64::from(SAMPLE_RATE) * COUPLING_TAU)).exp());
        let out = (vr - self.coupling) * SCALE;
        let s = if self.muted { 0 } else { (out * 32767.0).round().clamp(-32768.0, 32767.0) as i16 };
        self.out.push(s);
        self.sum = 0.0;
        self.count = 0;
        self.samples += 1;
    }
}

/// The sound board's state: what a save state holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KonamiSound {
    kind: Kind,
    cpu: Z80,
    ram: Vec<u8>,
    latch: u8,
    control: u8,
    /// Sound CPU cycles since power-on (its clock), and since its last reset (what the timer
    /// counts, as MAME's total_cycles does).
    local: u64,
    since_reset: u64,
    analog: Analog,
    #[serde(skip)]
    rom: Vec<u8>,
    #[serde(skip, default = "resistances")]
    table: [f64; 16],
    /// The last instruction run: its first cycle, its length, its bus cycles in order (M an
    /// opcode fetch, R and W memory, I I/O) and the CPU before it. A reset that lands inside it
    /// cuts it where MAME's Z80 had stopped (`galaxian::pause_point`).
    last: Option<(u64, u32, Vec<u8>, Z80)>,
}

fn resistances() -> [f64; 16] {
    let mut t = [0.0; 16];
    for (level, r) in t.iter_mut().enumerate() {
        *r = level_resistance(level);
    }
    t
}

/// The sound Z80's view of the board while one instruction runs.
struct SoundBus<'a> {
    kind: Kind,
    rom: &'a [u8],
    ram: &'a mut [u8],
    latch: u8,
    analog: &'a mut Analog,
    table: &'a [f64; 16],
    /// The instruction's first cycle, and the cycles its bus accesses have taken so far.
    start: u64,
    since_reset: u64,
    elapsed: u32,
    irq_acked: bool,
    kinds: Vec<u8>,
}

/// The AY's ports on this board: A reads the latch, B the timer.
struct AyPorts {
    latch: u8,
    timer: u8,
}

impl ay8910::Ports for AyPorts {
    fn read(&mut self, port: ay8910::Port) -> Option<u8> {
        Some(match port {
            ay8910::Port::A => self.latch,
            ay8910::Port::B => self.timer,
        })
    }
}

/// The counter chain behind the timer, after `cycles` sound CPU cycles (each 8 clocks of the
/// 14.318 MHz crystal): B7 the last ÷2, B6 and B5 the ÷5's top two bits, B4 the ÷8's top bit,
/// B3-B1 high and B0 grounded.
pub fn timer(kind: Kind, cycles: u64) -> u8 {
    let mut c = (cycles * 8) % 40_960;
    let hibit = u8::from(c >= 20_480);
    if hibit != 0 {
        c -= 20_480;
    }
    let bit = |n: u32| (c >> n & 1) as u8;
    let v = hibit << 7 | bit(14) << 6 | bit(13) << 5 | bit(11) << 4 | 0x0E;
    match kind {
        Kind::TwoAy => v,
        // Frogger's board has bits 3 and 5 the other way round.
        Kind::Frogger => (v & !0x28) | (v >> 2 & 0x08) | (v << 2 & 0x20),
    }
}

impl SoundBus<'_> {
    fn now(&self, len: u32) -> u64 {
        self.start + u64::from(self.elapsed + len)
    }

    fn ay_ports(&self, len: u32) -> AyPorts {
        let cycles = self.since_reset + (self.now(len) - self.start);
        AyPorts { latch: self.latch, timer: timer(self.kind, cycles) }
    }
}

impl Bus for SoundBus<'_> {
    fn read(&mut self, addr: u16) -> u8 {
        self.elapsed += 3;
        self.kinds.push(b'R');
        self.mem(addr)
    }

    fn fetch(&mut self, addr: u16) -> u8 {
        self.elapsed += 4;
        self.kinds.push(b'M');
        self.mem(addr)
    }

    fn write(&mut self, addr: u16, val: u8) {
        // An access happens at the start of its bus cycle, as MAME's Z80 has it (the access,
        // then the cycle's T-states).
        let at = self.start + u64::from(self.elapsed);
        self.elapsed += 3;
        self.kinds.push(b'W');
        let a = usize::from(addr);
        match self.kind {
            Kind::TwoAy => match a {
                // 8000h-83FFh, mirrored wherever A15 is set and A12 clear.
                _ if a & 0x9000 == 0x8000 => self.ram[a & 0x3FF] = val,
                // The filter latch: the address is the data.
                _ if a & 0x9000 == 0x9000 => {
                    self.analog.run_to(self.kind, at, self.table);
                    self.analog.switches = (a & 0xFFF) as u16;
                }
                _ => {}
            },
            Kind::Frogger => match a & 0x7FFF {
                0x4000..=0x5FFF => self.ram[a & 0x3FF] = val,
                0x6000..=0x7FFF => {
                    self.analog.run_to(self.kind, at, self.table);
                    self.analog.switches = (a & 0xFFF) as u16;
                }
                _ => {}
            },
        }
    }

    fn input(&mut self, port: u16) -> u8 {
        let p = port as u8;
        let mut ports = self.ay_ports(0);
        self.elapsed += 4;
        self.kinds.push(b'I');
        let mut v = 0xFF;
        match self.kind {
            Kind::TwoAy => {
                if p & 0x20 != 0 {
                    v &= self.analog.ay[1].read_data(&mut ports);
                }
                if p & 0x80 != 0 {
                    v &= self.analog.ay[0].read_data(&mut ports);
                }
            }
            Kind::Frogger => {
                if p & 0x40 != 0 {
                    v &= self.analog.ay[0].read_data(&mut ports);
                }
            }
        }
        v
    }

    fn output(&mut self, port: u16, val: u8) {
        let p = port as u8;
        // The chips see the write at the start of the I/O cycle.
        self.analog.run_to(self.kind, self.start + u64::from(self.elapsed), self.table);
        let mut ports = self.ay_ports(0);
        self.elapsed += 4;
        self.kinds.push(b'I');
        let ay = &mut self.analog.ay;
        match self.kind {
            Kind::TwoAy => {
                if p & 0x10 != 0 {
                    ay[1].write_address(val);
                } else if p & 0x20 != 0 {
                    ay[1].write_data(val, &mut ports);
                }
                if p & 0x40 != 0 {
                    ay[0].write_address(val);
                } else if p & 0x80 != 0 {
                    ay[0].write_data(val, &mut ports);
                }
            }
            Kind::Frogger => {
                if p & 0x40 != 0 {
                    ay[0].write_data(val, &mut ports);
                } else if p & 0x80 != 0 {
                    ay[0].write_address(val);
                }
            }
        }
    }

    fn int_ack(&mut self) -> u8 {
        // The interrupt flip-flop is cleared by the acknowledge.
        self.irq_acked = true;
        0xFF
    }
}

impl SoundBus<'_> {
    fn mem(&self, addr: u16) -> u8 {
        let a = usize::from(addr);
        match self.kind {
            Kind::TwoAy => match a {
                0x0000..=0x1FFF => self.rom.get(a).copied().unwrap_or(0),
                _ if a & 0x9000 == 0x8000 => self.ram[a & 0x3FF],
                _ => 0,
            },
            Kind::Frogger => match a & 0x7FFF {
                0x0000..=0x1FFF => self.rom.get(a & 0x1FFF).copied().unwrap_or(0),
                0x4000..=0x5FFF => self.ram[a & 0x3FF],
                _ => 0,
            },
        }
    }
}

impl KonamiSound {
    /// The board with its program (8 KB at 0000h, as the set's region holds it).
    pub fn new(kind: Kind, program: &[u8]) -> KonamiSound {
        let mut rom = program.to_vec();
        rom.resize(0x2000, 0);
        if kind == Kind::Frogger {
            // The first ROM's D0 and D1 are swapped on the board.
            for b in rom[..0x800].iter_mut() {
                *b = (*b & 0xFC) | (*b >> 1 & 1) | (*b << 1 & 2);
            }
        }
        KonamiSound {
            kind,
            cpu: Z80::new(),
            ram: vec![0; 0x400],
            latch: 0,
            control: 0,
            local: 0,
            since_reset: 0,
            analog: Analog::new(),
            rom,
            table: resistances(),
            last: None,
        }
    }

    /// After a save state is restored: the ROM isn't in the state.
    pub fn restore_rom(&mut self, from: &KonamiSound) {
        self.rom = from.rom.clone();
    }

    pub fn cpu(&self) -> &Z80 {
        &self.cpu
    }

    /// Sound CPU cycles since its last reset (what MAME's debugger calls totalcycles).
    pub fn cycles_since_reset(&self) -> u64 {
        self.since_reset
    }

    pub fn ay(&self, n: usize) -> &Ay8910 {
        &self.analog.ay[n]
    }

    pub fn filter_switches(&self) -> u16 {
        self.analog.switches
    }

    pub fn latch(&self) -> u8 {
        self.latch
    }

    /// One instruction (and an interrupt before it if one is waiting); calls `trace` first with
    /// the CPU as it stands, for comparing with a trace of MAME's.
    fn step(&mut self, trace: &mut Option<Trace>) {
        // An interrupt first, on its own, so that a trace shows the handler's first instruction
        // with the response's cycles counted (MAME's debugger sees it that way).
        let t = self.bus_op(|cpu, bus| cpu.take_interrupt(bus));
        self.local += u64::from(t);
        self.since_reset += u64::from(t);
        if let Some(tr) = trace.as_mut() {
            tr(&self.cpu, self.since_reset);
        }
        let before = self.cpu.clone();
        let start = self.local;
        let mut kinds = self.last.take().map(|l| l.2).unwrap_or_default();
        kinds.clear();
        let (t, kinds) = self.bus_op_kinds(kinds, |cpu, bus| cpu.step(bus));
        self.local += u64::from(t);
        self.since_reset += u64::from(t);
        self.last = Some((start, t, kinds, before));
    }

    fn bus_op_kinds(&mut self, kinds: Vec<u8>, op: impl FnOnce(&mut Z80, &mut SoundBus) -> u32) -> (u32, Vec<u8>) {
        let mut bus = SoundBus {
            kind: self.kind,
            rom: &self.rom,
            ram: &mut self.ram,
            latch: self.latch,
            analog: &mut self.analog,
            table: &self.table,
            start: self.local,
            since_reset: self.since_reset,
            elapsed: 0,
            irq_acked: false,
            kinds,
        };
        let t = op(&mut self.cpu, &mut bus);
        let acked = bus.irq_acked;
        let kinds = std::mem::take(&mut bus.kinds);
        if acked {
            self.cpu.set_irq(false);
        }
        (t, kinds)
    }

    /// Runs `op` on the CPU with the board as its bus, starting at the current cycle.
    fn bus_op(&mut self, op: impl FnOnce(&mut Z80, &mut SoundBus) -> u32) -> u32 {
        let mut bus = SoundBus {
            kind: self.kind,
            rom: &self.rom,
            ram: &mut self.ram,
            latch: self.latch,
            analog: &mut self.analog,
            table: &self.table,
            start: self.local,
            since_reset: self.since_reset,
            elapsed: 0,
            irq_acked: false,
            kinds: Vec::new(),
        };
        let t = op(&mut self.cpu, &mut bus);
        if bus.irq_acked {
            self.cpu.set_irq(false);
        }
        t
    }

    /// Whether the main board has silenced the sound (its control's bit 4).
    pub fn muted(&self) -> bool {
        self.analog.muted
    }

    fn apply(&mut self, e: Event, at: u64) {
        match e {
            Event::Latch(v) => self.latch = v,
            Event::Control(v) => {
                let old = self.control;
                self.control = v;
                if old & 0x08 != 0 && v & 0x08 == 0 {
                    self.cpu.set_irq(true);
                }
                self.analog.run_to(self.kind, self.local, &self.table);
                self.analog.muted = v & 0x10 != 0;
            }
            Event::Reset => {
                // MAME's Z80 stopped for the reset at the first point at or after its budget
                // where it checks its cycle count; the rest of an instruction it was in the
                // middle of never happens (its registers stay as they were), and its clock
                // carries on from there.
                if let Some((start, len, kinds, before)) = self.last.take() {
                    let end = start + u64::from(len);
                    if start < at && at < end && self.local == end {
                        let p = crate::galaxian::pause_point(start, len, &kinds, at);
                        if p < end {
                            self.cpu = before;
                            self.local = p;
                        }
                    }
                }
                self.analog.run_to(self.kind, self.local, &self.table);
                self.cpu.reset();
                self.cpu.set_irq(false);
                self.since_reset = 0;
                for ay in self.analog.ay.iter_mut() {
                    ay.reset();
                }
            }
        }
    }

    /// The first sound CPU cycle at which an instruction sees something that happened at `t`
    /// attoseconds.
    fn cycle_at(t: i128) -> u64 {
        (t.max(0) / AS_PER_CYCLE) as u64
    }

    /// Runs the board up to `until` attoseconds after power-on, taking in the main board's events
    /// (each with the moment it happened) on the way, and appends the finished 48 kHz samples to
    /// `out`.
    pub fn run(
        &mut self,
        until: i128,
        events: &[(i128, Event)],
        out: &mut Vec<i16>,
        mut trace: Option<Trace>,
    ) {
        for &(t, e) in events {
            let at = Self::cycle_at(t);
            while self.local < at {
                self.step(&mut trace);
            }
            self.apply(e, at);
        }
        let end = Self::cycle_at(until);
        while self.local < end {
            self.step(&mut trace);
        }
        self.analog.run_to(self.kind, self.local, &self.table);
        out.append(&mut self.analog.out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_transistor_as_resistors() {
        let r: Vec<f64> = (0..16).map(level_resistance).collect();
        // Full volume is about 4.3 kΩ, silence hundreds of megohms, and each step is less.
        assert!((4000.0..4600.0).contains(&r[15]), "{}", r[15]);
        assert!(r[0] > 1e8);
        assert!(r.windows(2).all(|w| w[1] < w[0]));
    }

    #[test]
    fn the_timer_counts_40960_clocks_round() {
        // 5120 sound CPU cycles a round; B7 is high for the second half.
        assert_eq!(timer(Kind::TwoAy, 0), 0x0E);
        assert_eq!(timer(Kind::TwoAy, 2560) & 0x80, 0x80);
        assert_eq!(timer(Kind::TwoAy, 5120), 0x0E);
        // B4 is the ÷8's top bit: 1024 clocks (128 cycles) on, then off.
        assert_eq!(timer(Kind::TwoAy, 256) & 0x10, 0x10);
        assert_eq!(timer(Kind::TwoAy, 255) & 0x10, 0x00);
        // Every value the chain goes through, and only those.
        let mut seen = std::collections::BTreeSet::new();
        for c in 0..5120 {
            seen.insert(timer(Kind::TwoAy, c));
        }
        assert_eq!(seen.len(), 12);
        // Frogger's has bits 3 and 5 swapped.
        for c in (0..5120).step_by(37) {
            let (k, f) = (timer(Kind::TwoAy, c), timer(Kind::Frogger, c));
            assert_eq!(f & !0x28, k & !0x28);
            assert_eq!(f >> 3 & 1, k >> 5 & 1);
            assert_eq!(f >> 5 & 1, k >> 3 & 1);
        }
    }

    /// A program: IM 1, EI, a loop; at 38h it reads the latch through the first AY's port A into
    /// RAM, plays it as channel A's period at full volume, and returns.
    fn program() -> Vec<u8> {
        let mut p = vec![0u8; 0x2000];
        let main = [
            0x31, 0x00, 0x84, // LD SP,8400h
            0xED, 0x56, // IM 1
            0xFB, // EI
            0x18, 0xFE, // JR $
        ];
        p[..main.len()].copy_from_slice(&main);
        let isr = [
            0x3E, 0x0E, 0xD3, 0x40, // LD A,14 : OUT (40h),A   ; address R14
            0xDB, 0x80, // IN A,(80h)               ; the latch
            0x32, 0x00, 0x80, // LD (8000h),A
            0x47, // LD B,A
            0x3E, 0x00, 0xD3, 0x40, 0x78, 0xD3, 0x80, // R0 = B
            0x3E, 0x08, 0xD3, 0x40, 0x3E, 0x0F, 0xD3, 0x80, // R8 = 15
            0x3E, 0x07, 0xD3, 0x40, 0x3E, 0x3E, 0xD3, 0x80, // R7: tone A only
            0xFB, 0xC9, // EI : RET
        ];
        p[0x38..0x38 + isr.len()].copy_from_slice(&isr);
        p
    }

    fn frame(s: &mut KonamiSound, n: i128, events: &[(i128, Event)]) -> Vec<i16> {
        let frame = 50_688 * (AS_PER_SECOND / 3_072_000);
        let mut out = Vec::new();
        s.run(frame * n, events, &mut out, None);
        out
    }

    #[test]
    fn a_command_interrupts_and_plays() {
        let mut s = KonamiSound::new(Kind::TwoAy, &program());
        let f = 50_688 * (AS_PER_SECOND / 3_072_000);
        let out = frame_events(&mut s);
        assert_eq!(s.ram[0], 0x42, "the interrupt read the latch");
        assert_eq!(s.analog.ay[0].tone_period(0), 0x42);
        // A tone at 1.79 MHz / 16 / 0x42 = 1695 Hz, after the command: not silent.
        let tail = &out[out.len() - 800..];
        let swing = tail.iter().max().unwrap() - tail.iter().min().unwrap();
        assert!(swing > 1000, "swing {swing}");
        // Muting silences it.
        let quiet = frame(&mut s, 4, &[(f * 3, Event::Control(0x18))]);
        assert!(quiet[400..].iter().all(|&v| v == 0));
    }

    fn frame_events(s: &mut KonamiSound) -> Vec<i16> {
        let f = 50_688 * (AS_PER_SECOND / 3_072_000);
        let events = [
            (f / 2, Event::Latch(0x42)),
            (f / 2 + 1000, Event::Control(0x08)),
            (f / 2 + 2000, Event::Control(0x00)),
        ];
        let mut out = frame(s, 1, &events);
        out.extend(frame(s, 3, &[]));
        out
    }

    #[test]
    fn a_state_round_trips() {
        let mut s = KonamiSound::new(Kind::TwoAy, &program());
        frame_events(&mut s);
        let saved = bincode::serialize(&s).unwrap();
        let a = frame(&mut s, 5, &[]);
        let mut t: KonamiSound = bincode::deserialize(&saved).unwrap();
        t.restore_rom(&s);
        assert_eq!(frame(&mut t, 5, &[]), a);
    }

    #[test]
    fn frogger_swaps_d0_and_d1_of_its_first_rom() {
        let mut p = vec![0u8; 0x2000];
        p[0] = 0x01;
        p[0x800] = 0x01;
        let s = KonamiSound::new(Kind::Frogger, &p);
        assert_eq!((s.rom[0], s.rom[0x800]), (0x02, 0x01));
    }
}
