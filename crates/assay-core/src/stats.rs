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

/// Applies an Item Armor Rating Bonus to the item-sourced armour rating:
/// `item_ar × (100 + bonus) / 100`, one banker's rounding (ADR-005
/// amendment). Only the item bucket is passed in — keeping the other
/// bucket out of this call is how the exclusion is enforced.
#[must_use]
pub fn apply_item_armor_bonus(item_ar: Fixed, bonus: Fixed) -> Fixed {
    item_ar.mul_div_half_even(PERCENT + bonus, PERCENT)
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

/// What kind of damage a strike deals.
///
/// The type does not add a step or reorder one: it chooses which stats the
/// nine already have read (ADR-006 amendment: damage type). Physical Power
/// Bonus or Magic Power Bonus at step 3; Armor Rating or Magic Resistance at
/// step 5; the reduction each of those converts into at 6 and 7.
///
/// True damage is not a third type. It has its own field and lands after the
/// whole reduction chain, because bypassing reduction is what it means.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum DamageType {
    /// Reduced by armour.
    #[default]
    Physical,
    /// Reduced by magic resistance.
    Magic,
}

impl DamageType {
    /// The name used in files and readouts.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DamageType::Physical => "physical",
            DamageType::Magic => "magic",
        }
    }

    /// The attacker's power bonus for this type: step 3.
    #[must_use]
    pub fn power_bonus(self) -> &'static str {
        match self {
            DamageType::Physical => crate::derived::well_known::PHYSICAL_POWER_BONUS,
            DamageType::Magic => crate::derived::well_known::MAGIC_POWER_BONUS,
        }
    }

    /// The defender's rating for this type: step 5.
    #[must_use]
    pub fn rating(self) -> &'static str {
        match self {
            DamageType::Physical => crate::derived::well_known::ARMOR_RATING,
            DamageType::Magic => crate::derived::well_known::MAGIC_RESISTANCE,
        }
    }

    /// The reduction that rating converts into: steps 6 and 7.
    #[must_use]
    pub fn reduction(self) -> &'static str {
        match self {
            DamageType::Physical => crate::derived::well_known::PDR,
            DamageType::Magic => crate::derived::well_known::MAGICAL_DAMAGE_REDUCTION,
        }
    }
}

/// What flavour a blow carries, on either side of the damage type.
///
/// Physical damage has a kind — Slash, Pierce, Blunt. Magical damage has a
/// school — Fire, Ice and ten more. They are the same idea and so they are
/// one type: neither changes the number (the weapon decides physical damage
/// and the spell decides magical, exactly as a card's `20(1.0)` says), and
/// both exist so that perks and skills can condition on them. Blunt Weapon
/// Mastery and Fire Mastery are the same sentence with a different word in
/// it (ADR-014).
///
/// Two tag types would have meant two fields on `Strike`, two on
/// `StackedEffect`, two gate checks and two probes, to say one thing twice.
/// Nothing here stops `Blunt` reaching a magical strike; the loader does,
/// which is where every other invalid value in this dataset is caught.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DamageTag {
    /// A cut.
    Slash,
    /// A thrust.
    Pierce,
    /// An impact.
    Blunt,
    /// Fire.
    Fire,
    /// Ice.
    Ice,
    /// Lightning.
    Lightning,
    /// Earth.
    Earth,
    /// Arcane.
    Arcane,
    /// Light.
    Light,
    /// Dark.
    Dark,
    /// Evil.
    Evil,
    /// Curse.
    Curse,
    /// Divine.
    Divine,
    /// Air.
    Air,
    /// Spirit.
    Spirit,
}

impl DamageTag {
    /// Every tag, so a caller can enumerate them without repeating the list.
    pub const ALL: [DamageTag; 15] = [
        DamageTag::Slash,
        DamageTag::Pierce,
        DamageTag::Blunt,
        DamageTag::Fire,
        DamageTag::Ice,
        DamageTag::Lightning,
        DamageTag::Earth,
        DamageTag::Arcane,
        DamageTag::Light,
        DamageTag::Dark,
        DamageTag::Evil,
        DamageTag::Curse,
        DamageTag::Divine,
        DamageTag::Air,
        DamageTag::Spirit,
    ];

    /// The name used in files and readouts.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DamageTag::Slash => "slash",
            DamageTag::Pierce => "pierce",
            DamageTag::Blunt => "blunt",
            DamageTag::Fire => "fire",
            DamageTag::Ice => "ice",
            DamageTag::Lightning => "lightning",
            DamageTag::Earth => "earth",
            DamageTag::Arcane => "arcane",
            DamageTag::Light => "light",
            DamageTag::Dark => "dark",
            DamageTag::Evil => "evil",
            DamageTag::Curse => "curse",
            DamageTag::Divine => "divine",
            DamageTag::Air => "air",
            DamageTag::Spirit => "spirit",
        }
    }

    /// Which side of the type this tag belongs to.
    ///
    /// A blunt spell and a fiery sword-swing are both nonsense, and this is
    /// what lets the loader say so by name instead of accepting a gate that
    /// can never fire.
    #[must_use]
    pub fn damage_type(self) -> DamageType {
        match self {
            DamageTag::Slash | DamageTag::Pierce | DamageTag::Blunt => DamageType::Physical,
            _ => DamageType::Magic,
        }
    }

    /// Reads the name a data file writes.
    ///
    /// `None` for anything else, and the caller names the offender: a tag
    /// nobody recognises must not become a swing that quietly matches no
    /// gate, because a gate that never fires looks exactly like a perk that
    /// does nothing.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        DamageTag::ALL.into_iter().find(|t| t.as_str() == text)
    }
}

/// How good a copy of an item is.
///
/// A property of the copy rather than of the kind of thing — two Great
/// Helms of different rarity roll different numbers — but every dataset
/// entry is one specific copy, so it belongs on the entry.
///
/// It had been living inside the display name as a `(Epic)` suffix, which
/// meant the only way to know an item's rarity was to parse a string
/// written for a human to read.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Rarity {
    /// Grey.
    Poor,
    /// White.
    Common,
    /// Green.
    Uncommon,
    /// Blue.
    Rare,
    /// Purple.
    Epic,
    /// Orange.
    Legendary,
    /// Red.
    Unique,
}

impl Rarity {
    /// The name used in files and readouts.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Rarity::Poor => "poor",
            Rarity::Common => "common",
            Rarity::Uncommon => "uncommon",
            Rarity::Rare => "rare",
            Rarity::Epic => "epic",
            Rarity::Legendary => "legendary",
            Rarity::Unique => "unique",
        }
    }

    /// Reads the name a data file writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "poor" => Some(Rarity::Poor),
            "common" => Some(Rarity::Common),
            "uncommon" => Some(Rarity::Uncommon),
            "rare" => Some(Rarity::Rare),
            "epic" => Some(Rarity::Epic),
            "legendary" => Some(Rarity::Legendary),
            "unique" => Some(Rarity::Unique),
            _ => None,
        }
    }
}
