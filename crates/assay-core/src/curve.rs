//! Curves as data, not code (ADR-004).
//!
//! Attribute→derived mappings (Strength → Physical Power Bonus, Armor Rating
//! → PDR) are themselves balance-patched, so they live in the dataset as
//! point sets with a defined interpolation. A hardcoded curve would mean a
//! code change every time Ironmace adjusts scaling.
//!
//! Interpolation runs in fixed-point with exactly one rounding per sample
//! (ADR-001: interpolating in fixed-point requires a documented rounding
//! rule): `y = y0 + (y1 − y0) · (x − x0) / (x1 − x0)`, where the
//! multiply-divide is one fused [`Fixed::mul_div_half_even`]. Outside the
//! point range the curve clamps to its end values — extrapolation is a
//! guess, and guesses are ADR-007's department, not silent arithmetic's.

use alloc::vec::Vec;
use core::fmt;

use crate::fixed::Fixed;

/// How values between curve points are computed.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Interpolation {
    /// Straight line between neighbouring points, one rounding per sample.
    Linear,
}

/// A sampled curve: `(input, output)` points, strictly ascending in input.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Curve {
    points: Vec<(Fixed, Fixed)>,
    interpolation: Interpolation,
}

/// Why a curve definition was rejected.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CurveError {
    /// A curve needs at least one point.
    Empty,
    /// Inputs must be strictly ascending — equal inputs would make the
    /// sampled value depend on point order.
    NotStrictlyAscending,
}

impl fmt::Display for CurveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CurveError::Empty => write!(f, "curve has no points"),
            CurveError::NotStrictlyAscending => {
                write!(f, "curve inputs must be strictly ascending")
            }
        }
    }
}

impl Curve {
    /// Builds a linear curve, validating the point set.
    pub fn linear(points: Vec<(Fixed, Fixed)>) -> Result<Curve, CurveError> {
        if points.is_empty() {
            return Err(CurveError::Empty);
        }
        if points.windows(2).any(|w| w[1].0 <= w[0].0) {
            return Err(CurveError::NotStrictlyAscending);
        }
        Ok(Curve {
            points,
            interpolation: Interpolation::Linear,
        })
    }

    /// Samples the curve at `x`: clamped outside the range, interpolated
    /// between neighbours inside it, exact at the points themselves.
    #[must_use]
    pub fn sample(&self, x: Fixed) -> Fixed {
        let first = self.points[0];
        if x <= first.0 {
            return first.1;
        }
        let last = self.points[self.points.len() - 1];
        if x >= last.0 {
            return last.1;
        }
        // Index of the first point with input > x; x lies strictly between
        // points, so both neighbours exist.
        let hi = self.points.partition_point(|p| p.0 <= x);
        let (x0, y0) = self.points[hi - 1];
        let (x1, y1) = self.points[hi];
        if x == x0 {
            return y0;
        }
        match self.interpolation {
            Interpolation::Linear => y0 + (y1 - y0).mul_div_half_even(x - x0, x1 - x0),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::vec;

    use proptest::prelude::*;

    use super::*;

    fn f(units: i64) -> Fixed {
        Fixed::from_int(units)
    }

    #[test]
    fn rejects_empty_and_unsorted() {
        assert_eq!(Curve::linear(vec![]), Err(CurveError::Empty));
        assert_eq!(
            Curve::linear(vec![(f(0), f(0)), (f(0), f(1))]),
            Err(CurveError::NotStrictlyAscending)
        );
        assert_eq!(
            Curve::linear(vec![(f(5), f(0)), (f(1), f(1))]),
            Err(CurveError::NotStrictlyAscending)
        );
    }

    #[test]
    fn exact_at_points_clamped_outside() {
        let curve = Curve::linear(vec![(f(0), f(-22)), (f(100), f(40))]).unwrap();
        assert_eq!(curve.sample(f(0)), f(-22));
        assert_eq!(curve.sample(f(100)), f(40));
        assert_eq!(curve.sample(f(-50)), f(-22), "clamp below");
        assert_eq!(curve.sample(f(500)), f(40), "clamp above");
    }

    #[test]
    fn linear_interpolation_with_one_rounding() {
        // Between (0, 0) and (3, 1): at x = 1, y = 1/3 → 0.333333 (half-even).
        let curve = Curve::linear(vec![(f(0), f(0)), (f(3), f(1))]).unwrap();
        assert_eq!(curve.sample(f(1)).micro(), 333_333);
        assert_eq!(curve.sample(f(2)).micro(), 666_667);
    }

    #[test]
    fn segment_lookup_picks_the_right_neighbours() {
        let curve = Curve::linear(vec![(f(0), f(0)), (f(10), f(100)), (f(20), f(0))]).unwrap();
        assert_eq!(curve.sample(f(5)), f(50));
        assert_eq!(curve.sample(f(10)), f(100));
        assert_eq!(curve.sample(f(15)), f(50));
    }

    proptest! {
        #[test]
        fn monotonic_points_give_monotonic_samples(
            xs in proptest::collection::btree_set(-1_000i64..1_000, 2..8),
            start in -1_000i64..1_000,
            steps in proptest::collection::vec(0i64..500, 7),
            probe_a in -1_200i64..1_200,
            probe_b in -1_200i64..1_200,
        ) {
            // Build a monotonically non-decreasing curve from sorted inputs.
            let xs: alloc::vec::Vec<i64> = xs.into_iter().collect();
            let mut y = start;
            let points: alloc::vec::Vec<(Fixed, Fixed)> = xs
                .iter()
                .enumerate()
                .map(|(i, &x)| {
                    if i > 0 {
                        y += steps[(i - 1) % steps.len()];
                    }
                    (Fixed::from_int(x), Fixed::from_int(y))
                })
                .collect();
            let curve = Curve::linear(points).unwrap();
            let (lo, hi) = if probe_a <= probe_b { (probe_a, probe_b) } else { (probe_b, probe_a) };
            // Monotonicity survives sampling — the property behind the
            // "more Armor Rating never lowers PDR" invariant (ADR-010).
            prop_assert!(curve.sample(Fixed::from_int(lo)) <= curve.sample(Fixed::from_int(hi)));
        }
    }
}
