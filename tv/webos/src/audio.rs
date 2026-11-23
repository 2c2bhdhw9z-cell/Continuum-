//! Sound out through SDL's queue. The game runs one frame per TV refresh, so its sound is
//! stretched very slightly to match the TV's speed, with a gentle nudge that keeps the queue at
//! the chosen delay (no crackles, no drift).

use crate::sdl::{AudioSpec, Sdl, AUDIO_S16LSB};

pub struct Audio {
    dev: u32,
    pub rate: u32,
    pos: f64,
    last: [i16; 2],
    out: Vec<i16>,
    pub target_ms: u32,
    paused: bool,
    /// How much the sound is stretched right now. It glides towards what the queue asks for
    /// instead of jumping, so the pitch never wobbles (owner, 22 November 2026: it sounded
    /// "sped up, then slowed").
    stretch: f64,
    /// For the log every 10 seconds: the emptiest the queue got and the most it was stretched.
    low_ms: f64,
    most: f64,
    pushes: u32,
}

impl Audio {
    pub fn open(sdl: &Sdl, rate: u32, target_ms: u32) -> Option<Audio> {
        let want = AudioSpec {
            freq: rate as i32,
            format: AUDIO_S16LSB,
            channels: 2,
            silence: 0,
            samples: 512,
            padding: 0,
            size: 0,
            callback: std::ptr::null(),
            userdata: std::ptr::null_mut(),
        };
        let mut have = AudioSpec { ..want };
        let dev = unsafe { (sdl.OpenAudioDevice)(std::ptr::null(), 0, &want, &mut have, 0) };
        if dev == 0 {
            crate::logln!("no sound: {}", sdl.error());
            return None;
        }
        crate::logln!("sound: {} Hz, {} channels, {} sample buffer", have.freq, have.channels, have.samples);
        unsafe { (sdl.PauseAudioDevice)(dev, 1) };
        Some(Audio { dev, rate: have.freq as u32, pos: 0.0, last: [0; 2], out: Vec::new(), target_ms, paused: true, stretch: 0.0, low_ms: f64::MAX, most: 0.0, pushes: 0 })
    }

    pub fn queued_ms(&self, sdl: &Sdl) -> f64 {
        let bytes = unsafe { (sdl.GetQueuedAudioSize)(self.dev) } as f64;
        bytes / 4.0 / self.rate as f64 * 1000.0
    }

    /// Queue one frame's sound. `ratio` is how many output samples per input sample before the
    /// nudge (TV refresh vs. game speed and the two sample rates).
    pub fn push(&mut self, sdl: &Sdl, input: &[i16], in_rate: f64, game_fps: f64, refresh: f64, volume: u32) {
        if input.len() < 2 {
            return;
        }
        let queued = self.queued_ms(sdl);
        let target = self.target_ms as f64;
        // Too much queued (the game was paused, or the TV ran slow): drop the extra at once.
        if queued > target * 3.0 {
            unsafe { (sdl.ClearQueuedAudio)(self.dev) };
        }
        // Keep the queue at the chosen delay with a nudge too small to hear. When the game falls
        // a little behind (a heavy moment), stretch the sound up to 2% instead of letting the
        // queue run dry, which is what crackles. The stretch glides there over about a second
        // (a sudden change of speed is what you hear), except when the queue is nearly empty.
        let mut want = ((target - queued) / target).clamp(-1.0, 1.0) * 0.005;
        if queued < target * 0.5 {
            want += 0.015 * ((target * 0.5 - queued) / (target * 0.5)).clamp(0.0, 1.0);
        }
        self.stretch += (want - self.stretch) * 0.04;
        if queued < target * 0.2 {
            self.stretch = self.stretch.max(want);
        }
        self.low_ms = self.low_ms.min(queued);
        self.most = self.most.max(self.stretch);
        self.pushes += 1;
        if self.pushes == 600 {
            crate::logln!("sound: queue at least {:.0} ms, stretched up to {:.2}%", self.low_ms, self.most * 100.0);
            self.pushes = 0;
            self.low_ms = f64::MAX;
            self.most = 0.0;
        }
        let step = (in_rate / self.rate as f64) * (refresh / game_fps).clamp(0.9, 1.1) / (1.0 + self.stretch);
        let frames = input.len() / 2;
        let vol = volume.min(100) as i32;
        self.out.clear();
        // linear resampler, carrying its position from one frame to the next
        while self.pos < frames as f64 {
            let i = self.pos.floor() as usize;
            let f = self.pos - i as f64;
            let (a, b) = if i == 0 {
                (self.last, [input[0], input[1]])
            } else {
                ([input[(i - 1) * 2], input[(i - 1) * 2 + 1]], [input[i * 2], input[i * 2 + 1]])
            };
            for ch in 0..2 {
                let v = a[ch] as f64 + (b[ch] as f64 - a[ch] as f64) * f;
                self.out.push(((v as i32 * vol) / 100) as i16);
            }
            self.pos += step;
        }
        self.pos -= frames as f64;
        self.last = [input[(frames - 1) * 2], input[(frames - 1) * 2 + 1]];
        unsafe {
            (sdl.QueueAudio)(self.dev, self.out.as_ptr() as *const _, (self.out.len() * 2) as u32);
        }
        if self.paused && self.queued_ms(sdl) >= target * 0.5 {
            unsafe { (sdl.PauseAudioDevice)(self.dev, 0) };
            self.paused = false;
        }
    }

    pub fn pause(&mut self, sdl: &Sdl) {
        unsafe {
            (sdl.PauseAudioDevice)(self.dev, 1);
            (sdl.ClearQueuedAudio)(self.dev);
        }
        self.paused = true;
        self.pos = 0.0;
        self.stretch = 0.0;
    }

    pub fn close(&mut self, sdl: &Sdl) {
        unsafe { (sdl.CloseAudioDevice)(self.dev) };
    }
}
