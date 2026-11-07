//! Resolved fade curves on the overlay wire. Names are resolved by the shell; the compositor
//! gets a bounded piecewise cubic animation once, with no engine frames or timers to sample it.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FadeCurve {
    kind: u32,
    points: [f32; 4],
}

impl FadeCurve {
    /// The original cubic ease-out, preserved exactly for clients without a curve.
    pub const DEFAULT: Self = Self {
        kind: 0,
        points: [0.0; 4],
    };

    pub const fn bezier(points: [f32; 4]) -> Self {
        Self { kind: 1, points }
    }

    /// Physical spring, evaluated over `seconds` of its natural time (named springs may be
    /// fitted to a different presentation duration by the shell).
    pub const fn spring(mass: f32, stiffness: f32, damping: f32, seconds: f32) -> Self {
        Self {
            kind: 2,
            points: [mass, stiffness, damping, seconds],
        }
    }

    pub fn valid(self) -> bool {
        let [a, b, c, d] = self.points;
        match self.kind {
            0 => self == Self::DEFAULT,
            1 => {
                (0.0..=1.0).contains(&a)
                    && (-2.0..=3.0).contains(&b)
                    && (0.0..=1.0).contains(&c)
                    && (-2.0..=3.0).contains(&d)
            }
            2 => {
                (0.1..=10.0).contains(&a)
                    && (1.0..=2000.0).contains(&b)
                    && (0.0..=200.0).contains(&c)
                    && (0.0..=5.0).contains(&d)
            }
            _ => false,
        }
    }

    /// Normalized opacity, clamped just as a compositor opacity is. Used only while building
    /// the animation at open; never evaluated by the engine's frame loop.
    pub fn at(self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let [a, b, c, d] = self.points;
        let value = match self.kind {
            0 => 1.0 - (1.0 - x).powi(3),
            1 => {
                let cubic = |a: f32, b: f32, t: f32| {
                    3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t
                };
                let (mut low, mut high) = (0.0, 1.0);
                for _ in 0..24 {
                    let t = (low + high) / 2.0;
                    if cubic(a, c, t) < x {
                        low = t;
                    } else {
                        high = t;
                    }
                }
                cubic(b, d, (low + high) / 2.0)
            }
            2 => {
                // The same analytic damped spring as the kit's transition::Easing::Spring.
                let (m, k, c, t) = (f64::from(a), f64::from(b), f64::from(c), f64::from(x * d));
                let w = (k / m).sqrt();
                let z = c / (2.0 * (k * m).sqrt());
                let value = if z < 1.0 {
                    let wd = w * (1.0 - z * z).sqrt();
                    1.0 - (-z * w * t).exp() * ((wd * t).cos() + z * w / wd * (wd * t).sin())
                } else if z == 1.0 {
                    1.0 - (-w * t).exp() * (1.0 + w * t)
                } else {
                    let q = (z * z - 1.0).sqrt();
                    let (r1, r2) = (-w * (z - q), -w * (z + q));
                    1.0 - (r2 * (r1 * t).exp() - r1 * (r2 * t).exp()) / (r2 - r1)
                };
                value as f32
            }
            _ => 1.0,
        };
        value.clamp(0.0, 1.0)
    }

    /// Linear pieces for DirectComposition's AddCubic. Quarter/midpoint refinement limits
    /// opacity error to under one alpha step, including extreme BÃ©ziers and fast springs.
    pub fn segments(self) -> Vec<[f32; 4]> {
        fn split(curve: FadeCurve, piece: [f32; 4], depth: u8, out: &mut Vec<[f32; 4]>) {
            let [x0, y0, x1, y1] = piece;
            let error = [0.25, 0.5, 0.75]
                .into_iter()
                .map(|p| (curve.at(x0 + (x1 - x0) * p) - (y0 + (y1 - y0) * p)).abs())
                .fold(0.0_f32, f32::max);
            if error > 1.0 / 2048.0 && depth < 24 && (x0 + x1) / 2.0 > x0 && (x0 + x1) / 2.0 < x1 {
                let x = (x0 + x1) / 2.0;
                let y = curve.at(x);
                split(curve, [x0, y0, x, y], depth + 1, out);
                split(curve, [x, y, x1, y1], depth + 1, out);
            } else {
                out.push(piece);
            }
        }
        let mut pieces = Vec::new();
        // 256 starting intervals cover even the fastest permitted physical spring without
        // aliasing its oscillations. Ordinary BÃ©ziers need just 16 before adaptive splitting.
        let count = if self.kind == 2 { 256 } else { 16 };
        for i in 0..count {
            let (x0, x1) = (i as f32 / count as f32, (i + 1) as f32 / count as f32);
            split(self, [x0, self.at(x0), x1, self.at(x1)], 0, &mut pieces);
        }
        pieces
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compositor_pieces_follow_beziers_and_springs_within_one_alpha_step() {
        for curve in [
            FadeCurve::bezier([0.0, 0.0, 1.0, 1.0]),
            FadeCurve::bezier([0.42, 0.0, 0.58, 1.0]),
            FadeCurve::bezier([0.2, 1.4, 0.4, 1.0]),
            FadeCurve::bezier([0.0, 3.0, 0.0, -2.0]),
            FadeCurve::bezier([1.0, 3.0, 1.0, -2.0]),
            FadeCurve::spring(1.0, 220.0, 22.0, 0.8),
            FadeCurve::spring(1.0, 260.0, 13.0, 1.2),
            FadeCurve::spring(0.1, 2000.0, 0.0, 5.0),
        ] {
            assert!(curve.valid());
            let pieces = curve.segments();
            assert_eq!(pieces.first().unwrap()[..2], [0.0, 0.0]);
            assert_eq!(pieces.last().unwrap()[2..], [1.0, 1.0]);
            let mut piece = 0;
            for i in 0..=10000 {
                let x = i as f32 / 10000.0;
                while piece + 1 < pieces.len() && x > pieces[piece][2] {
                    piece += 1;
                }
                let [x0, y0, x1, y1] = pieces[piece];
                let value = y0 + (y1 - y0) * ((x - x0) / (x1 - x0));
                assert!(
                    (value - curve.at(x)).abs() < 1.0 / 255.0,
                    "{curve:?} at {x}"
                );
            }
            for x in (2..=7).flat_map(|p| {
                let x = 10.0_f32.powi(-p);
                [x, 1.0 - x]
            }) {
                let &[x0, y0, x1, y1] = pieces.iter().find(|p| x <= p[2]).unwrap();
                let value = y0 + (y1 - y0) * ((x - x0) / (x1 - x0));
                assert!(
                    (value - curve.at(x)).abs() < 1.0 / 255.0,
                    "endpoint {curve:?} at {x}"
                );
            }
        }
    }
}
