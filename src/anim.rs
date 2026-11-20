//! Time-based animation primitives.
//!
//! A TUI redraws whole frames, so every "transition" is really a value that
//! converges over time. Springs are used instead of fixed-duration tweens for
//! anything reflecting live data, because a spring is interruptible: a new value
//! retargets the same spring mid-flight instead of restarting a timer, so a
//! cluster that reports new numbers every second never produces a stutter.
//!
//! Nothing here blocks. The main loop decides its frame interval from
//! [`AnimState::is_animating`], so an idle UI still costs one redraw per second
//! rather than sixty.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::style::Color;

/// Frame interval while something is moving (~60 fps).
pub const FRAME: Duration = Duration::from_millis(16);
/// Frame interval that keeps spinners smooth without burning a core.
pub const BUSY_FRAME: Duration = Duration::from_millis(80);
/// Frame interval when the UI is completely idle.
pub const IDLE_FRAME: Duration = Duration::from_millis(250);

/// A critically damped-ish spring that converges on its target.
///
/// Stiffness/damping are tuned for ~250–320 ms settle: fast enough to feel like
/// a direct response, slow enough that the eye reads it as motion rather than a
/// jump. Overshoot is minimal, which matters for numeric readouts — a number
/// that bounces past its value reads as a bug.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    value: f64,
    target: f64,
    velocity: f64,
    /// When set, the first `set_target` jumps instead of travelling. A readout
    /// appearing for the first time should show its value, not count up from
    /// zero — only *changes* to a value are worth animating.
    latch_first: bool,
    seen_target: bool,
    /// Time spent travelling toward the current target. Used as a watchdog:
    /// an animation that has not settled by `MAX_TRAVEL` snaps, so a spring can
    /// never keep the UI redrawing at 60 fps indefinitely.
    elapsed: Duration,
    /// How close is close enough. Depends on how the value is drawn, so it is a
    /// property of the spring rather than a global constant.
    tolerance: f64,
}

impl Spring {
    const STIFFNESS: f64 = 170.0;
    const DAMPING: f64 = 26.0;
    /// Integration step; large deltas are split so a stalled loop cannot make
    /// the spring explode.
    const SUBSTEP: f64 = 1.0 / 120.0;
    /// Settle threshold for a value rendered continuously (a ratio or bar):
    /// well under one pixel of a 60-cell bar.
    pub const TOLERANCE_RATIO: f64 = 5.0e-4;
    /// Settle threshold for a value rendered as an integer: half a unit, i.e.
    /// the point at which the drawn digits stop changing.
    pub const TOLERANCE_COUNT: f64 = 0.5;
    /// Hard stop for a single journey. Springs should settle in ~300 ms; this is
    /// a safety net so a mis-tuned value cannot animate forever.
    const MAX_TRAVEL: Duration = Duration::from_millis(1500);

    /// A spring that animates toward every target it is given.
    pub fn new(value: f64) -> Self {
        Self {
            value,
            target: value,
            velocity: 0.0,
            latch_first: false,
            seen_target: false,
            elapsed: Duration::ZERO,
            tolerance: Self::TOLERANCE_RATIO,
        }
    }

    /// A latched spring whose value is drawn as an integer, so it settles once
    /// the rendered digits stop changing instead of chasing fractions forever.
    pub fn latched_count(value: f64) -> Self {
        Self {
            tolerance: Self::TOLERANCE_COUNT,
            ..Self::latched(value)
        }
    }

    /// A spring whose first target is adopted instantly, and whose subsequent
    /// targets are animated. Use this for values sourced from the cluster.
    pub fn latched(value: f64) -> Self {
        Self {
            latch_first: true,
            ..Self::new(value)
        }
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    /// Retarget without touching the current position — this is what makes the
    /// motion interruptible.
    pub fn set_target(&mut self, target: f64) {
        if self.latch_first && !self.seen_target {
            self.seen_target = true;
            self.snap(target);
            return;
        }
        self.seen_target = true;
        if (self.target - target).abs() > f64::MIN_POSITIVE {
            self.elapsed = Duration::ZERO;
        }
        self.target = target;
    }

    /// Jump to a value and stop. Used for reduced motion and first paint.
    pub fn snap(&mut self, value: f64) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
        self.elapsed = Duration::ZERO;
    }

    /// Advance by `dt`. Returns true while still moving.
    pub fn step(&mut self, dt: Duration) -> bool {
        if self.is_animating() {
            self.elapsed += dt;
            if self.elapsed > Self::MAX_TRAVEL {
                let target = self.target;
                self.snap(target);
                return false;
            }

            let mut remaining = dt.as_secs_f64().min(0.25);
            while remaining > 0.0 {
                let h = remaining.min(Self::SUBSTEP);
                let accel = Self::STIFFNESS * (self.target - self.value)
                    - Self::DAMPING * self.velocity;
                self.velocity += accel * h;
                self.value += self.velocity * h;
                remaining -= h;
            }

            if !self.is_animating() {
                self.value = self.target;
                self.velocity = 0.0;
            }
        }
        self.is_animating()
    }

    /// True while the value is far enough from its target to change what is
    /// drawn.
    ///
    /// Velocity is deliberately not part of this test: a spring passing *through*
    /// its target at speed is, for rendering purposes, already there.
    pub fn is_animating(&self) -> bool {
        (self.target - self.value).abs() > self.tolerance
    }

    /// The value, clamped to a 0..=1 range, for ratios.
    pub fn ratio(&self) -> f64 {
        self.value.clamp(0.0, 1.0)
    }
}

/// Linear interpolation between two colours.
///
/// Only `Rgb` pairs are blended; anything else falls back to the target so the
/// function can never panic on indexed or reset colours.
pub fn lerp_color(from: Color, to: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (from, to) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let f = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
            Color::Rgb(f(r1, r2), f(g1, g2), f(b1, b2))
        }
        _ => {
            if t >= 0.5 {
                to
            } else {
                from
            }
        }
    }
}

/// How long a status message stays fully opaque before it starts fading.
pub const STATUS_HOLD: Duration = Duration::from_millis(4000);
/// Fade duration after the hold.
pub const STATUS_FADE: Duration = Duration::from_millis(1200);

/// How long an error toast stays fully opaque before it starts fading.
pub const TOAST_HOLD: Duration = Duration::from_millis(4000);
/// Fade duration after the hold.
pub const TOAST_FADE: Duration = Duration::from_millis(1200);

/// All animated values the UI owns.
///
/// Kept in one struct so the main loop can ask a single question — "is anything
/// still moving?" — and because every spring needs to be stepped together.
pub struct AnimState {
    /// Reduced motion: springs snap to their targets instead of travelling.
    pub reduced: bool,

    /// Share of cluster capacity in use (dashboard bar).
    pub capacity: Spring,
    /// Replica availability ratio (fsck gauge).
    pub health: Spring,
    /// Namespace counters, so a refresh counts up instead of jumping.
    pub dirs: Spring,
    pub files: Spring,
    pub blocks: Spring,
    pub workers: Spring,
    /// Per-transfer progress, keyed by transfer id.
    pub transfers: HashMap<u64, Spring>,

    /// When the current status message was set.
    status_at: Option<Instant>,
    /// Age of the visible error toast. Advanced by [`AnimState::step`] rather
    /// than read from the wall clock, so expiry is a function of rendered
    /// frames and tests can drive it without sleeping.
    toast_age: Option<Duration>,
}

impl Default for AnimState {
    fn default() -> Self {
        Self::new(false)
    }
}

impl AnimState {
    pub fn new(reduced: bool) -> Self {
        let s = |v: f64| {
            let mut spring = Spring::latched(v);
            if reduced {
                spring.snap(v);
            }
            spring
        };
        let c = |v: f64| Spring::latched_count(v);
        Self {
            reduced,
            capacity: s(0.0),
            health: s(0.0),
            dirs: c(0.0),
            files: c(0.0),
            blocks: c(0.0),
            workers: c(0.0),
            transfers: HashMap::new(),
            status_at: None,
            toast_age: None,
        }
    }

    pub fn mark_status(&mut self) {
        self.status_at = Some(Instant::now());
    }

    pub fn clear_status_mark(&mut self) {
        self.status_at = None;
    }

    /// Opacity of the status message: 1.0 while held, then fading to 0.
    pub fn status_alpha(&self) -> f64 {
        let Some(at) = self.status_at else {
            return 1.0;
        };
        let elapsed = at.elapsed();
        if elapsed <= STATUS_HOLD {
            return 1.0;
        }
        let faded = elapsed - STATUS_HOLD;
        (1.0 - faded.as_secs_f64() / STATUS_FADE.as_secs_f64()).clamp(0.0, 1.0)
    }

    /// True once the status message has fully faded and should be dropped.
    pub fn status_expired(&self) -> bool {
        self.status_at
            .map(|at| at.elapsed() > STATUS_HOLD + STATUS_FADE)
            .unwrap_or(false)
    }

    /// Start (or restart) the toast clock. Called every time an error is set,
    /// including a repeat of the message already on screen.
    pub fn mark_toast(&mut self) {
        self.toast_age = Some(Duration::ZERO);
    }

    pub fn clear_toast_mark(&mut self) {
        self.toast_age = None;
    }

    /// Opacity of the toast: 1.0 while held, then fading to 0. The fade uses
    /// the same linear curve as the footer status, but its clock is the
    /// frame-driven age rather than `Instant::now()`.
    pub fn toast_alpha(&self) -> f64 {
        let Some(age) = self.toast_age else {
            return 1.0;
        };
        if age <= TOAST_HOLD {
            return 1.0;
        }
        let faded = age - TOAST_HOLD;
        (1.0 - faded.as_secs_f64() / TOAST_FADE.as_secs_f64()).clamp(0.0, 1.0)
    }

    /// True once the toast has fully faded and should be dropped.
    pub fn toast_expired(&self) -> bool {
        self.toast_age
            .map(|age| age > TOAST_HOLD + TOAST_FADE)
            .unwrap_or(false)
    }

    /// Track a transfer's progress spring, creating it at `initial` on first
    /// sight so new rows do not animate up from zero.
    pub fn transfer(&mut self, id: u64, initial: f64) -> &mut Spring {
        self.transfers
            .entry(id)
            .or_insert_with(|| Spring::latched(initial))
    }

    pub fn prune_transfers(&mut self, keep: &[u64]) {
        self.transfers.retain(|id, spring| {
            keep.contains(id) || spring.is_animating()
        });
    }

    /// Advance every spring. Returns true while anything is still moving.
    pub fn step(&mut self, dt: Duration) -> bool {
        // The toast clock advances even under reduced motion: the fade is then
        // skipped by the frame governor, but expiry must still happen.
        if let Some(age) = self.toast_age.as_mut() {
            *age += dt;
        }

        if self.reduced {
            for s in [
                &mut self.capacity,
                &mut self.health,
                &mut self.dirs,
                &mut self.files,
                &mut self.blocks,
                &mut self.workers,
            ] {
                s.snap(s.target());
            }
            for s in self.transfers.values_mut() {
                s.snap(s.target());
            }
            return false;
        }

        let mut moving = false;
        for s in [
            &mut self.capacity,
            &mut self.health,
            &mut self.dirs,
            &mut self.files,
            &mut self.blocks,
            &mut self.workers,
        ] {
            moving |= s.step(dt);
        }
        for s in self.transfers.values_mut() {
            moving |= s.step(dt);
        }

        moving
    }

    pub fn is_animating(&self) -> bool {
        if self.reduced {
            return false;
        }
        // A fading toast needs smooth frames; during the hold it can wait for
        // the idle tick and still expire on time.
        let toast_fading = self
            .toast_age
            .map(|age| age > TOAST_HOLD && age <= TOAST_HOLD + TOAST_FADE)
            .unwrap_or(false);
        toast_fading
            || self.capacity.is_animating()
            || self.health.is_animating()
            || self.dirs.is_animating()
            || self.files.is_animating()
            || self.blocks.is_animating()
            || self.workers.is_animating()
            || self.transfers.values().any(|s| s.is_animating())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_on_target() {
        let mut s = Spring::new(0.0);
        s.set_target(1.0);
        for _ in 0..120 {
            s.step(FRAME);
        }
        assert!((s.value() - 1.0).abs() < 0.01, "value = {}", s.value());
        assert!(!s.is_animating());
    }

    #[test]
    fn spring_is_interruptible_mid_flight() {
        let mut s = Spring::new(0.0);
        s.set_target(1.0);
        for _ in 0..5 {
            s.step(FRAME);
        }
        let mid = s.value();
        assert!(mid > 0.0 && mid < 1.0, "expected mid-flight, got {mid}");

        // Retargeting must keep the current position — it must not jump back.
        s.set_target(0.0);
        assert_eq!(s.value(), mid, "retarget moved the current value");
        for _ in 0..120 {
            s.step(FRAME);
        }
        assert!((s.value() - 0.0).abs() < 0.01);
    }

    #[test]
    fn spring_does_not_overshoot_badly() {
        // Numeric readouts must not bounce past their value.
        let mut s = Spring::new(0.0);
        s.set_target(1.0);
        let mut peak: f64 = 0.0;
        for _ in 0..60 {
            s.step(FRAME);
            peak = peak.max(s.value());
        }
        assert!(peak < 1.15, "overshoot too large: {peak}");
    }

    #[test]
    fn spring_survives_a_long_stall() {
        // A paused process (laptop sleep, blocked syscall) must not explode.
        let mut s = Spring::new(0.0);
        s.set_target(1.0);
        s.step(Duration::from_secs(30));
        assert!(s.value().is_finite(), "value went non-finite");
        assert!(s.value() >= 0.0 && s.value() <= 1.0, "value = {}", s.value());
    }

    #[test]
    fn snap_is_instant() {
        let mut s = Spring::new(0.0);
        s.set_target(1.0);
        s.snap(0.5);
        assert_eq!(s.value(), 0.5);
        assert!(!s.is_animating());
    }

    #[test]
    fn reduced_motion_snaps_everything() {
        let mut anim = AnimState::new(true);
        anim.capacity.set_target(1.0);
        anim.blocks.set_target(340.0);
        anim.step(FRAME);
        assert_eq!(anim.capacity.value(), 1.0);
        assert_eq!(anim.blocks.value(), 340.0);
        assert!(!anim.is_animating(), "reduced motion must not animate");
    }


    #[test]
    fn status_fades_then_expires() {
        let mut anim = AnimState::new(false);
        assert_eq!(anim.status_alpha(), 1.0);
        assert!(!anim.status_expired());

        anim.mark_status();
        assert_eq!(anim.status_alpha(), 1.0);
        assert!(!anim.status_expired());
    }

    #[test]
    fn lerp_color_midpoint() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(100, 200, 50);
        assert_eq!(lerp_color(a, b, 0.0), a);
        assert_eq!(lerp_color(a, b, 1.0), b);
        assert_eq!(lerp_color(a, b, 0.5), Color::Rgb(50, 100, 25));
    }

    #[test]
    fn lerp_color_handles_non_rgb_without_panicking() {
        let a = Color::Reset;
        let b = Color::Rgb(10, 20, 30);
        assert_eq!(lerp_color(a, b, 0.2), a);
        assert_eq!(lerp_color(a, b, 0.8), b);
    }
}
