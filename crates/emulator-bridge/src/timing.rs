//! Frame pacing.
//!
//! The display ticks at 60/120/144 Hz; cores run at 59.94 (NES), 50 (PAL), 59.73
//! (Game Boy) and so on. The pacer decouples the two: given a wall-clock
//! timestamp it answers a single question — *how many core steps does this tick
//! owe?* — and refuses to owe more than a small burst so a long stall cannot
//! trigger a multi-second fast-forward.

/// Hard ceiling on catch-up steps in one tick. Beyond this, emulated time is
/// deliberately abandoned rather than blocking the main thread.
const MAX_CATCH_UP_STEPS: u32 = 4;

/// If a tick's delta exceeds this, the gap is treated as a stall (tab was hidden,
/// GC pause, breakpoint) and emulated time resynchronises instead of catching up.
const STALL_THRESHOLD_MS: f64 = 500.0;

#[derive(Debug)]
pub struct FramePacer {
    target_fps: f64,
    step_ms: f64,
    /// Emulated-time debt in milliseconds.
    accumulator_ms: f64,
    last_timestamp_ms: Option<f64>,
    speed: f64,
    // --- statistics, surfaced to the debug HUD ---
    fps_ema: f64,
    steps_total: u64,
    steps_dropped: u64,
    stalls: u64,
}

/// What a single tick should do. Returned by [`FramePacer::plan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickPlan {
    /// Core steps to run this tick. `0` means the display is ahead of the core
    /// (e.g. 144 Hz display, 60 Hz core) — present the previous frame again.
    pub steps: u32,
    /// Steps discarded because the catch-up ceiling was hit.
    pub dropped: u32,
    /// A stall was detected and emulated time was resynchronised.
    pub resynced: bool,
}

impl FramePacer {
    pub fn new(target_fps: f64) -> Self {
        let target_fps = if target_fps.is_finite() && target_fps > 1.0 {
            target_fps
        } else {
            60.0
        };
        Self {
            target_fps,
            step_ms: 1000.0 / target_fps,
            accumulator_ms: 0.0,
            last_timestamp_ms: None,
            speed: 1.0,
            fps_ema: 0.0,
            steps_total: 0,
            steps_dropped: 0,
            stalls: 0,
        }
    }

    pub fn set_target_fps(&mut self, fps: f64) {
        if fps.is_finite() && fps > 1.0 {
            self.target_fps = fps;
            self.step_ms = 1000.0 / fps;
            self.accumulator_ms = 0.0;
        }
    }

    pub fn target_fps(&self) -> f64 {
        self.target_fps
    }

    /// Fast-forward / slow-motion multiplier. `2.0` runs the core twice as fast.
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.05, 16.0);
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// Drops accumulated debt and re-anchors to `now_ms`. Call on resume, load
    /// state, or when the tab becomes visible again.
    pub fn resync(&mut self, now_ms: f64) {
        self.accumulator_ms = 0.0;
        self.last_timestamp_ms = Some(now_ms);
    }

    pub fn plan(&mut self, now_ms: f64) -> TickPlan {
        let last = match self.last_timestamp_ms {
            Some(t) => t,
            None => {
                // First tick: run exactly one step so something is on screen.
                self.last_timestamp_ms = Some(now_ms);
                self.steps_total += 1;
                return TickPlan {
                    steps: 1,
                    dropped: 0,
                    resynced: false,
                };
            }
        };

        let delta_ms = (now_ms - last).max(0.0);
        self.last_timestamp_ms = Some(now_ms);

        if delta_ms > 0.0 {
            let instant_fps = 1000.0 / delta_ms;
            self.fps_ema = if self.fps_ema == 0.0 {
                instant_fps
            } else {
                self.fps_ema * 0.9 + instant_fps * 0.1
            };
        }

        if delta_ms > STALL_THRESHOLD_MS {
            self.stalls += 1;
            self.accumulator_ms = 0.0;
            self.steps_total += 1;
            return TickPlan {
                steps: 1,
                dropped: 0,
                resynced: true,
            };
        }

        self.accumulator_ms += delta_ms * self.speed;

        let mut steps = (self.accumulator_ms / self.step_ms).floor() as u32;
        self.accumulator_ms -= steps as f64 * self.step_ms;

        let mut dropped = 0;
        if steps > MAX_CATCH_UP_STEPS {
            dropped = steps - MAX_CATCH_UP_STEPS;
            steps = MAX_CATCH_UP_STEPS;
            // Debt already subtracted above, so dropped steps are simply forfeited.
            self.steps_dropped += dropped as u64;
        }

        self.steps_total += steps as u64;
        TickPlan {
            steps,
            dropped,
            resynced: false,
        }
    }

    pub fn display_fps(&self) -> f64 {
        self.fps_ema
    }

    pub fn steps_total(&self) -> u64 {
        self.steps_total
    }

    pub fn steps_dropped(&self) -> u64 {
        self.steps_dropped
    }

    pub fn stalls(&self) -> u64 {
        self.stalls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_tick_runs_one_step() {
        let mut p = FramePacer::new(60.0);
        assert_eq!(p.plan(0.0).steps, 1);
    }

    #[test]
    fn matched_refresh_runs_one_step_per_tick() {
        let mut p = FramePacer::new(60.0);
        p.plan(0.0);
        let mut t = 0.0;
        for _ in 0..10 {
            t += 16.667;
            assert_eq!(p.plan(t).steps, 1);
        }
    }

    #[test]
    fn fast_display_skips_steps() {
        // 144 Hz display, 60 Hz core: most ticks must not step the core.
        let mut p = FramePacer::new(60.0);
        p.plan(0.0);
        let mut t = 0.0;
        let mut steps = 0;
        for _ in 0..144 {
            t += 1000.0 / 144.0;
            steps += p.plan(t).steps;
        }
        assert!((59..=61).contains(&steps), "stepped {steps} times");
    }

    #[test]
    fn long_stall_resyncs_instead_of_fast_forwarding() {
        let mut p = FramePacer::new(60.0);
        p.plan(0.0);
        let plan = p.plan(30_000.0); // tab hidden for 30s
        assert!(plan.resynced);
        assert_eq!(plan.steps, 1);
        assert_eq!(p.stalls(), 1);
    }

    #[test]
    fn moderate_hitch_is_capped() {
        let mut p = FramePacer::new(60.0);
        p.plan(0.0);
        let plan = p.plan(200.0); // ~12 frames of debt
        assert_eq!(plan.steps, MAX_CATCH_UP_STEPS);
        assert!(plan.dropped > 0);
    }
}
