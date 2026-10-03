//! Motion: `RETRO_ENVIRONMENT_GET_SENSOR_INTERFACE`, fed by the phone's accelerometer and gyro.
//!
//! The tilt cartridges are the reason: Yoshi Topsy-Turvy and Kirby Tilt 'n' Tumble read an
//! accelerometer in the cartridge, WarioWare: Twisted! reads a gyro, and mGBA asks the frontend
//! for both through this interface (`_initSensors` in mGBA's libretro.c enables the accelerometer
//! and the gyroscope on port 0 and reads `ACCELEROMETER_X/Y` and `GYROSCOPE_Z` every frame).
//! Azahar reads all six axes for the 3DS.
//!
//! THE SAMPLES NEVER TAKE THE ENGINE LOCK. CoreMotion delivers on its own queue at up to 100 Hz,
//! and the display link holds the engine lock for the whole of every tick, so a sample that waited
//! for it would arrive a frame late, every frame. Every value here is an atomic, exactly like the
//! microphone's hub: Swift stores, the core loads, and nobody waits.
//!
//! Units and axes are libretro's (libretro.h:5193-5264): acceleration in g INCLUDING gravity,
//! "a device at rest on a table will have values close to 0, 0, 1"; angular velocity in radians
//! per second, counter-clockwise positive. Note that iOS reports gravity with the opposite sign
//! (CoreMotion reads 0, 0, -1 flat on a table), so the host negates acceleration before pushing.
//!
//! What this adds on top of the raw sample, all of it the user's:
//! - an on/off switch (off reads as a phone lying perfectly still and flat),
//! - calibration: "the way I am holding it now is level",
//! - an invert for each tilt axis,
//! - the screen orientation, so tilting "right" means right on the screen whichever way the phone
//!   is turned, rather than right in portrait,
//! - a SHAKE: a short, strong, alternating burst, for games that read a shake as a gesture.

use std::ffi::{c_uint, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// `RETRO_ENVIRONMENT_GET_SENSOR_INTERFACE` (libretro.h:1203): `25 | RETRO_ENVIRONMENT_EXPERIMENTAL`
/// with `RETRO_ENVIRONMENT_EXPERIMENTAL = 0x10000` (libretro.h:729). The experimental bit is part
/// of the number a core sends; without it this arm would never match.
pub const ENV_GET_SENSOR_INTERFACE: c_uint = 25 | 0x10000;

// `enum retro_sensor_action` (libretro.h:5170-5191).
pub const RETRO_SENSOR_ACCELEROMETER_ENABLE: c_uint = 0;
pub const RETRO_SENSOR_ACCELEROMETER_DISABLE: c_uint = 1;
pub const RETRO_SENSOR_GYROSCOPE_ENABLE: c_uint = 2;
pub const RETRO_SENSOR_GYROSCOPE_DISABLE: c_uint = 3;
pub const RETRO_SENSOR_ILLUMINANCE_ENABLE: c_uint = 4;
pub const RETRO_SENSOR_ILLUMINANCE_DISABLE: c_uint = 5;

// Sensor ids (libretro.h:5206-5264).
pub const RETRO_SENSOR_ACCELEROMETER_X: c_uint = 0;
pub const RETRO_SENSOR_ACCELEROMETER_Y: c_uint = 1;
pub const RETRO_SENSOR_ACCELEROMETER_Z: c_uint = 2;
pub const RETRO_SENSOR_GYROSCOPE_X: c_uint = 3;
pub const RETRO_SENSOR_GYROSCOPE_Y: c_uint = 4;
pub const RETRO_SENSOR_GYROSCOPE_Z: c_uint = 5;
pub const RETRO_SENSOR_ILLUMINANCE: c_uint = 6;

/// `struct retro_sensor_interface` (libretro.h:5303). Two function pointers, in this order.
#[repr(C)]
pub struct RetroSensorInterface {
    pub set_sensor_state: unsafe extern "C" fn(c_uint, c_uint, c_uint) -> bool,
    pub get_sensor_input: unsafe extern "C" fn(c_uint, c_uint) -> f32,
}

/// Core frames a shake lasts. A third of a second at 60 Hz: long enough that a game sampling
/// every few frames sees it, short enough to read as one shake rather than a tremor.
pub const SHAKE_FRAMES: u32 = 20;
/// The burst's peak, in g. A real hard shake of a handheld is two to three g.
const SHAKE_G: f32 = 2.5;

/// Which way the screen is turned, so tilt follows the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ScreenOrientation {
    /// Home indicator at the bottom.
    Portrait = 0,
    /// `UIInterfaceOrientation.landscapeRight`: the phone's top edge is on the LEFT.
    LandscapeRight = 1,
    /// `UIInterfaceOrientation.landscapeLeft`: the phone's top edge is on the RIGHT.
    LandscapeLeft = 2,
    PortraitUpsideDown = 3,
}

impl ScreenOrientation {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::LandscapeRight,
            2 => Self::LandscapeLeft,
            3 => Self::PortraitUpsideDown,
            _ => Self::Portrait,
        }
    }

    /// Device axes (x right and y up, in portrait) to screen axes.
    pub fn rotate(self, x: f32, y: f32) -> (f32, f32) {
        match self {
            Self::Portrait => (x, y),
            // Top edge on the left: the device's +y points screen-left, its +x points screen-up.
            Self::LandscapeRight => (-y, x),
            // Top edge on the right: +y points screen-right, +x points screen-down.
            Self::LandscapeLeft => (y, -x),
            Self::PortraitUpsideDown => (-x, -y),
        }
    }
}

fn load_f32(cell: &AtomicU32) -> f32 {
    f32::from_bits(cell.load(Ordering::Relaxed))
}

fn store_f32(cell: &AtomicU32, value: f32) {
    let value = if value.is_finite() { value } else { 0.0 };
    cell.store(value.to_bits(), Ordering::Relaxed);
}

/// Everything shared between CoreMotion's queue, the main thread and the core's thread.
pub struct SensorHub {
    /// The user's switch. ON by default: a tilt game is unplayable without it, and the sensor is
    /// only actually run while a core has asked for it (see [`SensorHub::wanted`]).
    enabled: AtomicBool,
    invert_x: AtomicBool,
    invert_y: AtomicBool,
    orientation: AtomicU32,
    /// Latest sample, libretro convention, device axes.
    accel: [AtomicU32; 3],
    gyro: [AtomicU32; 3],
    /// The sample taken as "level" by calibrate. Starts as flat on a table.
    neutral: [AtomicU32; 3],
    samples: AtomicU64,
    /// What the core switched on. Per sensor, not per port: every core here reads port 0, and a
    /// phone has one accelerometer.
    core_accel: AtomicBool,
    core_gyro: AtomicBool,
    /// Whether a core has been handed the interface at all.
    interface_given: AtomicBool,
    /// Core frames of shake remaining, and the frame counter the burst alternates on.
    shake_left: AtomicU32,
    shake_phase: AtomicU32,
}

impl SensorHub {
    const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            invert_x: AtomicBool::new(false),
            invert_y: AtomicBool::new(false),
            orientation: AtomicU32::new(0),
            accel: [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0x3f80_0000)],
            gyro: [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)],
            neutral: [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0x3f80_0000)],
            samples: AtomicU64::new(0),
            core_accel: AtomicBool::new(false),
            core_gyro: AtomicBool::new(false),
            interface_given: AtomicBool::new(false),
            shake_left: AtomicU32::new(0),
            shake_phase: AtomicU32::new(0),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_inverted(&self, x: bool, y: bool) {
        self.invert_x.store(x, Ordering::Relaxed);
        self.invert_y.store(y, Ordering::Relaxed);
    }

    pub fn inverted(&self) -> (bool, bool) {
        (self.invert_x.load(Ordering::Relaxed), self.invert_y.load(Ordering::Relaxed))
    }

    pub fn set_orientation(&self, orientation: ScreenOrientation) {
        self.orientation.store(orientation as u32, Ordering::Relaxed);
    }

    pub fn orientation(&self) -> ScreenOrientation {
        ScreenOrientation::from_u32(self.orientation.load(Ordering::Relaxed))
    }

    /// One sample. Atomic stores only; safe from any thread at any rate.
    pub fn push(&self, accel: [f32; 3], gyro: [f32; 3]) {
        for (cell, value) in self.accel.iter().zip(accel) {
            store_f32(cell, value);
        }
        for (cell, value) in self.gyro.iter().zip(gyro) {
            store_f32(cell, value);
        }
        self.samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Takes the latest sample as level. Returns false when no sample has arrived yet, because
    /// calibrating against the start-up default would silently do nothing.
    pub fn calibrate(&self) -> bool {
        if self.samples.load(Ordering::Relaxed) == 0 {
            return false;
        }
        for (neutral, accel) in self.neutral.iter().zip(&self.accel) {
            neutral.store(accel.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        true
    }

    /// Forgets the calibration: flat on a table is level again.
    pub fn reset_calibration(&self) {
        store_f32(&self.neutral[0], 0.0);
        store_f32(&self.neutral[1], 0.0);
        store_f32(&self.neutral[2], 1.0);
    }

    pub fn neutral(&self) -> [f32; 3] {
        [load_f32(&self.neutral[0]), load_f32(&self.neutral[1]), load_f32(&self.neutral[2])]
    }

    /// Starts a shake burst.
    pub fn shake(&self) {
        self.shake_phase.store(0, Ordering::Relaxed);
        self.shake_left.store(SHAKE_FRAMES, Ordering::Relaxed);
    }

    pub fn shaking(&self) -> bool {
        self.shake_left.load(Ordering::Relaxed) > 0
    }

    /// Once per core frame, before `retro_run`: the burst advances by one frame.
    pub fn advance_frame(&self) {
        let left = self.shake_left.load(Ordering::Relaxed);
        if left > 0 {
            self.shake_left.store(left - 1, Ordering::Relaxed);
            self.shake_phase.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Whether the host should be running CoreMotion: a core switched a sensor on and the user
    /// allows it. Atomic reads only, so it is polled every frame.
    pub fn wanted(&self) -> bool {
        self.is_enabled()
            && (self.core_accel.load(Ordering::Relaxed) || self.core_gyro.load(Ordering::Relaxed))
    }

    pub fn samples(&self) -> u64 {
        self.samples.load(Ordering::Relaxed)
    }

    /// The shake's contribution this frame, in g, screen axes. Alternates sign every two frames
    /// and fades out, which is what a hand shaking a console looks like to its sensor.
    fn shake_offset(&self) -> (f32, f32) {
        let left = self.shake_left.load(Ordering::Relaxed);
        if left == 0 {
            return (0.0, 0.0);
        }
        let phase = self.shake_phase.load(Ordering::Relaxed);
        let sign = if (phase / 2) % 2 == 0 { 1.0 } else { -1.0 };
        let fade = left as f32 / SHAKE_FRAMES as f32;
        (sign * SHAKE_G * fade, sign * SHAKE_G * 0.5 * fade)
    }

    /// What a core reads for `id`. Calibrated, turned to the screen, inverted as asked, plus the
    /// shake. With motion switched off this is a phone lying still and flat, whatever the hand is
    /// doing, except for a shake, which is an explicit request and so is honoured either way.
    pub fn read(&self, id: c_uint) -> f32 {
        let enabled = self.is_enabled();
        let orientation = self.orientation();
        let (invert_x, invert_y) = self.inverted();
        match id {
            RETRO_SENSOR_ACCELEROMETER_X
            | RETRO_SENSOR_ACCELEROMETER_Y
            | RETRO_SENSOR_ACCELEROMETER_Z => {
                let (mut x, mut y, z) = if enabled {
                    let raw = [
                        load_f32(&self.accel[0]),
                        load_f32(&self.accel[1]),
                        load_f32(&self.accel[2]),
                    ];
                    let neutral = self.neutral();
                    // Relative to the calibrated level, then re-based on flat so a core that
                    // expects gravity on Z still finds it there.
                    let dx = raw[0] - neutral[0];
                    let dy = raw[1] - neutral[1];
                    let dz = raw[2] - neutral[2] + 1.0;
                    let (sx, sy) = orientation.rotate(dx, dy);
                    (sx, sy, dz)
                } else {
                    (0.0, 0.0, 1.0)
                };
                if invert_x {
                    x = -x;
                }
                if invert_y {
                    y = -y;
                }
                let (shake_x, shake_y) = self.shake_offset();
                match id {
                    RETRO_SENSOR_ACCELEROMETER_X => x + shake_x,
                    RETRO_SENSOR_ACCELEROMETER_Y => y + shake_y,
                    _ => z,
                }
            }
            RETRO_SENSOR_GYROSCOPE_X | RETRO_SENSOR_GYROSCOPE_Y | RETRO_SENSOR_GYROSCOPE_Z => {
                if !enabled {
                    return 0.0;
                }
                let (mut gx, mut gy) =
                    orientation.rotate(load_f32(&self.gyro[0]), load_f32(&self.gyro[1]));
                let gz = load_f32(&self.gyro[2]);
                // Rotation about X is what a forward tilt is, so inverting the Y tilt flips it,
                // and the same pairing the other way round.
                if invert_y {
                    gx = -gx;
                }
                if invert_x {
                    gy = -gy;
                }
                match id {
                    RETRO_SENSOR_GYROSCOPE_X => gx,
                    RETRO_SENSOR_GYROSCOPE_Y => gy,
                    _ => gz,
                }
            }
            // No light sensor is readable on iOS. Refused at enable time, so a core that asks
            // anyway gets "dark", which is libretro's absent value.
            _ => 0.0,
        }
    }

    /// The core's `set_sensor_state`.
    ///
    /// Accelerometer and gyroscope are accepted whether or not the user has motion on, which is
    /// deliberate and not a lie: mGBA asks ONCE, at its first frame, and never again
    /// (`sensorsInitDone`), so refusing while the switch happened to be off would leave the tilt
    /// dead for the rest of the session even after the switch went on. The switch is honoured per
    /// read instead. Illuminance is refused: there is no light sensor iOS will hand an app.
    pub fn set_state(&self, port: c_uint, action: c_uint) -> bool {
        if port as usize >= super::MAX_PORTS {
            return false;
        }
        match action {
            RETRO_SENSOR_ACCELEROMETER_ENABLE => {
                self.core_accel.store(true, Ordering::Relaxed);
                true
            }
            RETRO_SENSOR_ACCELEROMETER_DISABLE => {
                self.core_accel.store(false, Ordering::Relaxed);
                true
            }
            RETRO_SENSOR_GYROSCOPE_ENABLE => {
                self.core_gyro.store(true, Ordering::Relaxed);
                true
            }
            RETRO_SENSOR_GYROSCOPE_DISABLE => {
                self.core_gyro.store(false, Ordering::Relaxed);
                true
            }
            RETRO_SENSOR_ILLUMINANCE_ENABLE => false,
            RETRO_SENSOR_ILLUMINANCE_DISABLE => true,
            _ => false,
        }
    }

    /// A new core: nothing the last one switched on stays on. User settings are kept.
    pub fn reset_core_side(&self) {
        self.core_accel.store(false, Ordering::Relaxed);
        self.core_gyro.store(false, Ordering::Relaxed);
        self.interface_given.store(false, Ordering::Relaxed);
        self.shake_left.store(0, Ordering::Relaxed);
    }

    /// One sentence for the HUD and the Settings screen.
    pub fn status_line(&self) -> String {
        let asked = match (
            self.core_accel.load(Ordering::Relaxed),
            self.core_gyro.load(Ordering::Relaxed),
        ) {
            (true, true) => "the game reads tilt and rotation",
            (true, false) => "the game reads tilt",
            (false, true) => "the game reads rotation",
            (false, false) if self.interface_given.load(Ordering::Relaxed) => {
                "the core can read motion but this game has not asked"
            }
            (false, false) => "this core does not read motion",
        };
        if !self.is_enabled() {
            return format!("motion: off in Settings; {asked}");
        }
        format!("motion: on; {asked}; {} samples", self.samples())
    }
}

/// The process-global hub, for the reason every libretro callback target is global.
pub static SENSORS: SensorHub = SensorHub::new();

unsafe extern "C" fn on_set_sensor_state(port: c_uint, action: c_uint, _rate: c_uint) -> bool {
    SENSORS.set_state(port, action)
}

unsafe extern "C" fn on_get_sensor_input(port: c_uint, id: c_uint) -> f32 {
    if port as usize >= super::MAX_PORTS {
        return 0.0;
    }
    SENSORS.read(id)
}

/// `GET_SENSOR_INTERFACE`: fills in both function pointers. True whatever the phone has, which is
/// what libretro.h says the return means ("available, even if the device doesn't have any
/// supported sensors").
///
/// # Safety
/// `data` is what the core passed: `struct retro_sensor_interface *`.
pub unsafe fn answer_interface(data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    unsafe {
        let iface = data as *mut RetroSensorInterface;
        std::ptr::addr_of_mut!((*iface).set_sensor_state).write(on_set_sensor_state);
        std::ptr::addr_of_mut!((*iface).get_sensor_input).write(on_get_sensor_input);
    }
    SENSORS.interface_given.store(true, Ordering::Relaxed);
    true
}

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        let g = match TEST_LOCK.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        SENSORS.reset_core_side();
        SENSORS.set_enabled(true);
        SENSORS.set_inverted(false, false);
        SENSORS.set_orientation(ScreenOrientation::Portrait);
        SENSORS.reset_calibration();
        SENSORS.push([0.0, 0.0, 1.0], [0.0, 0.0, 0.0]);
        g
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn the_command_number_carries_the_experimental_bit() {
        assert_eq!(ENV_GET_SENSOR_INTERFACE, 0x10019);
    }

    #[test]
    fn the_interface_is_filled_and_answers() {
        let _g = guard();
        unsafe extern "C" fn dummy_state(_: c_uint, _: c_uint, _: c_uint) -> bool {
            false
        }
        unsafe extern "C" fn dummy_get(_: c_uint, _: c_uint) -> f32 {
            -9.0
        }
        let mut iface = RetroSensorInterface {
            set_sensor_state: dummy_state,
            get_sensor_input: dummy_get,
        };
        assert!(unsafe { answer_interface(&mut iface as *mut _ as *mut c_void) });
        // mGBA's first-frame sequence.
        assert!(unsafe { (iface.set_sensor_state)(0, RETRO_SENSOR_ACCELEROMETER_ENABLE, 60) });
        assert!(unsafe { (iface.set_sensor_state)(0, RETRO_SENSOR_GYROSCOPE_ENABLE, 60) });
        assert!(!unsafe { (iface.set_sensor_state)(0, RETRO_SENSOR_ILLUMINANCE_ENABLE, 60) });
        assert!(SENSORS.wanted());
        SENSORS.push([0.25, -0.5, 0.8], [0.0, 0.0, 1.5]);
        assert!(close(unsafe { (iface.get_sensor_input)(0, RETRO_SENSOR_ACCELEROMETER_X) }, 0.25));
        assert!(close(unsafe { (iface.get_sensor_input)(0, RETRO_SENSOR_ACCELEROMETER_Y) }, -0.5));
        assert!(close(unsafe { (iface.get_sensor_input)(0, RETRO_SENSOR_GYROSCOPE_Z) }, 1.5));
        assert!(!unsafe { answer_interface(std::ptr::null_mut()) });
    }

    #[test]
    fn off_reads_as_flat_and_still() {
        let _g = guard();
        SENSORS.push([0.7, 0.7, 0.1], [1.0, 1.0, 1.0]);
        SENSORS.set_enabled(false);
        assert_eq!(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X), 0.0);
        assert_eq!(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_Z), 1.0);
        assert_eq!(SENSORS.read(RETRO_SENSOR_GYROSCOPE_Z), 0.0);
        SENSORS.set_state(0, RETRO_SENSOR_ACCELEROMETER_ENABLE);
        assert!(!SENSORS.wanted(), "off means CoreMotion is not run");
        SENSORS.set_enabled(true);
    }

    #[test]
    fn calibration_makes_the_held_angle_level() {
        let _g = guard();
        SENSORS.push([0.3, -0.6, 0.74], [0.0; 3]);
        assert!(SENSORS.calibrate());
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X), 0.0));
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_Y), 0.0));
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_Z), 1.0));
        SENSORS.push([0.4, -0.6, 0.74], [0.0; 3]);
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X), 0.1));
        SENSORS.reset_calibration();
    }

    #[test]
    fn landscape_turns_tilt_to_the_screen() {
        let _g = guard();
        // Phone turned with its top edge to the LEFT and tipped so the screen's right side drops:
        // gravity's reaction then points along the device's -y... expressed as device x/y below.
        SENSORS.push([0.0, -0.5, 0.86], [0.0; 3]);
        SENSORS.set_orientation(ScreenOrientation::LandscapeRight);
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X), 0.5));
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_Y), 0.0));
        SENSORS.set_orientation(ScreenOrientation::LandscapeLeft);
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X), -0.5));
    }

    #[test]
    fn invert_flips_each_axis() {
        let _g = guard();
        SENSORS.push([0.2, 0.3, 0.9], [0.0; 3]);
        SENSORS.set_inverted(true, false);
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X), -0.2));
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_Y), 0.3));
        SENSORS.set_inverted(false, true);
        assert!(close(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_Y), -0.3));
    }

    #[test]
    fn a_shake_is_a_short_alternating_burst() {
        let _g = guard();
        SENSORS.shake();
        let mut readings = Vec::new();
        for _ in 0..(SHAKE_FRAMES + 5) {
            readings.push(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X));
            SENSORS.advance_frame();
        }
        assert!(readings[0] > 1.0, "a shake starts strong");
        assert!(readings.iter().any(|v| *v < -0.5), "and swings both ways");
        assert!(readings[SHAKE_FRAMES as usize..].iter().all(|v| *v == 0.0), "and ends");
        assert!(!SENSORS.shaking());
    }

    #[test]
    fn a_shake_is_honoured_with_motion_off() {
        let _g = guard();
        SENSORS.set_enabled(false);
        SENSORS.shake();
        assert!(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X) > 1.0);
        SENSORS.set_enabled(true);
        SENSORS.reset_core_side();
    }

    #[test]
    fn a_new_core_starts_with_nothing_switched_on() {
        let _g = guard();
        SENSORS.set_state(0, RETRO_SENSOR_GYROSCOPE_ENABLE);
        SENSORS.reset_core_side();
        assert!(!SENSORS.wanted());
        assert!(SENSORS.status_line().contains("does not read motion"));
    }

    #[test]
    fn non_finite_samples_are_ignored_as_zero() {
        let _g = guard();
        SENSORS.push([f32::NAN, f32::INFINITY, 1.0], [0.0; 3]);
        assert_eq!(SENSORS.read(RETRO_SENSOR_ACCELEROMETER_X), 0.0);
    }
}
