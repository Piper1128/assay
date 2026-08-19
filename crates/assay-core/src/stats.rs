//! Stat magnitudes as distinct newtypes (ADR-002).
//!
//! Five of the game's magnitudes are all "a number with a percent sign" and
//! behave fundamentally differently. Each is a distinct newtype over
//! [`Fixed`]; there is no `From`, no `Into` and no `Deref` between them, and
//! operators exist only where they are semantically valid. Conversions
//! between magnitudes happen exclusively through named functions that
//! represent an actual game mechanic (they land with the resolution pipeline,
//! ADR-005/006). If no named conversion exists, the mechanic does not exist.
//!
//! The two bans the ADR names explicitly are proven at compile time:
//!
//! `PdrPercent + PdrMod` does not exist — PDR Mod is a multiplicative layer
//! on top of PDR, never a summable bonus:
//!
//! ```compile_fail
//! use assay_core::stats::{PdrMod, PdrPercent};
//! use assay_core::Fixed;
//! let _ = PdrPercent::new(Fixed::ZERO) + PdrMod::new(Fixed::ZERO);
//! ```
//!
//! `MoveSpeedAdd + MoveSpeedBonus` does not exist — flat move speed and
//! percentage move speed apply at different pipeline stages:
//!
//! ```compile_fail
//! use assay_core::stats::{MoveSpeedAdd, MoveSpeedBonus};
//! use assay_core::Fixed;
//! let _ = MoveSpeedAdd::new(Fixed::ZERO) + MoveSpeedBonus::new(Fixed::ZERO);
//! ```

use crate::fixed::Fixed;

/// Declares a stat newtype over [`Fixed`] with explicit wrap/unwrap and no
/// implicit conversion to anything.
macro_rules! fixed_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
        pub struct $name(Fixed);

        impl $name {
            /// Wraps a fixed-point magnitude. Explicit by design: the ADR-002
            /// ban is on conversions *between* magnitudes, not on naming the
            /// wrap.
            #[must_use]
            pub const fn new(value: Fixed) -> Self {
                Self(value)
            }

            /// The underlying fixed-point value, surrendered explicitly.
            #[must_use]
            pub const fn value(self) -> Fixed {
                self.0
            }
        }
    };
}

/// Adds same-type `+`/`-` to a stat newtype. Only for magnitudes where
/// multiple sources genuinely sum (ADR-002: "bonuses are summed").
macro_rules! fixed_newtype_additive {
    ($name:ident) => {
        impl core::ops::Add for $name {
            type Output = $name;
            fn add(self, rhs: $name) -> $name {
                $name(self.0 + rhs.0)
            }
        }

        impl core::ops::Sub for $name {
            type Output = $name;
            fn sub(self, rhs: $name) -> $name {
                $name(self.0 - rhs.0)
            }
        }
    };
}

fixed_newtype!(
    /// Raw armor value; input to the PDR curve, never a percentage itself.
    /// Sums across gear pieces.
    ArmorRating
);
fixed_newtype_additive!(ArmorRating);

fixed_newtype!(
    /// Physical Damage Reduction as produced by the curve, capped (60%, or
    /// 75% with Defense Mastery). Additive with other PDR bonuses.
    PdrPercent
);
fixed_newtype_additive!(PdrPercent);

fixed_newtype!(
    /// Multiplicative layer applied *on top of* resolved PDR (Lethal Mark
    /// −30%). Not part of the defender's stat block — a modifier on the
    /// exchange (ADR-005 stage 8). Deliberately has no operators.
    PdrMod
);

fixed_newtype!(
    /// Reduces the defender's [`ArmorRating`] before the curve lookup
    /// (Thrust 15%). Sums across sources.
    ArmorPen
);
fixed_newtype_additive!(ArmorPen);

fixed_newtype!(
    /// Damage that bypasses the whole reduction chain and is added after it
    /// (Dagger Mastery +1). Sums across sources.
    TrueDamage
);
fixed_newtype_additive!(TrueDamage);

fixed_newtype!(
    /// Physical/Magical Power Bonus in percent points. Sums across sources;
    /// applied at ADR-006 stage 3.
    PowerBonus
);
fixed_newtype_additive!(PowerBonus);

fixed_newtype!(
    /// Flat move speed (ADR-005 stage 5). Never mixes with the percentage
    /// bonus — different stages.
    MoveSpeedAdd
);
fixed_newtype_additive!(MoveSpeedAdd);

fixed_newtype!(
    /// Percentage move speed bonus (ADR-005 stage 6). Never mixes with the
    /// flat add — different stages.
    MoveSpeedBonus
);
fixed_newtype_additive!(MoveSpeedBonus);

fixed_newtype!(
    /// Damage in hit points, at any step of the exchange chain (ADR-006).
    /// Sums where several sources contribute flat damage.
    Damage
);
fixed_newtype_additive!(Damage);

fixed_newtype!(
    /// A skill's scaling coefficient in percent (Sneak Attack 0%, Rupture
    /// 75%, Caltrops 100%) — ADR-006 step 2. Deliberately not additive: two
    /// coefficients never sum, and 0% is a load-bearing value, not a default.
    ScalingCoefficient
);

fixed_newtype!(
    /// PDR after the multiplicative PDR Mod layer (ADR-002
    /// `apply_pdr_mod`). Distinct from [`PdrPercent`] because it belongs to
    /// an *exchange*, not to a defender's stat block.
    EffectivePdr
);

/// Percent scale: percentages are stored as points, so 30% is `Fixed(30)`.
const PERCENT: Fixed = Fixed::from_int(100);

/// Applies a percentage to a damage value: `damage × (100 + percent) / 100`,
/// one banker's rounding. The named home for "a percent bonus was applied".
#[must_use]
pub fn apply_percent(damage: Damage, percent: Fixed) -> Damage {
    Damage::new(damage.value().mul_div_half_even(PERCENT + percent, PERCENT))
}

/// Applies a skill's scaling coefficient (ADR-006 step 2): `base × coeff`,
/// where the coefficient is a percentage of the base. A 0% coefficient
/// yields zero scaled damage — that is the mechanic, not a bug.
#[must_use]
pub fn apply_scaling(base: Damage, coefficient: ScalingCoefficient) -> Damage {
    Damage::new(base.value().mul_div_half_even(coefficient.value(), PERCENT))
}

/// Reduces the defender's armor rating by the attacker's penetration
/// (ADR-006 step 5): `armor × (100 − pen) / 100`, floored at zero.
/// Penetration is a percentage of the rating, never a flat subtraction.
#[must_use]
pub fn penetrate(armor: ArmorRating, pen: ArmorPen) -> ArmorRating {
    let reduced = armor
        .value()
        .mul_div_half_even(PERCENT - pen.value(), PERCENT);
    ArmorRating::new(reduced.max(Fixed::ZERO))
}

/// Applies the multiplicative PDR Mod layer (ADR-002's locked conversion):
/// `pdr × (100 + mod) / 100`. Lethal Mark's −30% turns 60% PDR into 42%,
/// never into 30% — the additive reading is what `pdr_mod_additive` probes.
#[must_use]
pub fn apply_pdr_mod(base: PdrPercent, m: PdrMod) -> EffectivePdr {
    EffectivePdr::new(
        base.value().mul_div_half_even(PERCENT + m.value(), PERCENT), // probe: pdr-mod-multiplicative
    )
}

/// Applies damage reduction to a damage value: `damage × (100 − pdr) / 100`.
/// Negative PDR (light armor at low ratings) correctly *increases* damage.
#[must_use]
pub fn reduce_by_pdr(damage: Damage, pdr: EffectivePdr) -> Damage {
    Damage::new(
        damage
            .value()
            .mul_div_half_even(PERCENT - pdr.value(), PERCENT),
    )
}

/// A character attribute (Strength, Agility, …). Whole integer by nature —
/// the game rolls and buffs attributes in whole points; only *derived* stats
/// are fractional.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Attribute(i32);

impl Attribute {
    /// Wraps a whole-point attribute value.
    #[must_use]
    pub const fn new(points: i32) -> Self {
        Attribute(points)
    }

    /// The whole-point value, surrendered explicitly.
    #[must_use]
    pub const fn points(self) -> i32 {
        self.0
    }
}

impl core::ops::Add for Attribute {
    type Output = Attribute;
    fn add(self, rhs: Attribute) -> Attribute {
        Attribute(self.0 + rhs.0)
    }
}

impl core::ops::Sub for Attribute {
    type Output = Attribute;
    fn sub(self, rhs: Attribute) -> Attribute {
        Attribute(self.0 - rhs.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::fixed::Fixed;

    #[test]
    fn summable_magnitudes_sum_within_their_type() {
        let a = PdrPercent::new("12.5".parse().unwrap());
        let b = PdrPercent::new("7.5".parse().unwrap());
        assert_eq!((a + b).value(), Fixed::from_int(20));

        let fortified = Attribute::new(3);
        let jokester = Attribute::new(2);
        assert_eq!((fortified + jokester).points(), 5);
    }

    #[test]
    fn newtypes_do_not_compare_across_types() {
        // Same numeric payload, different meanings — the type system keeps
        // them apart; equality only exists within a type.
        let rating = ArmorRating::new(Fixed::from_int(30));
        let pen = ArmorPen::new(Fixed::from_int(30));
        assert_eq!(rating.value(), pen.value());
    }
}
