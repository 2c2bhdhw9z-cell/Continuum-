//! Timer-3 PWM and the small piezo speaker's measured high-pass response.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Audio {
    pub phase: u64,
    pub timer: u16,
    pub preset: u16,
    pub pivot: u16,
    pub enabled: bool,
    pub volume: u8,
    piezo_in: f32,
    piezo_out: f32,
    lowpass: f32,
}

impl Default for Audio {
    fn default() -> Self {
        Self {
            phase: 0,
            timer: 0xffff,
            preset: 0xffff,
            pivot: 0x7fff,
            enabled: false,
            volume: 3,
            piezo_in: 0.0,
            piezo_out: 0.0,
            lowpass: 0.0,
        }
    }
}

impl Audio {
    pub fn render(
        &mut self,
        cpu_cycles: u32,
        rate: u32,
        piezo: bool,
        lowpass_percent: u8,
        gain: u8,
        out: &mut Vec<i16>,
    ) {
        self.phase += cpu_cycles as u64 * rate as u64;
        while self.phase >= 4_000_000 {
            self.phase -= 4_000_000;
            let raw = if self.enabled && self.volume != 0 {
                if self.timer <= self.pivot {
                    1.0
                } else {
                    -1.0
                }
            } else {
                0.0
            } * (self.volume.min(3) as f32 / 3.0);
            let mut value = raw;
            if piezo {
                let hp = 0.985 * (self.piezo_out + raw - self.piezo_in);
                self.piezo_in = raw;
                self.piezo_out = hp;
                value = hp;
            }
            if lowpass_percent != 0 {
                let alpha = (100 - lowpass_percent.min(95)) as f32 / 100.0;
                self.lowpass += alpha * (value - self.lowpass);
                value = self.lowpass;
            }
            let sample =
                (value * 12_000.0 * gain.min(100) as f32 / 100.0).clamp(-32768.0, 32767.0) as i16;
            out.extend_from_slice(&[sample, sample]);
        }
    }
}
