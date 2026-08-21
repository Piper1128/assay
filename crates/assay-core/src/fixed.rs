//! Fixed-point arithmetic (ADR-001 rev 2).
//!
//! Internal representation is `i64` in micro-units (scale 1e-6). Addition and
//! subtraction are exact. Multiplication rounds with banker's rounding
//! (round half to even) at exactly one documented point: after the full
//! product is formed in `i128`. Division exists only as named functions that
//! state their rounding rule. Floats do not exist in this crate — the clippy
//! deny-list enforces it workspace-wide.
//!
//! Why micro and not milli: the game's numbers need four decimals — Rogue's
//! base Action Speed is 7.8125% (a binary fraction, 7 + 13/16). Milli-units
//! would truncate it; micro-units carry it exactly as `7_812_500`.

use core::fmt;
use core::str::FromStr;

/// Number of micro-units per whole unit.
pub const SCALE: i64 = 1_000_000;

/// Fixed-point number, scale 1e-6. `7.8125` is stored as `7_812_500`.
///
/// Plain `+`/`-` are exact and panic on `i64` overflow (a bug, not a data
/// condition — the whole game domain lives many orders of magnitude below
/// the `i64` micro range). `*` rounds half-to-even. There is no `/` operator:
/// use [`Fixed::div_half_even`], which names its rounding rule.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Fixed(i64);

impl Fixed {
    /// Zero.
    pub const ZERO: Fixed = Fixed(0);
    /// One whole unit.
    pub const ONE: Fixed = Fixed(SCALE);

    /// Wraps a raw micro-unit count.
    #[must_use]
    pub const fn from_micro(micro: i64) -> Self {
        Fixed(micro)
    }

    /// Converts a whole number of units. Panics on overflow (overflow-checks
    /// are on in every profile).
    #[must_use]
    pub const fn from_int(units: i64) -> Self {
        Fixed(units * SCALE)
    }

    /// The raw micro-unit count. This is the canonical wire representation
    /// (ADR-001 rev 2 §3): canonical encodings carry this integer, never a
    /// decimal rendering.
    #[must_use]
    pub const fn micro(self) -> i64 {
        self.0
    }

    /// True if the value is exactly zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Multiplies and divides in one step: `self * num / den`, rounded half
    /// to even at the single final division. Forming the full product in
    /// `i128` first means no intermediate rounding.
    ///
    /// This is the primitive behind percent application and curve
    /// interpolation. Panics if `den` is zero or the result leaves the `i64`
    /// micro range.
    #[must_use]
    pub fn mul_div_half_even(self, num: Fixed, den: Fixed) -> Fixed {
        assert!(den.0 != 0, "fixed-point: division by zero");
        let mut product = i128::from(self.0) * i128::from(num.0);
        let mut divisor = i128::from(den.0);
        if divisor < 0 {
            product = -product;
            divisor = -divisor;
        }
        Fixed(narrow(div_round_half_even(product, divisor)))
    }

    /// How many whole `self` it takes to cover `total`, rounded **up**.
    ///
    /// Not `div_half_even`. Three point nine swings is four swings: you
    /// cannot land a fraction of a hit, and rounding a kill down would
    /// report a fight won that was still going. This is the one place in
    /// the project where rounding to nearest is the wrong answer, so it
    /// gets its own named operation rather than a call site remembering.
    ///
    /// `None` when `self` is zero or negative: an attack that takes nothing
    /// off never finishes, and reporting some number of hits would be worse
    /// than saying so.
    // No `#[must_use]`: `Option` already carries it, and saying so twice
    // is a warning rather than emphasis.
    pub fn hits_to_cover(self, total: Fixed) -> Option<i64> {
        if self.0 <= 0 {
            return None;
        }
        if total.0 <= 0 {
            return Some(0);
        }
        // Both sides carry the same scale, so it divides out and the answer
        // is a plain count.
        Some(total.0.div_euclid(self.0) + i64::from(total.0.rem_euclid(self.0) != 0))
    }

    /// Division rounding half to even (banker's rounding), per the ADR-001
    /// rev 2 rule that division is only available through named functions.
    /// Panics if `rhs` is zero.
    #[must_use]
    pub fn div_half_even(self, rhs: Fixed) -> Fixed {
        self.mul_div_half_even(Fixed(SCALE), rhs)
    }
}

impl core::ops::Add for Fixed {
    type Output = Fixed;
    fn add(self, rhs: Fixed) -> Fixed {
        Fixed(self.0 + rhs.0)
    }
}

impl core::ops::Sub for Fixed {
    type Output = Fixed;
    fn sub(self, rhs: Fixed) -> Fixed {
        Fixed(self.0 - rhs.0)
    }
}

impl core::ops::Neg for Fixed {
    type Output = Fixed;
    fn neg(self) -> Fixed {
        Fixed(-self.0)
    }
}

impl core::ops::Mul for Fixed {
    type Output = Fixed;
    /// Full product in `i128`, then one banker's rounding back to micro-units.
    fn mul(self, rhs: Fixed) -> Fixed {
        self.mul_div_half_even(rhs, Fixed(SCALE))
    }
}

/// Rounds `n / d` half to even. `d` must be positive; callers normalise sign.
/// Correct for negative `n`: `div_euclid`/`rem_euclid` give a floor quotient
/// and a remainder in `0..d`, so the tie test is sign-free.
fn div_round_half_even(n: i128, d: i128) -> i128 {
    assert!(
        d > 0,
        "fixed-point: divisor must be positive (callers normalise sign)"
    );
    let q = n.div_euclid(d);
    let r = n.rem_euclid(d);
    let twice = r * 2;
    if twice > d || (twice == d && q % 2 != 0) {
        q + 1
    } else {
        q
    }
}

/// Narrows the rounded `i128` quotient back to the `i64` micro range.
/// Out-of-range means the computation left any plausible game magnitude —
/// that is a bug in the caller, so it panics rather than saturating silently.
fn narrow(x: i128) -> i64 {
    match i64::try_from(x) {
        Ok(v) => v,
        Err(_) => panic!("fixed-point: result outside i64 micro range"),
    }
}

impl fmt::Display for Fixed {
    /// Exact decimal rendering from integers — no floats. Trailing zeros in
    /// the fraction are trimmed; whole numbers render without a point.
    /// Presentation only: canonical encodings carry [`Fixed::micro`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Honour `{:+}`: a trace line reading "x(100+0)%" is legible, while
        // "x(1000)%" is a different number entirely.
        let sign = if self.0 < 0 {
            "-"
        } else if f.sign_plus() {
            "+"
        } else {
            ""
        };
        let abs = self.0.unsigned_abs();
        let whole = abs / SCALE.unsigned_abs();
        let frac = abs % SCALE.unsigned_abs();
        if frac == 0 {
            // `f.pad` rather than `write!`: a formatter's width and
            // alignment are as much a part of the request as its sign, and
            // writing straight to `f` drops them. A column that silently
            // refuses to line up is the same defect as `{:+}` printing
            // nothing — the caller asked for something and got no sign that
            // it was ignored.
            return f.pad(&alloc::format!("{sign}{whole}"));
        }
        let mut frac_digits = [0u8; 6];
        let mut rem = frac;
        for slot in frac_digits.iter_mut().rev() {
            *slot = b'0' + u8::try_from(rem % 10).unwrap_or(b'?');
            rem /= 10;
        }
        let mut len = 6;
        while len > 0 && frac_digits[len - 1] == b'0' {
            len -= 1;
        }
        let frac_str = core::str::from_utf8(&frac_digits[..len]).map_err(|_| fmt::Error)?;
        f.pad(&alloc::format!("{sign}{whole}.{frac_str}"))
    }
}

/// Why parsing a decimal into [`Fixed`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseFixedError {
    /// Empty input, stray characters, or a malformed number.
    Malformed,
    /// More than six fraction digits. Silent truncation is exactly the error
    /// class this project exists to prevent, so over-precision is rejected,
    /// never rounded.
    TooPrecise,
    /// The value does not fit in the `i64` micro range.
    OutOfRange,
}

impl fmt::Display for ParseFixedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseFixedError::Malformed => write!(f, "malformed fixed-point literal"),
            ParseFixedError::TooPrecise => {
                write!(f, "more than six fraction digits (micro precision)")
            }
            ParseFixedError::OutOfRange => write!(f, "outside the i64 micro range"),
        }
    }
}

impl FromStr for Fixed {
    type Err = ParseFixedError;

    /// Parses an exact decimal literal: optional sign, digits, optional
    /// fraction of at most six digits. `"7.8125"` becomes `7_812_500` micro.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (negative, body) = match s.as_bytes() {
            [b'-', rest @ ..] if !rest.is_empty() => (true, rest),
            [b'+', rest @ ..] if !rest.is_empty() => (false, rest),
            rest => (false, rest),
        };
        let mut parts = body.splitn(2, |b| *b == b'.');
        let whole = parts.next().unwrap_or(&[]);
        let frac = parts.next();
        if whole.is_empty() && frac.is_none_or(<[u8]>::is_empty) {
            return Err(ParseFixedError::Malformed);
        }
        let mut micro: i64 = 0;
        for &b in whole {
            let digit = digit_value(b)?;
            micro = micro
                .checked_mul(10)
                .and_then(|m| m.checked_add(i64::from(digit)))
                .ok_or(ParseFixedError::OutOfRange)?;
        }
        micro = micro
            .checked_mul(SCALE)
            .ok_or(ParseFixedError::OutOfRange)?;
        if let Some(frac) = frac {
            if frac.is_empty() {
                return Err(ParseFixedError::Malformed);
            }
            if frac.len() > 6 {
                return Err(ParseFixedError::TooPrecise);
            }
            let mut frac_micro: i64 = 0;
            for &b in frac {
                let digit = digit_value(b)?;
                frac_micro = frac_micro * 10 + i64::from(digit);
            }
            for _ in frac.len()..6 {
                frac_micro *= 10;
            }
            micro = micro
                .checked_add(frac_micro)
                .ok_or(ParseFixedError::OutOfRange)?;
        }
        Ok(Fixed(if negative { -micro } else { micro }))
    }
}

fn digit_value(b: u8) -> Result<u8, ParseFixedError> {
    if b.is_ascii_digit() {
        Ok(b - b'0')
    } else {
        Err(ParseFixedError::Malformed)
    }
}

#[cfg(test)]
mod hits_tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_fraction_of_a_hit_is_a_whole_hit() {
        let damage = Fixed::from_micro(27_374_116);
        // 109 health at 27.374116 is 3.98 swings, which is four swings.
        assert_eq!(damage.hits_to_cover(Fixed::from_int(109)), Some(4));
        // Exactly three is three, not four: the ceiling only rounds a
        // remainder, and inventing a swing nobody needs is as wrong as
        // dropping one they do.
        assert_eq!(
            Fixed::from_int(10).hits_to_cover(Fixed::from_int(30)),
            Some(3)
        );
    }

    #[test]
    fn an_attack_that_takes_nothing_off_never_finishes() {
        // Reporting a number here would be worse than saying so: a fully
        // resisted attack does not kill in a very large number of hits, it
        // does not kill.
        assert_eq!(Fixed::ZERO.hits_to_cover(Fixed::from_int(100)), None);
        assert_eq!(
            Fixed::from_int(-5).hits_to_cover(Fixed::from_int(100)),
            None
        );
    }

    #[test]
    fn nothing_to_take_off_is_no_hits() {
        assert_eq!(Fixed::from_int(10).hits_to_cover(Fixed::ZERO), Some(0));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::format;
    use alloc::string::ToString;

    use proptest::prelude::*;

    use super::*;

    #[test]
    fn rogue_action_speed_is_exact() {
        // 7.8125% = 7 + 13/16 — the value that forced micro over milli.
        let v: Fixed = "7.8125".parse().unwrap();
        assert_eq!(v.micro(), 7_812_500);
        assert_eq!(v.to_string(), "7.8125");
    }

    #[test]
    fn addition_and_subtraction_are_exact() {
        let a = Fixed::from_micro(7_812_500);
        let b = Fixed::from_micro(2_187_500);
        assert_eq!((a + b).micro(), 10_000_000);
        assert_eq!((a - b).micro(), 5_625_000);
        assert_eq!((-a).micro(), -7_812_500);
    }

    #[test]
    fn multiplication_uses_bankers_rounding() {
        // 1µ × 0.5 = 0.5µ → tie → rounds to even 0.
        assert_eq!(
            (Fixed::from_micro(1) * Fixed::from_micro(500_000)).micro(),
            0
        );
        // 3µ × 0.5 = 1.5µ → tie → rounds to even 2.
        assert_eq!(
            (Fixed::from_micro(3) * Fixed::from_micro(500_000)).micro(),
            2
        );
        // 5µ × 0.5 = 2.5µ → tie → rounds to even 2.
        assert_eq!(
            (Fixed::from_micro(5) * Fixed::from_micro(500_000)).micro(),
            2
        );
        // Above the tie: 0.7µ rounds up.
        assert_eq!(
            (Fixed::from_micro(7) * Fixed::from_micro(100_000)).micro(),
            1
        );
    }

    #[test]
    fn bankers_rounding_is_sign_symmetric() {
        // Half-to-even on the value: −1.5 → −2, −0.5 → 0, −2.5 → −2.
        assert_eq!(
            (Fixed::from_micro(-3) * Fixed::from_micro(500_000)).micro(),
            -2
        );
        assert_eq!(
            (Fixed::from_micro(-1) * Fixed::from_micro(500_000)).micro(),
            0
        );
        assert_eq!(
            (Fixed::from_micro(-5) * Fixed::from_micro(500_000)).micro(),
            -2
        );
    }

    #[test]
    fn whole_unit_multiplication_is_exact() {
        let seven_and = Fixed::from_str("7.8125").unwrap();
        assert_eq!((seven_and * Fixed::from_int(2)).micro(), 15_625_000);
        assert_eq!((seven_and * Fixed::ONE).micro(), 7_812_500);
        assert_eq!((seven_and * Fixed::ZERO).micro(), 0);
    }

    #[test]
    fn named_division_rounds_half_even() {
        // 1 / 3 = 0.333333̅ → 333_333µ (below half).
        assert_eq!(
            Fixed::from_int(1).div_half_even(Fixed::from_int(3)).micro(),
            333_333
        );
        // 2 / 3 = 0.666666̅ → 666_667µ (above half).
        assert_eq!(
            Fixed::from_int(2).div_half_even(Fixed::from_int(3)).micro(),
            666_667
        );
        // 1 / −4 = −0.25 exactly.
        assert_eq!(
            Fixed::from_int(1)
                .div_half_even(Fixed::from_int(-4))
                .micro(),
            -250_000
        );
    }

    #[test]
    fn display_honours_the_sign_flag() {
        // Without this, a "+0" in a trace line silently becomes "0" and runs
        // into the next character: "x(100+0)%" would read "x(1000)%".
        assert_eq!(format!("{:+}", Fixed::ZERO), "+0");
        assert_eq!(format!("{:+}", Fixed::from_int(5)), "+5");
        assert_eq!(format!("{:+}", Fixed::from_int(-5)), "-5");
        assert_eq!(format!("{}", Fixed::ZERO), "0");
    }

    #[test]
    fn display_renders_exact_decimals() {
        assert_eq!(Fixed::from_micro(7_812_500).to_string(), "7.8125");
        assert_eq!(Fixed::from_micro(-1_500_000).to_string(), "-1.5");
        assert_eq!(Fixed::from_int(306).to_string(), "306");
        assert_eq!(Fixed::from_micro(1).to_string(), "0.000001");
        assert_eq!(Fixed::ZERO.to_string(), "0");
    }

    #[test]
    fn parse_rejects_over_precision_instead_of_truncating() {
        assert_eq!(
            "0.1234567".parse::<Fixed>(),
            Err(ParseFixedError::TooPrecise)
        );
    }

    #[test]
    fn parse_rejects_garbage() {
        for bad in ["", "-", "+", ".", "1.", "1..2", "1,5", "abc", "0x10"] {
            assert!(bad.parse::<Fixed>().is_err(), "accepted: {bad:?}");
        }
    }

    proptest! {
        #[test]
        fn add_sub_roundtrip(a in -1_000_000_000_000i64..1_000_000_000_000, b in -1_000_000_000_000i64..1_000_000_000_000) {
            let (fa, fb) = (Fixed::from_micro(a), Fixed::from_micro(b));
            prop_assert_eq!((fa + fb) - fb, fa);
        }

        #[test]
        fn mul_is_commutative(a in -1_000_000_000i64..1_000_000_000, b in -1_000_000_000i64..1_000_000_000) {
            let (fa, fb) = (Fixed::from_micro(a), Fixed::from_micro(b));
            prop_assert_eq!(fa * fb, fb * fa);
        }

        #[test]
        fn mul_by_one_is_identity(a in -1_000_000_000_000i64..1_000_000_000_000) {
            let fa = Fixed::from_micro(a);
            prop_assert_eq!(fa * Fixed::ONE, fa);
        }

        #[test]
        fn display_parse_roundtrip(a in -1_000_000_000_000i64..1_000_000_000_000) {
            let fa = Fixed::from_micro(a);
            let rendered = format!("{fa}");
            let parsed: Fixed = rendered.parse().unwrap();
            prop_assert_eq!(parsed, fa);
        }

        #[test]
        fn rounding_error_is_at_most_half_micro(a in -1_000_000i64..1_000_000, b in -1_000_000i64..1_000_000) {
            // |(a*b)/SCALE - round(...)| ≤ 0.5µ, measured in the exact i128 domain.
            let (fa, fb) = (Fixed::from_micro(a), Fixed::from_micro(b));
            let exact_times_scale = i128::from(a) * i128::from(b);
            let rounded = i128::from((fa * fb).micro());
            let err_times_scale = (exact_times_scale - rounded * i128::from(SCALE)).abs();
            prop_assert!(err_times_scale * 2 <= i128::from(SCALE));
        }
    }
}
