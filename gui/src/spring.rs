//! Apple-inspired spring physics, momentum projection, and rubber-banding.
//!
//! Designed around Apple's *Designing Fluid Interfaces* (WWDC 2018) parameters:
//! - `damping_ratio`: `1.0` = critically damped (no overshoot), `< 1.0` = subtle bounce.
//! - `response`: approximate settle time in seconds (lower = snappier).
//!
//! Springs preserve their live `value` and `velocity` when re-targeted mid-flight,
//! making every animation naturally interruptible without velocity discontinuities.

use std::f32::consts::TAU;

use cosmic::iced::Color;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringParams {
    pub damping_ratio: f32,
    pub response: f32,
}

impl SpringParams {
    /// Critically damped default for general UI transitions (no overshoot).
    pub const UI_DEFAULT: Self = Self {
        damping_ratio: 1.0,
        response: 0.32,
    };

    /// Snappy, slightly under-damped spring for segmented pills and switches.
    pub const SNAPPY: Self = Self {
        damping_ratio: 0.82,
        response: 0.26,
    };

    /// Fast tactile spring for press-down squish and release bounce.
    pub const TACTILE: Self = Self {
        damping_ratio: 0.78,
        response: 0.18,
    };

    /// Gentle critically damped spring for battery rings and EQ curve morphs.
    pub const GENTLE: Self = Self {
        damping_ratio: 1.0,
        response: 0.42,
    };

    #[must_use]
    pub const fn new(damping_ratio: f32, response: f32) -> Self {
        Self {
            damping_ratio,
            response,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub params: SpringParams,
}

impl Spring {
    #[must_use]
    pub fn new(initial: f32, params: SpringParams) -> Self {
        Self {
            value: initial,
            target: initial,
            velocity: 0.0,
            params,
        }
    }

    /// Re-targets the spring while preserving its live `value` and `velocity`
    /// so mid-flight interruptions are completely seamless.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Immediately snaps the spring to `value` with zero velocity.
    pub fn snap_to(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// Hands off gesture release velocity into the spring.
    pub fn set_velocity(&mut self, velocity: f32) {
        self.velocity = velocity;
    }

    #[must_use]
    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 0.0008 && self.velocity.abs() < 0.002
    }

    /// Advances the spring by `dt` seconds using sub-stepped semi-implicit Euler
    /// for unconditional stability at any refresh rate. Returns `true` if still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }

        let dt = dt.clamp(0.0, 0.05);
        if dt <= 0.0 {
            return true;
        }

        let response = self.params.response.max(0.04);
        let omega = TAU / response;
        let stiffness = omega * omega;
        let damping = 2.0 * self.params.damping_ratio.max(0.1) * omega;

        // Sub-step at <= 1/240s for rock-solid stability even with snappy springs.
        let steps = (dt / (1.0 / 240.0)).ceil().max(1.0) as u32;
        let sub_dt = dt / steps as f32;

        for _ in 0..steps {
            let displacement = self.value - self.target;
            let force = -stiffness * displacement - damping * self.velocity;
            self.velocity += force * sub_dt;
            self.value += self.velocity * sub_dt;
        }

        if self.is_settled() {
            self.value = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}

/// Apple's progressive boundary resistance (rubber-banding).
/// `overshoot` is distance past the edge; `dimension` is the track/container size.
#[must_use]
pub fn rubberband(overshoot: f32, dimension: f32, constant: f32) -> f32 {
    if dimension <= 0.0 {
        return 0.0;
    }
    (overshoot * dimension * constant) / (dimension + constant * overshoot.abs())
}

/// Apple's exponential-decay momentum projection (from WWDC 2018 *Designing Fluid Interfaces*).
/// `velocity` is in logical px/s; `deceleration_rate` is ~0.998 (normal) or ~0.99 (snappier).
#[must_use]
pub fn project_momentum(velocity: f32, deceleration_rate: f32) -> f32 {
    (velocity / 1000.0) * deceleration_rate / (1.0 - deceleration_rate)
}

/// Linearly interpolates two colors (clamping `t` to `[0.0, 1.0]`).
#[must_use]
pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critically_damped_spring_converges_without_overshoot() {
        let mut s = Spring::new(0.0, SpringParams::UI_DEFAULT);
        s.set_target(1.0);
        for _ in 0..120 {
            s.step(1.0 / 60.0);
            assert!(
                s.value <= 1.001,
                "critically damped spring overshot: {}",
                s.value
            );
        }
        assert!(s.is_settled());
        assert!((s.value - 1.0).abs() < 0.002);
    }

    #[test]
    fn retargeting_preserves_live_value_and_velocity() {
        let mut s = Spring::new(0.0, SpringParams::SNAPPY);
        s.set_target(10.0);
        for _ in 0..5 {
            s.step(1.0 / 60.0);
        }
        let live_val = s.value;
        let live_vel = s.velocity;
        assert!(live_val > 0.0);
        assert!(live_vel > 0.0);

        s.set_target(0.0);
        assert!((s.value - live_val).abs() < f32::EPSILON);
        assert!((s.velocity - live_vel).abs() < f32::EPSILON);
    }

    #[test]
    fn rubberband_resists_progressively() {
        let r1 = rubberband(10.0, 100.0, 0.55);
        let r2 = rubberband(50.0, 100.0, 0.55);
        assert!(r1 > 0.0 && r1 < 10.0);
        assert!(r2 > r1 && r2 < 50.0);
    }

    #[test]
    fn project_momentum_is_monotonic_with_velocity() {
        let p_slow = project_momentum(100.0, 0.99);
        let p_fast = project_momentum(500.0, 0.99);
        assert!(p_slow > 0.0);
        assert!(p_fast > p_slow);
    }
}
