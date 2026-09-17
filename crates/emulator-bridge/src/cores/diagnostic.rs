//! Phase 1 stand-in core. **Contains no emulation.**
//!
//! Its entire purpose is to exercise the plumbing end to end before any C code
//! exists: it produces a synthetic video pattern, optional test audio, reacts to
//! input, and implements save-state so every path the UI touches is real.
//!
//! Concretely it proves, on real hardware, that:
//!
//! - the WebGPU upload/draw path presents a correctly oriented, correctly scaled
//!   frame (the pattern is asymmetric on purpose — a flipped blit is obvious);
//! - input reaches the core within the same tick that produced the frame (the
//!   D-pad moves the reticle);
//! - the audio ring drains to the device without clicks or drift.
//!
//! TODO(phase1b): [`crate::cores::registry::instantiate`] swaps this for the real
//! libretro instance. Nothing outside that function needs to change, because both
//! are just `EmulatorCore` implementations.

use super::{CoreDescriptor, EmulatorCore};
use crate::audio::{AudioSink, CHANNELS};
use crate::error::BridgeError;
use crate::frame::{FrameView, PixelFormat};
use crate::input::{Button, InputSnapshot};

const BYTES_PER_PIXEL: usize = 4;
const TONE_HZ: f64 = 440.0;
const TONE_AMPLITUDE: f32 = 0.06;

pub struct DiagnosticCore {
    descriptor: CoreDescriptor,
    framebuffer: Vec<u8>,
    width: u32,
    height: u32,
    frame_count: u64,
    /// Reticle position, moved by the D-pad to prove input latency is one frame.
    reticle: (f32, f32),
    content_label: Option<String>,
    // --- audio ---
    tone_enabled: bool,
    tone_phase: f64,
    /// Fractional carry so `samples_per_frame` averages out to the exact rate
    /// instead of drifting by a few samples a second.
    sample_debt: f64,
    audio_scratch: Vec<f32>,
}

impl DiagnosticCore {
    pub fn new(descriptor: CoreDescriptor) -> Self {
        let width = descriptor.geometry.base_width.max(1);
        let height = descriptor.geometry.base_height.max(1);
        let sample_rate = descriptor.audio_sample_rate.max(8_000) as f64;
        let fps = if descriptor.target_fps > 1.0 {
            descriptor.target_fps
        } else {
            60.0
        };
        // One frame of audio plus slack, allocated once.
        let audio_capacity = ((sample_rate / fps).ceil() as usize + 8) * CHANNELS;

        Self {
            descriptor,
            framebuffer: vec![0; width as usize * height as usize * BYTES_PER_PIXEL],
            width,
            height,
            frame_count: 0,
            reticle: (0.5, 0.5),
            content_label: None,
            tone_enabled: false,
            tone_phase: 0.0,
            sample_debt: 0.0,
            audio_scratch: Vec::with_capacity(audio_capacity),
        }
    }

    /// Enables a quiet reference tone. Off by default — an emulator that beeps at
    /// you on launch is a bug report waiting to happen.
    pub fn set_tone_enabled(&mut self, enabled: bool) {
        self.tone_enabled = enabled;
    }

    fn samples_this_frame(&mut self) -> usize {
        let rate = self.descriptor.audio_sample_rate.max(8_000) as f64;
        let fps = if self.descriptor.target_fps > 1.0 {
            self.descriptor.target_fps
        } else {
            60.0
        };
        let exact = rate / fps + self.sample_debt;
        let whole = exact.floor();
        self.sample_debt = exact - whole;
        whole as usize
    }

    fn render_pattern(&mut self, input: &InputSnapshot) {
        let w = self.width as usize;
        let h = self.height as usize;
        let t = self.frame_count as f32;

        // Slow diagonal sweep so a frozen frame is visually distinct from a
        // running one at a glance.
        let sweep = (t * 0.01).sin() * 0.5 + 0.5;
        let pressed_a = input.button(0, Button::A);

        for y in 0..h {
            let v = y as f32 / h as f32;
            for x in 0..w {
                let u = x as f32 / w as f32;

                // 16x16 checkerboard: makes non-integer scaling and filtering
                // artifacts immediately visible.
                let checker = ((x / 16) + (y / 16)) % 2 == 0;
                let base = if checker { 0.12 } else { 0.06 };

                let mut r = base + u * 0.55 * sweep;
                let mut g = base + v * 0.35;
                let mut b = base + (1.0 - u) * 0.65 * (1.0 - sweep);

                // Corner marker in the top-left only: an upside-down or mirrored
                // blit cannot hide.
                if x < w / 12 && y < h / 12 {
                    r = 0.95;
                    g = 0.25;
                    b = 0.35;
                }

                // Input reticle.
                let rx = self.reticle.0 * w as f32;
                let ry = self.reticle.1 * h as f32;
                let dx = (x as f32 - rx).abs();
                let dy = (y as f32 - ry).abs();
                if (dx < 12.0 && dy < 1.5) || (dy < 12.0 && dx < 1.5) {
                    r = if pressed_a { 1.0 } else { 0.55 };
                    g = 1.0;
                    b = if pressed_a { 0.4 } else { 0.95 };
                }

                let i = (y * w + x) * BYTES_PER_PIXEL;
                self.framebuffer[i] = (r.clamp(0.0, 1.0) * 255.0) as u8;
                self.framebuffer[i + 1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
                self.framebuffer[i + 2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
                self.framebuffer[i + 3] = 255;
            }
        }
    }
}

impl EmulatorCore for DiagnosticCore {
    fn descriptor(&self) -> &CoreDescriptor {
        &self.descriptor
    }

    fn load_content(&mut self, content: &[u8]) -> Result<(), BridgeError> {
        if content.is_empty() {
            return Err(BridgeError::InvalidContent {
                core_id: self.descriptor.id.clone(),
                reason: "content is empty".into(),
            });
        }
        self.content_label = Some(format!("{} bytes", content.len()));
        self.frame_count = 0;
        // Paint an initial frame so the framebuffer is meaningful the moment content
        // is loaded, rather than a black flash until the first `run_frame`. Real
        // cores behave the same way — `retro_load_game` leaves the framebuffer in a
        // defined state.
        let idle = crate::input::InputState::default().snapshot();
        self.render_pattern(&idle);
        Ok(())
    }

    fn run_frame(&mut self, input: &InputSnapshot) -> Result<(), BridgeError> {
        let speed = 0.006;
        if input.button(0, Button::Left) {
            self.reticle.0 = (self.reticle.0 - speed).max(0.0);
        }
        if input.button(0, Button::Right) {
            self.reticle.0 = (self.reticle.0 + speed).min(1.0);
        }
        if input.button(0, Button::Up) {
            self.reticle.1 = (self.reticle.1 - speed).max(0.0);
        }
        if input.button(0, Button::Down) {
            self.reticle.1 = (self.reticle.1 + speed).min(1.0);
        }

        self.render_pattern(input);
        self.frame_count += 1;
        Ok(())
    }

    fn video(&self) -> Option<FrameView<'_>> {
        Some(FrameView {
            data: &self.framebuffer,
            width: self.width,
            height: self.height,
            stride_bytes: self.width as usize * BYTES_PER_PIXEL,
            format: PixelFormat::Rgba8888,
        })
    }

    fn drain_audio(&mut self, sink: &mut dyn AudioSink) {
        let frames = self.samples_this_frame();
        if frames == 0 {
            return;
        }
        let rate = self.descriptor.audio_sample_rate.max(8_000) as f64;
        let step = TONE_HZ * core::f64::consts::TAU / rate;

        self.audio_scratch.clear();
        for _ in 0..frames {
            let s = if self.tone_enabled {
                (self.tone_phase.sin() as f32) * TONE_AMPLITUDE
            } else {
                0.0
            };
            self.audio_scratch.push(s);
            self.audio_scratch.push(s);
            self.tone_phase += step;
            if self.tone_phase > core::f64::consts::TAU {
                self.tone_phase -= core::f64::consts::TAU;
            }
        }
        sink.submit_f32(&self.audio_scratch);
    }

    fn reset(&mut self) -> Result<(), BridgeError> {
        self.frame_count = 0;
        self.reticle = (0.5, 0.5);
        self.tone_phase = 0.0;
        self.sample_debt = 0.0;
        Ok(())
    }

    /// 24 bytes: frame counter + reticle. Small, but enough to make the save-state
    /// UI a real feature rather than a disabled button.
    fn state_size(&self) -> usize {
        24
    }

    fn save_state(&self, dst: &mut [u8]) -> Result<usize, BridgeError> {
        if dst.len() < self.state_size() {
            return Err(BridgeError::SaveState(format!(
                "buffer of {} bytes is too small for {} bytes of state",
                dst.len(),
                self.state_size()
            )));
        }
        dst[0..8].copy_from_slice(&self.frame_count.to_le_bytes());
        dst[8..12].copy_from_slice(&self.reticle.0.to_le_bytes());
        dst[12..16].copy_from_slice(&self.reticle.1.to_le_bytes());
        dst[16..24].copy_from_slice(&self.tone_phase.to_le_bytes());
        Ok(self.state_size())
    }

    fn load_state(&mut self, src: &[u8]) -> Result<(), BridgeError> {
        if src.len() < self.state_size() {
            return Err(BridgeError::SaveState(format!(
                "state is {} bytes, expected {}",
                src.len(),
                self.state_size()
            )));
        }
        self.frame_count = u64::from_le_bytes(src[0..8].try_into().unwrap());
        self.reticle.0 = f32::from_le_bytes(src[8..12].try_into().unwrap());
        self.reticle.1 = f32::from_le_bytes(src[12..16].try_into().unwrap());
        self.tone_phase = f64::from_le_bytes(src[16..24].try_into().unwrap());
        Ok(())
    }

    fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::RingAudioSink;
    use crate::cores::CoreDescriptor;
    use crate::frame::FrameGeometry;
    use crate::input::InputState;

    fn descriptor() -> CoreDescriptor {
        CoreDescriptor {
            id: "diagnostic".into(),
            display_name: "Diagnostic".into(),
            systems: vec!["test".into()],
            geometry: FrameGeometry::new(256, 240, 4.0 / 3.0),
            target_fps: 60.0,
            audio_sample_rate: 48_000,
            pixel_format: PixelFormat::Rgba8888,
            module_url: String::new(),
        }
    }

    #[test]
    fn produces_a_valid_frame() {
        let mut core = DiagnosticCore::new(descriptor());
        let input = InputState::default().snapshot();
        core.run_frame(&input).unwrap();
        let view = core.video().unwrap();
        view.validate().unwrap();
        assert_eq!(view.width, 256);
        assert_eq!(core.frame_count(), 1);
    }

    #[test]
    fn input_moves_the_reticle() {
        let mut core = DiagnosticCore::new(descriptor());
        let mut state = InputState::default();
        let before = core.reticle.0;
        state.set_button(0, Button::Right, true);
        core.run_frame(&state.snapshot()).unwrap();
        assert!(core.reticle.0 > before);
    }

    #[test]
    fn audio_rate_averages_out_over_a_second() {
        let mut core = DiagnosticCore::new(descriptor());
        let mut sink = RingAudioSink::new(48_000, 48_000, 60.0, 8);
        let mut total = 0usize;
        let mut drain = vec![0.0; 4096];
        for _ in 0..60 {
            core.drain_audio(&mut sink);
            total += sink.drain(&mut drain);
        }
        // 48000 frames * 2 channels in one emulated second.
        assert_eq!(total, 96_000);
    }

    #[test]
    fn state_round_trips() {
        let mut core = DiagnosticCore::new(descriptor());
        let input = InputState::default().snapshot();
        for _ in 0..10 {
            core.run_frame(&input).unwrap();
        }
        let mut state = vec![0u8; core.state_size()];
        core.save_state(&mut state).unwrap();
        core.reset().unwrap();
        assert_eq!(core.frame_count(), 0);
        core.load_state(&state).unwrap();
        assert_eq!(core.frame_count(), 10);
    }

    #[test]
    fn rejects_empty_content() {
        let mut core = DiagnosticCore::new(descriptor());
        assert!(core.load_content(&[]).is_err());
    }
}
