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

/// If a tick's delta exceeds this, the gap is treated as a stall (app in the
/// background, a long main-thread hitch, a breakpoint) and emulated time
/// resynchronises instead of catching up.
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
    /// How long one core step has been taking, in milliseconds (an average the bridge feeds
    /// with [`FramePacer::note_step_cost`]); 0 until measured.
    step_cost_ms: f64,
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
            step_cost_ms: 0.0,
        }
    }

    /// How long the core steps of the last tick took, each, in milliseconds.
    pub fn note_step_cost(&mut self, ms: f64) {
        if !ms.is_finite() || ms <= 0.0 {
            return;
        }
        self.step_cost_ms = if self.step_cost_ms == 0.0 { ms } else { self.step_cost_ms * 0.8 + ms * 0.2 };
    }

    /// The most core steps one tick may run, from what a step costs.
    ///
    /// A core that cannot keep up must not be asked to catch up: each extra step makes this tick
    /// longer, so the next one owes even more, until every tick runs the full burst and the screen
    /// updates a dozen times a second (the owner's iPhone, build 230, GameCube Animal Crossing:
    /// 4 steps a tick, 11-13 pictures a second, while the core was managing 44-52 frames a second).
    /// So a tick may spend about two core frames' worth of time catching up, and at least one step
    /// always runs; whatever else is owed is let go, and the game runs a little slow instead of
    /// stuttering.
    fn catch_up_cap(&self) -> u32 {
        if self.step_cost_ms <= 0.0 {
            return MAX_CATCH_UP_STEPS;
        }
        // Two core frames of wall time (step_ms is the core frame period at normal speed).
        let budget = 2.0 * self.step_ms;
        let fits = (budget / self.step_cost_ms).floor() as u32;
        fits.clamp(1, MAX_CATCH_UP_STEPS)
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
    ///
    /// A non-finite value is ignored: NaN survives `clamp`, and once it reached the
    /// accumulator every later tick would plan zero steps, freezing the game.
    pub fn set_speed(&mut self, speed: f64) {
        if !speed.is_finite() {
            return;
        }
        self.speed = speed.clamp(0.05, 16.0);
    }

    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// Drops accumulated debt and re-anchors to `now_ms`. Call on resume, load
    /// state, or when the app returns to the foreground. A non-finite `now_ms` only
    /// drops the debt and leaves the anchor alone.
    pub fn resync(&mut self, now_ms: f64) {
        self.accumulator_ms = 0.0;
        if !now_ms.is_finite() {
            return;
        }
        self.last_timestamp_ms = Some(now_ms);
    }

    pub fn plan(&mut self, now_ms: f64) -> TickPlan {
        // A non-finite timestamp would become the anchor, and every delta measured from a NaN
        // anchor is NaN, which `max(0.0)` turns into 0: no step would ever run again. Such a tick
        // owes nothing and the anchor stays where it was.
        if !now_ms.is_finite() {
            return TickPlan {
                steps: 0,
                dropped: 0,
                resynced: false,
            };
        }
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
        let cap = self.catch_up_cap();
        if steps > cap {
            dropped = steps - cap;
            steps = cap;
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
        let plan = p.plan(30_000.0); // app in the background for 30s
        assert!(plan.resynced);
        assert_eq!(plan.steps, 1);
        assert_eq!(p.stalls(), 1);
    }

    #[test]
    fn non_finite_inputs_cannot_freeze_the_pacer() {
        let mut p = FramePacer::new(60.0);
        p.plan(0.0);
        p.set_speed(f64::NAN);
        assert_eq!(p.speed(), 1.0);
        assert_eq!(p.plan(f64::NAN).steps, 0);
        p.resync(f64::INFINITY);
        // Still anchored at 0, so a normal tick after it runs normally.
        assert_eq!(p.plan(16.7).steps, 1);
        assert_eq!(p.plan(33.4).steps, 1);
    }

    #[test]
    fn moderate_hitch_is_capped() {
        let mut p = FramePacer::new(60.0);
        p.plan(0.0);
        let plan = p.plan(200.0); // ~12 frames of debt
        assert_eq!(plan.steps, MAX_CATCH_UP_STEPS);
        assert!(plan.dropped > 0);
    }

    #[test]
    fn a_core_slower_than_the_screen_runs_one_step_a_tick_instead_of_spiralling() {
        // 60 fps core, 120 Hz screen, each step takes 20 ms: a tick takes 20 ms, so ticks come
        // 20-25 ms apart and each owes more than one step. Without the cap they owed 4.
        let mut p = FramePacer::new(60.0);
        p.plan(0.0);
        p.note_step_cost(20.0);
        let mut t = 0.0;
        for _ in 0..50 {
            t += 22.0;
            let plan = p.plan(t);
            assert_eq!(plan.steps, 1, "a slow core gets one step a tick");
            p.note_step_cost(20.0);
        }
    }

    #[test]
    fn a_fast_core_still_catches_up_after_a_hitch() {
        let mut p = FramePacer::new(60.0);
        p.plan(0.0);
        p.note_step_cost(2.0);
        // One 50 ms hitch: three steps owed, all run.
        assert_eq!(p.plan(50.0).steps, 3);
    }

    #[test]
    fn fast_forward_is_not_capped_by_a_cheap_core() {
        let mut p = FramePacer::new(60.0);
        p.set_speed(4.0);
        p.plan(0.0);
        p.note_step_cost(1.0);
        let mut steps = 0;
        let mut t = 0.0;
        for _ in 0..60 {
            t += 1000.0 / 60.0;
            steps += p.plan(t).steps;
        }
        assert!(steps >= 230, "4x on a 60 Hz screen ran {steps} steps");
    }

}
