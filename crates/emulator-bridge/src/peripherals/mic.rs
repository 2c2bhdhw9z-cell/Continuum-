//! The microphone: `RETRO_ENVIRONMENT_GET_MICROPHONE_INTERFACE` and the ring that feeds it.
//!
//! ## Who calls what, and on which thread
//!
//! - The CORE calls the `retro_microphone_interface` functions below (`open_mic`, `read_mic` and
//!   the rest) from its own thread, from inside `retro_run`. Azahar polls `read_mic` once per
//!   frame from `retro_run` and buffers the result itself (`audio_core/libretro_input.cpp`).
//! - SWIFT pushes captured audio from the AVAudioEngine input tap, on CoreAudio's tap thread,
//!   through `ContinuumEngine::push_microphone_samples`. That call never takes the engine
//!   `Mutex`: the display link holds that lock for the whole tick, and a tap thread waiting on it
//!   would drop audio every frame.
//!
//! So there is exactly one producer (the tap) and one consumer (the core), on two different
//! threads, and [`MicRing`] is the lock-free single-producer single-consumer ring between them.
//! The resampler lives on the producer side, behind a `Mutex` that only the producer ever takes,
//! so the consumer never waits on anything.
//!
//! ## Resampling
//!
//! The core names the rate it wants in `open_mic` (Azahar always asks for 48000, see the comment
//! on `kMicOpenRate` in its source). The phone's input runs at whatever the route gives, usually
//! 48000 but 44100 or 16000 on some Bluetooth headsets. The existing linear resampler in
//! `audio/resample.rs` converts, so `get_params` can honestly report the rate the core asked for.
//! It is stereo, so mono samples are fed through it duplicated and the left channel is kept.
//!
//! ## The user's switch
//!
//! "Allow microphone in games" is [`set_allowed`]. When it is off the interface is still
//! answered and `open_mic` still succeeds, so a core that asked at load keeps a working handle and
//! the switch can be flipped mid-game; `read_mic` simply hands back silence, and Swift never
//! starts the input tap, so iOS never shows the microphone indicator.

use std::ffi::{c_int, c_uint, c_void};
use std::sync::atomic::{AtomicBool, AtomicI16, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::audio::Resampler;

/// libretro.h:2489 `RETRO_ENVIRONMENT_GET_MICROPHONE_INTERFACE (75 | RETRO_ENVIRONMENT_EXPERIMENTAL)`.
/// The experimental bit is kept: dropping it would make an arm that can never match.
pub const ENV_GET_MICROPHONE_INTERFACE: c_uint = 75 | 0x10000;

/// libretro.h:7944 `RETRO_MICROPHONE_INTERFACE_VERSION 1`.
pub const MICROPHONE_INTERFACE_VERSION: c_uint = 1;

/// The rate handed out when a core opens a mic with `rate == 0` or with no params at all.
/// libretro.h:7898 says the frontend picks "some reasonable default".
pub const DEFAULT_MIC_RATE: u32 = 48_000;

/// Ring capacity in mono samples. 16384 is 341 ms at 48 kHz: generous headroom, because the
/// latency is bounded by [`MAX_LATENCY_MS`] on the read side, not by the capacity.
pub const MIC_RING_CAPACITY: usize = 16_384;

/// The most audio `read_mic` will let pile up before it skips to the newest samples.
///
/// A core that reads less than the mic produces (Azahar reads 128 samples per frame at 48 kHz,
/// which is 7680 a second against 48000 produced) would otherwise hear a breath seconds late. A
/// "blow into the mic" game needs the present moment, so old audio is dropped on the read side,
/// which is the side that owns the read cursor and can therefore do it without a lock.
pub const MAX_LATENCY_MS: u32 = 60;

// ------------------------------------------------------------------ libretro ABI

/// libretro.h:7891 `retro_microphone_params_t { unsigned rate; }`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RetroMicrophoneParams {
    pub rate: c_uint,
}

pub type RetroOpenMic = unsafe extern "C" fn(*const RetroMicrophoneParams) -> *mut c_void;
pub type RetroCloseMic = unsafe extern "C" fn(*mut c_void);
pub type RetroGetMicParams = unsafe extern "C" fn(*const c_void, *mut RetroMicrophoneParams) -> bool;
pub type RetroSetMicState = unsafe extern "C" fn(*mut c_void, bool) -> bool;
pub type RetroGetMicState = unsafe extern "C" fn(*const c_void) -> bool;
pub type RetroReadMic = unsafe extern "C" fn(*mut c_void, *mut i16, usize) -> c_int;

/// libretro.h:7951 `struct retro_microphone_interface`, field for field and in order.
#[repr(C)]
pub struct RetroMicrophoneInterface {
    pub interface_version: c_uint,
    pub open_mic: Option<RetroOpenMic>,
    pub close_mic: Option<RetroCloseMic>,
    pub get_params: Option<RetroGetMicParams>,
    pub set_mic_state: Option<RetroSetMicState>,
    pub get_mic_state: Option<RetroGetMicState>,
    pub read_mic: Option<RetroReadMic>,
}

// ------------------------------------------------------------------ the ring

/// Lock-free single-producer single-consumer ring of mono `i16` samples.
///
/// The cursors are free-running counters (they only ever increase, wrapping at `usize::MAX`) and
/// the capacity is a power of two, so `count = tail - head` and `index = cursor & mask` are both
/// exact with no "is it full or empty" ambiguity. The cells are `AtomicI16` so the whole thing is
/// safe Rust: relaxed cell traffic, published by a release store of the cursor and observed by an
/// acquire load of it on the other side.
///
/// RULES: exactly one thread may call the producer methods ([`MicRing::push`]) and exactly one
/// the consumer methods ([`MicRing::pop`], [`MicRing::skip_to_newest`], [`MicRing::clear`]).
/// [`MicHub`] enforces the producer half with a mutex nobody else takes; the consumer is the
/// core's own thread.
#[derive(Debug)]
pub struct MicRing {
    cells: Box<[AtomicI16]>,
    mask: usize,
    /// Read cursor. Written by the consumer only.
    head: AtomicUsize,
    /// Write cursor. Written by the producer only.
    tail: AtomicUsize,
    /// Samples the producer could not fit.
    overruns: AtomicU64,
    /// Samples the consumer threw away to keep latency bounded.
    skipped: AtomicU64,
}

impl MicRing {
    /// `capacity` is rounded up to a power of two, and is at least 64.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(64).next_power_of_two();
        let cells = (0..capacity).map(|_| AtomicI16::new(0)).collect::<Vec<_>>();
        Self {
            cells: cells.into_boxed_slice(),
            mask: capacity - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            overruns: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.cells.len()
    }

    /// Samples ready to read. Exact on the consumer thread; a lower bound on any other.
    pub fn available(&self) -> usize {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    pub fn overruns(&self) -> u64 {
        self.overruns.load(Ordering::Relaxed)
    }

    pub fn skipped(&self) -> u64 {
        self.skipped.load(Ordering::Relaxed)
    }

    /// PRODUCER. Appends as many samples as fit and returns how many that was. The rest are
    /// counted as overruns and dropped: the producer cannot move the read cursor, so it cannot
    /// make room by discarding old audio. The consumer does that in [`MicRing::skip_to_newest`].
    pub fn push(&self, samples: &[i16]) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let used = tail.wrapping_sub(head);
        let free = self.capacity().saturating_sub(used);
        let count = samples.len().min(free);
        for (offset, sample) in samples[..count].iter().enumerate() {
            self.cells[tail.wrapping_add(offset) & self.mask].store(*sample, Ordering::Relaxed);
        }
        self.tail.store(tail.wrapping_add(count), Ordering::Release);
        let dropped = samples.len() - count;
        if dropped > 0 {
            self.overruns.fetch_add(dropped as u64, Ordering::Relaxed);
        }
        count
    }

    /// CONSUMER. Copies up to `out.len()` samples out, oldest first, and returns the count.
    pub fn pop(&self, out: &mut [i16]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let count = tail.wrapping_sub(head).min(out.len());
        for (offset, slot) in out[..count].iter_mut().enumerate() {
            *slot = self.cells[head.wrapping_add(offset) & self.mask].load(Ordering::Relaxed);
        }
        self.head.store(head.wrapping_add(count), Ordering::Release);
        count
    }

    /// CONSUMER. Drops the oldest samples until at most `keep` remain, returning how many went.
    pub fn skip_to_newest(&self, keep: usize) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let count = tail.wrapping_sub(head);
        if count <= keep {
            return 0;
        }
        let drop = count - keep;
        self.head.store(head.wrapping_add(drop), Ordering::Release);
        self.skipped.fetch_add(drop as u64, Ordering::Relaxed);
        drop
    }

    /// CONSUMER. Empties the ring.
    pub fn clear(&self) {
        let tail = self.tail.load(Ordering::Acquire);
        self.head.store(tail, Ordering::Release);
    }
}

// ------------------------------------------------------------------ the hub

/// Producer-only state: the resampler and its scratch buffers.
#[derive(Debug, Default)]
struct Producer {
    resampler: Option<Resampler>,
    stereo_in: Vec<f32>,
    stereo_out: Vec<f32>,
    mono_out: Vec<i16>,
}

/// Everything the microphone path shares between the core, the tap and Swift's poll.
///
/// One microphone at a time. libretro lets `open_mic` return NULL when "the maximum number of
/// supported microphones has been reached", and a phone has one useful microphone, so a second
/// open while one is held is refused rather than handed a second view of the same stream.
#[derive(Debug)]
pub struct MicHub {
    ring: MicRing,
    producer: Mutex<Producer>,
    /// The Settings switch. Defaults to off: nothing reaches the microphone unless asked.
    allowed: AtomicBool,
    /// Swift says the input tap is running and delivering.
    capturing: AtomicBool,
    /// The core holds the handle.
    open: AtomicBool,
    /// The core switched the handle on with `set_mic_state(true)`.
    active: AtomicBool,
    /// The rate the core asked for, which is the rate `read_mic` delivers.
    rate: AtomicU32,
    /// The rate the last push arrived at, for the HUD.
    source_rate: AtomicU32,
    /// Samples handed to the core by `read_mic`, real audio only (not silence).
    delivered: AtomicU64,
    /// `read_mic` calls answered with silence because nothing was capturing.
    silent_reads: AtomicU64,
    /// Times the core asked for the interface.
    interface_requests: AtomicU32,
}

impl MicHub {
    pub fn new() -> Self {
        Self {
            ring: MicRing::new(MIC_RING_CAPACITY),
            producer: Mutex::new(Producer::default()),
            allowed: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
            open: AtomicBool::new(false),
            active: AtomicBool::new(false),
            rate: AtomicU32::new(DEFAULT_MIC_RATE),
            source_rate: AtomicU32::new(0),
            delivered: AtomicU64::new(0),
            silent_reads: AtomicU64::new(0),
            interface_requests: AtomicU32::new(0),
        }
    }

    pub fn ring(&self) -> &MicRing {
        &self.ring
    }

    pub fn set_allowed(&self, allowed: bool) {
        self.allowed.store(allowed, Ordering::Release);
    }

    pub fn allowed(&self) -> bool {
        self.allowed.load(Ordering::Acquire)
    }

    pub fn set_capturing(&self, capturing: bool) {
        self.capturing.store(capturing, Ordering::Release);
    }

    pub fn capturing(&self) -> bool {
        self.capturing.load(Ordering::Acquire)
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub fn rate(&self) -> u32 {
        self.rate.load(Ordering::Acquire)
    }

    pub fn interface_requests(&self) -> u32 {
        self.interface_requests.load(Ordering::Relaxed)
    }

    /// Forgets every core-side fact. Called when a core is loaded and when it is released, so a
    /// handle the last core held can never be honoured for the next one.
    pub fn reset_core_side(&self) {
        self.open.store(false, Ordering::Release);
        self.active.store(false, Ordering::Release);
        self.rate.store(DEFAULT_MIC_RATE, Ordering::Release);
        self.delivered.store(0, Ordering::Relaxed);
        self.silent_reads.store(0, Ordering::Relaxed);
        self.interface_requests.store(0, Ordering::Relaxed);
        self.ring.clear();
    }

    /// Whether Swift should have the input tap running: a core holds a mic and the user allows it.
    ///
    /// Open rather than active, deliberately. Azahar opens the mic and switches it off again
    /// immediately, then toggles it on for each sampling session; following `active` would
    /// rebuild the audio session (and glitch the game's sound) every time a game started
    /// listening. Holding the input for as long as the handle is held matches libretro's own
    /// description of an opened-but-inactive mic as "available but idle".
    pub fn wants_capture(&self) -> bool {
        self.is_open() && self.allowed()
    }

    /// PRODUCER. Takes mono float samples at `source_rate` from the tap, resamples them to the
    /// rate the core asked for and appends them to the ring. Returns how many samples went in.
    ///
    /// Dropped (returning 0) whenever the core is not listening, so the ring never holds audio
    /// from before the game asked for it.
    pub fn push_float(&self, samples: &[f32], source_rate: u32) -> usize {
        if samples.is_empty() || source_rate == 0 {
            return 0;
        }
        self.source_rate.store(source_rate, Ordering::Relaxed);
        if !(self.is_open() && self.is_active() && self.allowed()) {
            return 0;
        }
        let target = self.rate();
        let mut guard = match self.producer.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let producer = &mut *guard;
        let rebuild = match producer.resampler.as_ref() {
            Some(existing) => {
                existing.source_rate() != source_rate || existing.output_rate() != target
            }
            None => true,
        };
        if rebuild {
            producer.resampler = Some(Resampler::new(source_rate, target));
        }
        let Some(resampler) = producer.resampler.as_mut() else {
            return 0;
        };

        producer.stereo_in.clear();
        for sample in samples {
            producer.stereo_in.push(*sample);
            producer.stereo_in.push(*sample);
        }
        producer.stereo_out.clear();
        resampler.process(&producer.stereo_in, &mut producer.stereo_out);

        producer.mono_out.clear();
        for frame in producer.stereo_out.chunks_exact(2) {
            producer.mono_out.push(float_to_i16(frame[0]));
        }
        self.ring.push(&producer.mono_out)
    }

    /// PRODUCER, for a caller that already has 16-bit samples at the core's rate. Used by tests
    /// and by any future platform whose capture API hands out integers.
    pub fn push_i16(&self, samples: &[i16]) -> usize {
        if !(self.is_open() && self.is_active() && self.allowed()) {
            return 0;
        }
        let _producer = match self.producer.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.ring.push(samples)
    }

    /// The HUD sentence. Never empty, and different for every state that matters.
    pub fn status_line(&self) -> String {
        if self.interface_requests() == 0 {
            return "mic: this core did not ask for a microphone".into();
        }
        if !self.is_open() {
            return "mic: offered to the core, no microphone open".into();
        }
        if !self.allowed() {
            return "mic: the game opened the microphone, but Allow microphone in games is off, \
                    so it hears silence"
                .into();
        }
        if !self.capturing() {
            return format!(
                "mic: open at {} Hz, waiting for the iPhone microphone to start",
                self.rate()
            );
        }
        format!(
            "mic: {} at {} Hz from {} Hz input, {} samples delivered, {} skipped for latency, {} \
             overrun",
            if self.is_active() { "listening" } else { "open, idle" },
            self.rate(),
            self.source_rate.load(Ordering::Relaxed),
            self.delivered.load(Ordering::Relaxed),
            self.ring.skipped(),
            self.ring.overruns()
        )
    }
}

impl Default for MicHub {
    fn default() -> Self {
        Self::new()
    }
}

fn float_to_i16(sample: f32) -> i16 {
    if !sample.is_finite() {
        return 0;
    }
    (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

static HUB: OnceLock<MicHub> = OnceLock::new();

/// The process-wide hub. A global because libretro's callbacks carry no user data, for the same
/// reason every other libretro exchange in `native_core.rs` is a static.
pub fn hub() -> &'static MicHub {
    HUB.get_or_init(MicHub::new)
}

pub fn set_allowed(allowed: bool) {
    hub().set_allowed(allowed);
}

// ------------------------------------------------------------------ the callbacks

/// What `open_mic` returns. Its address is the handle; nothing is ever read through it.
static HANDLE_TOKEN: u8 = 0;

fn handle() -> *mut c_void {
    std::ptr::addr_of!(HANDLE_TOKEN) as *mut c_void
}

fn is_our_handle(pointer: *const c_void) -> bool {
    !pointer.is_null() && std::ptr::eq(pointer, handle())
}

unsafe extern "C" fn mic_open(params: *const RetroMicrophoneParams) -> *mut c_void {
    let hub = hub();
    if hub.is_open() {
        log::warn!("mic: the core asked for a second microphone; only one is offered");
        return std::ptr::null_mut();
    }
    let requested = if params.is_null() {
        0
    } else {
        unsafe { (*params).rate }
    };
    let rate = if requested == 0 {
        DEFAULT_MIC_RATE
    } else {
        requested
    };
    hub.rate.store(rate, Ordering::Release);
    hub.active.store(false, Ordering::Release);
    hub.ring.clear();
    hub.open.store(true, Ordering::Release);
    log::info!("mic: opened at {rate} Hz (requested {requested})");
    handle()
}

unsafe extern "C" fn mic_close(microphone: *mut c_void) {
    if !is_our_handle(microphone) {
        return;
    }
    let hub = hub();
    hub.active.store(false, Ordering::Release);
    hub.open.store(false, Ordering::Release);
    hub.ring.clear();
    log::info!("mic: closed by the core");
}

unsafe extern "C" fn mic_get_params(
    microphone: *const c_void,
    params: *mut RetroMicrophoneParams,
) -> bool {
    if !is_our_handle(microphone) || params.is_null() || !hub().is_open() {
        return false;
    }
    unsafe { (*params).rate = hub().rate() };
    true
}

unsafe extern "C" fn mic_set_state(microphone: *mut c_void, state: bool) -> bool {
    let hub = hub();
    if !is_our_handle(microphone) || !hub.is_open() {
        return false;
    }
    let was = hub.active.swap(state, Ordering::AcqRel);
    if was != state {
        // Fresh audio from the moment the game starts listening, nothing older.
        hub.ring.clear();
    }
    true
}

unsafe extern "C" fn mic_get_state(microphone: *const c_void) -> bool {
    let hub = hub();
    is_our_handle(microphone) && hub.is_open() && hub.is_active()
}

unsafe extern "C" fn mic_read(microphone: *mut c_void, samples: *mut i16, count: usize) -> c_int {
    let hub = hub();
    if !is_our_handle(microphone) || !hub.is_open() || !hub.is_active() || samples.is_null() {
        return -1;
    }
    // c_int return, so a request larger than it can express is clamped rather than wrapped.
    let count = count.min(c_int::MAX as usize);
    let out = unsafe { std::slice::from_raw_parts_mut(samples, count) };

    // libretro.h:8074: "If microphone is pending driver initialization, this function will copy
    // silence of the requested length". The switch being off, and the tap not running yet, are
    // both that from the core's point of view.
    if !hub.allowed() || !hub.capturing() {
        out.fill(0);
        hub.silent_reads.fetch_add(1, Ordering::Relaxed);
        return count as c_int;
    }

    let keep = (hub.rate() as usize * MAX_LATENCY_MS as usize / 1000).max(count);
    hub.ring.skip_to_newest(keep);
    let copied = hub.ring.pop(out);
    hub.delivered.fetch_add(copied as u64, Ordering::Relaxed);
    copied as c_int
}

/// Answers `GET_MICROPHONE_INTERFACE`.
///
/// # Safety
///
/// `data` must be null or point at a writable `retro_microphone_interface`.
pub unsafe fn answer_interface(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    let hub = hub();
    hub.interface_requests.fetch_add(1, Ordering::Relaxed);
    let interface = data as *mut RetroMicrophoneInterface;
    let requested = unsafe { (*interface).interface_version };
    unsafe {
        interface.write(RetroMicrophoneInterface {
            interface_version: MICROPHONE_INTERFACE_VERSION,
            open_mic: Some(mic_open),
            close_mic: Some(mic_close),
            get_params: Some(mic_get_params),
            set_mic_state: Some(mic_set_state),
            get_mic_state: Some(mic_get_state),
            read_mic: Some(mic_read),
        });
    }
    log::info!(
        "mic: interface version {MICROPHONE_INTERFACE_VERSION} given to the core (it asked for \
         {requested})"
    );
    true
}

/// Serialises every test, in any module, that touches the global hub.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn struct_layouts_match_libretro_h() {
        // unsigned + padding, then six function pointers.
        let pointer = std::mem::size_of::<usize>();
        assert_eq!(
            std::mem::size_of::<RetroMicrophoneInterface>(),
            pointer + 6 * pointer
        );
        assert_eq!(std::mem::size_of::<RetroMicrophoneParams>(), 4);
        assert_eq!(ENV_GET_MICROPHONE_INTERFACE, 0x1004B);
    }

    #[test]
    fn ring_round_trips_in_order() {
        let ring = MicRing::new(64);
        assert_eq!(ring.push(&[1, 2, 3, 4, 5]), 5);
        let mut out = [0i16; 3];
        assert_eq!(ring.pop(&mut out), 3);
        assert_eq!(out, [1, 2, 3]);
        assert_eq!(ring.available(), 2);
        let mut rest = [0i16; 8];
        assert_eq!(ring.pop(&mut rest), 2);
        assert_eq!(&rest[..2], &[4, 5]);
        assert_eq!(ring.available(), 0);
    }

    #[test]
    fn ring_wraps_and_counts_overruns() {
        let ring = MicRing::new(64);
        assert_eq!(ring.capacity(), 64);
        let first: Vec<i16> = (0..60).collect();
        assert_eq!(ring.push(&first), 60);
        let mut out = vec![0i16; 50];
        assert_eq!(ring.pop(&mut out), 50);
        // 10 left, 54 free: pushing 60 crosses the end of storage and drops 6.
        let second: Vec<i16> = (100..160).collect();
        assert_eq!(ring.push(&second), 54);
        assert_eq!(ring.overruns(), 6);
        let mut all = vec![0i16; 64];
        assert_eq!(ring.pop(&mut all), 64);
        let expected: Vec<i16> = (50..60).chain(100..154).collect();
        assert_eq!(all, expected);
    }

    #[test]
    fn ring_skip_keeps_only_the_newest() {
        let ring = MicRing::new(128);
        ring.push(&(0..100).collect::<Vec<i16>>());
        assert_eq!(ring.skip_to_newest(10), 90);
        assert_eq!(ring.skipped(), 90);
        let mut out = vec![0i16; 10];
        assert_eq!(ring.pop(&mut out), 10);
        assert_eq!(out, (90..100).collect::<Vec<i16>>());
        assert_eq!(ring.skip_to_newest(10), 0);
    }

    #[test]
    fn ring_capacity_rounds_up_to_a_power_of_two() {
        assert_eq!(MicRing::new(100).capacity(), 128);
        assert_eq!(MicRing::new(1).capacity(), 64);
    }

    #[test]
    fn ring_is_ordered_across_two_threads() {
        // One producer thread, one consumer thread, a long sequence: every sample arrives once,
        // in order, with nothing invented. This is the property the core depends on.
        let ring = Arc::new(MicRing::new(256));
        let total: i32 = 200_000;
        let producer = {
            let ring = Arc::clone(&ring);
            std::thread::spawn(move || {
                let mut next: i32 = 0;
                while next < total {
                    let end = (next + 37).min(total);
                    let batch: Vec<i16> = (next..end).map(|v| (v % 30_000) as i16).collect();
                    let pushed = ring.push(&batch);
                    next += pushed as i32;
                    if pushed == 0 {
                        std::thread::yield_now();
                    }
                }
            })
        };
        let mut expected: i32 = 0;
        let mut out = [0i16; 53];
        while expected < total {
            let got = ring.pop(&mut out);
            for sample in &out[..got] {
                assert_eq!(*sample, (expected % 30_000) as i16);
                expected += 1;
            }
            if got == 0 {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
        assert_eq!(ring.available(), 0);
    }

    #[test]
    fn float_conversion_clamps_and_rejects_nan() {
        assert_eq!(float_to_i16(0.0), 0);
        assert_eq!(float_to_i16(1.0), 32767);
        assert_eq!(float_to_i16(-2.0), -32767);
        assert_eq!(float_to_i16(f32::NAN), 0);
    }

    fn locked() -> std::sync::MutexGuard<'static, ()> {
        let guard = match TEST_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        hub().reset_core_side();
        hub().set_allowed(false);
        hub().set_capturing(false);
        guard
    }

    fn interface() -> RetroMicrophoneInterface {
        let mut interface = RetroMicrophoneInterface {
            interface_version: MICROPHONE_INTERFACE_VERSION,
            open_mic: None,
            close_mic: None,
            get_params: None,
            set_mic_state: None,
            get_mic_state: None,
            read_mic: None,
        };
        let ok = unsafe { answer_interface(&mut interface as *mut _ as *mut c_void) };
        assert!(ok);
        interface
    }

    #[test]
    fn answer_fills_every_function_and_the_version() {
        let _guard = locked();
        let interface = interface();
        assert_eq!(interface.interface_version, 1);
        assert!(interface.open_mic.is_some());
        assert!(interface.close_mic.is_some());
        assert!(interface.get_params.is_some());
        assert!(interface.set_mic_state.is_some());
        assert!(interface.get_mic_state.is_some());
        assert!(interface.read_mic.is_some());
        assert!(!unsafe { answer_interface(std::ptr::null_mut()) });
    }

    #[test]
    fn open_get_params_state_and_close() {
        let _guard = locked();
        let api = interface();
        unsafe {
            let params = RetroMicrophoneParams { rate: 32_728 };
            let mic = (api.open_mic.unwrap())(&params);
            assert!(!mic.is_null());
            // One microphone at a time.
            assert!((api.open_mic.unwrap())(&params).is_null());

            let mut read_back = RetroMicrophoneParams::default();
            assert!((api.get_params.unwrap())(mic, &mut read_back));
            assert_eq!(read_back.rate, 32_728);

            // Inactive by default, and a read on an inactive mic is -1.
            assert!(!(api.get_mic_state.unwrap())(mic));
            let mut buffer = [7i16; 16];
            assert_eq!((api.read_mic.unwrap())(mic, buffer.as_mut_ptr(), 16), -1);

            assert!((api.set_mic_state.unwrap())(mic, true));
            assert!((api.get_mic_state.unwrap())(mic));

            // A foreign handle is refused everywhere.
            let bogus = 0x1234usize as *mut c_void;
            assert!(!(api.set_mic_state.unwrap())(bogus, true));
            assert!(!(api.get_mic_state.unwrap())(bogus));
            assert_eq!((api.read_mic.unwrap())(bogus, buffer.as_mut_ptr(), 16), -1);
            assert!(!(api.get_params.unwrap())(bogus, &mut read_back));

            (api.close_mic.unwrap())(mic);
            assert!(!hub().is_open());
            assert!(!(api.get_mic_state.unwrap())(mic));
            // Closing NULL does nothing, as libretro.h requires.
            (api.close_mic.unwrap())(std::ptr::null_mut());
        }
    }

    #[test]
    fn null_params_or_zero_rate_get_the_default() {
        let _guard = locked();
        let api = interface();
        unsafe {
            let mic = (api.open_mic.unwrap())(std::ptr::null());
            let mut read_back = RetroMicrophoneParams::default();
            assert!((api.get_params.unwrap())(mic, &mut read_back));
            assert_eq!(read_back.rate, DEFAULT_MIC_RATE);
            (api.close_mic.unwrap())(mic);
        }
    }

    #[test]
    fn read_is_silence_until_allowed_and_capturing() {
        let _guard = locked();
        let api = interface();
        unsafe {
            let mic = (api.open_mic.unwrap())(&RetroMicrophoneParams { rate: 48_000 });
            assert!((api.set_mic_state.unwrap())(mic, true));
            let mut buffer = [9i16; 32];
            // Not allowed: silence of the requested length.
            assert_eq!((api.read_mic.unwrap())(mic, buffer.as_mut_ptr(), 32), 32);
            assert!(buffer.iter().all(|s| *s == 0));
            assert!(!hub().wants_capture());

            // Allowed: Swift is told to capture, but until it does, still silence.
            hub().set_allowed(true);
            assert!(hub().wants_capture());
            buffer.fill(9);
            assert_eq!((api.read_mic.unwrap())(mic, buffer.as_mut_ptr(), 32), 32);
            assert!(buffer.iter().all(|s| *s == 0));

            // Capturing: real samples, and only as many as exist.
            hub().set_capturing(true);
            assert_eq!(hub().push_i16(&[10, 20, 30]), 3);
            buffer.fill(9);
            assert_eq!((api.read_mic.unwrap())(mic, buffer.as_mut_ptr(), 32), 3);
            assert_eq!(&buffer[..3], &[10, 20, 30]);
            assert_eq!((api.read_mic.unwrap())(mic, buffer.as_mut_ptr(), 32), 0);
            (api.close_mic.unwrap())(mic);
        }
    }

    #[test]
    fn push_is_dropped_while_the_core_is_not_listening() {
        let _guard = locked();
        hub().set_allowed(true);
        hub().set_capturing(true);
        assert_eq!(hub().push_float(&[0.5; 100], 48_000), 0);
        let api = interface();
        unsafe {
            let mic = (api.open_mic.unwrap())(&RetroMicrophoneParams { rate: 48_000 });
            // Open but inactive: still dropped.
            assert_eq!(hub().push_float(&[0.5; 100], 48_000), 0);
            assert!((api.set_mic_state.unwrap())(mic, true));
            assert_eq!(hub().push_float(&[0.5; 100], 48_000), 100);
            (api.close_mic.unwrap())(mic);
        }
    }

    #[test]
    fn push_resamples_to_the_rate_the_core_asked_for() {
        let _guard = locked();
        hub().set_allowed(true);
        hub().set_capturing(true);
        let api = interface();
        unsafe {
            let mic = (api.open_mic.unwrap())(&RetroMicrophoneParams { rate: 16_000 });
            assert!((api.set_mic_state.unwrap())(mic, true));
            // 4800 samples at 48 kHz is 100 ms, which is about 1600 at 16 kHz.
            let pushed = hub().push_float(&vec![0.25; 4_800], 48_000);
            assert!((1_598..=1_602).contains(&pushed), "pushed {pushed}");
            let mut out = vec![0i16; 4_096];
            // Reading more than the latency cap would keep: the newest 60 ms (960) survive.
            let got = (api.read_mic.unwrap())(mic, out.as_mut_ptr(), 512);
            assert_eq!(got, 512);
            let quarter = (0.25f32 * 32767.0).round() as i16;
            assert!(out[..512].iter().all(|s| (*s - quarter).abs() <= 1));
            (api.close_mic.unwrap())(mic);
        }
    }

    #[test]
    fn read_bounds_latency_by_skipping_old_audio() {
        let _guard = locked();
        hub().set_allowed(true);
        hub().set_capturing(true);
        let api = interface();
        unsafe {
            let mic = (api.open_mic.unwrap())(&RetroMicrophoneParams { rate: 48_000 });
            assert!((api.set_mic_state.unwrap())(mic, true));
            let old: Vec<i16> = vec![1; 10_000];
            let new: Vec<i16> = vec![2; 128];
            hub().push_i16(&old);
            hub().push_i16(&new);
            // 60 ms at 48 kHz is 2880 samples kept, so the first read starts inside the old run
            // but much nearer its end, and the final 128 are the new ones.
            let mut out = vec![0i16; 2_880];
            let got = (api.read_mic.unwrap())(mic, out.as_mut_ptr(), out.len()) as usize;
            assert_eq!(got, 2_880);
            assert!(out[got - 128..].iter().all(|s| *s == 2));
            assert!(hub().ring().skipped() >= 10_000 + 128 - 2_880);
            (api.close_mic.unwrap())(mic);
        }
    }

    #[test]
    fn reset_forgets_the_handle() {
        let _guard = locked();
        let api = interface();
        unsafe {
            let mic = (api.open_mic.unwrap())(std::ptr::null());
            assert!((api.set_mic_state.unwrap())(mic, true));
            hub().reset_core_side();
            assert!(!(api.get_mic_state.unwrap())(mic));
            assert!(!(api.set_mic_state.unwrap())(mic, true));
        }
    }

    #[test]
    fn status_line_names_each_state() {
        let _guard = locked();
        assert!(hub().status_line().contains("did not ask"));
        let api = interface();
        assert!(hub().status_line().contains("no microphone open"));
        unsafe {
            let mic = (api.open_mic.unwrap())(std::ptr::null());
            assert!(hub().status_line().contains("is off"));
            hub().set_allowed(true);
            assert!(hub().status_line().contains("waiting"));
            hub().set_capturing(true);
            assert!(hub().status_line().contains("open, idle"));
            (api.set_mic_state.unwrap())(mic, true);
            assert!(hub().status_line().contains("listening"));
            (api.close_mic.unwrap())(mic);
        }
    }
}
