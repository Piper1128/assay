//! The damage application model (ADR-006).
//!
//! Damage is not a method on the attacker or the defender — it is an
//! [`Exchange`]: a pure value object over attacker, defender and context.
//! Anything else makes "how much damage" depend on which object you happened
//! to ask.
//!
//! Locked step order:
//!
//! ```text
//! 1. base damage from weapon/skill
//! 2. × scaling coefficient        Sneak Attack 0% · Rupture 75% · Caltrops 100%
//! 3. + Physical Power Bonus       Back Attack +30% applies here
//! 4. + flat Buff Weapon Damage    Slayer +5 · Sword Mastery +2
//! 5. − defender's Armor Rating, reduced by attacker's Armor Penetration
//! 6. → PDR from the curve, capped
//! 7. × PDR Mod (multiplicative)   Lethal Mark −30%
//! 8. + True Damage (untouched by 5–7)   Dagger Mastery +1
//! 9. × hit location multiplier    headshot / airborne
//! ```
//!
//! **Step 2 is the most consequential detail in the model.** Sneak Attack has
//! 0% scaling and is therefore immune to the Hide-exit penalty of −30%
//! Physical Power Bonus. Model scaling implicitly and that insight vanishes —
//! and it is the entire basis for the Rogue's correct opener in Season 10.
//! The `scaling_ignored` probe hardcodes the coefficient to 100% and expects
//! the Sneak Attack fixture to fail.
//!
//! **Step 8 comes after reduction, never before.** True Damage bypasses the
//! armor chain; adding it earlier would let armor eat it. Probed by
//! `true_damage_pre_reduction`.
//!
//! ## Flagged ADR ambiguity (ADR-010 rev 2 §3: an ambiguous ADR is the bug)
//!
//! ADR-006 lists steps 6 and 7 as consecutive operations on the *damage*
//! ("→ PDR from the curve, capped", "× PDR Mod"), which read literally would
//! apply PDR to the damage and then multiply the damage by the mod. ADR-002
//! locks a different shape in a signature: `apply_pdr_mod(base: PdrPercent,
//! m: PdrMod) -> EffectivePdr` — the mod modifies *the PDR*, producing an
//! effective PDR that then reduces damage once.
//!
//! This implementation follows ADR-002's locked signature, because a typed
//! conversion in an accepted ADR is a stronger statement than prose step
//! ordering, and because the alternative double-reduces. Consequence with
//! Lethal Mark (−30%) against 60% PDR: effective PDR 42%, damage × 0.58 —
//! not damage × 0.40 × 0.70. **This needs an ADR-006 amendment to say so in
//! one place instead of two.**

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::confidence::Confidence;
use crate::fixed::Fixed;
use crate::resolve::{Resolved, StageNote};
use crate::stats::{
    ArmorPen, ArmorRating, Damage, EffectivePdr, PdrMod, PdrPercent, ScalingCoefficient,
    TrueDamage, apply_pdr_mod, apply_percent, apply_scaling, penetrate, reduce_by_pdr,
};

/// What the attacker brings to one exchange: the strike being made, as
/// opposed to the attacker's standing stat block.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Strike {
    /// Base damage of the weapon or skill (step 1).
    pub base: Confidence<Damage>,
    /// The skill's scaling coefficient (step 2). 100% for a plain weapon
    /// swing; 0% for Sneak Attack.
    pub scaling: Confidence<ScalingCoefficient>,
    /// Flat Buff Weapon Damage (step 4).
    pub flat_bonus: Confidence<Damage>,
    /// Armor penetration carried by the strike (step 5).
    pub armor_pen: Confidence<ArmorPen>,
    /// True damage, applied after the whole reduction chain (step 8).
    pub true_damage: Confidence<TrueDamage>,
}

/// Situational modifiers that belong to the exchange, not to either stat
/// block (ADR-005 stage 8 hands these over deliberately).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExchangeContext {
    /// Percentage-point adjustment to the attacker's Physical Power Bonus
    /// for this strike: Back Attack +30, Hide-exit −30 (step 3).
    pub power_bonus_adjust: Confidence<Fixed>,
    /// Multiplicative PDR Mod on the defender (step 7): Lethal Mark −30.
    pub pdr_mod: Confidence<PdrMod>,
    /// Hit location multiplier in percentage points relative to 100
    /// (step 9): headshot and airborne bonuses.
    pub hit_location_bonus: Confidence<Fixed>,
}

impl Default for ExchangeContext {
    /// A neutral exchange: no positional bonus, no debuff, body shot.
    fn default() -> Self {
        ExchangeContext {
            power_bonus_adjust: Confidence::Verified(Fixed::ZERO),
            pdr_mod: Confidence::Verified(PdrMod::new(Fixed::ZERO)),
            hit_location_bonus: Confidence::Verified(Fixed::ZERO),
        }
    }
}

/// One damage exchange: attacker, defender, context. Fully pure.
pub struct Exchange<'a> {
    attacker: &'a Resolved,
    defender: &'a Resolved,
    strike: &'a Strike,
    context: &'a ExchangeContext,
}

/// The outcome of an exchange, with the same inspectable trail the
/// resolution pipeline produces.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExchangeOutcome {
    /// Final damage dealt.
    pub damage: Confidence<Damage>,
    /// The defender's effective PDR for this exchange, after the mod layer.
    pub effective_pdr: Confidence<EffectivePdr>,
    /// The `--explain` trail, in step order.
    pub trace: Vec<StageNote>,
}

impl<'a> Exchange<'a> {
    /// Builds an exchange from two resolved stat blocks, a strike and a
    /// context.
    #[must_use]
    pub fn new(
        attacker: &'a Resolved,
        defender: &'a Resolved,
        strike: &'a Strike,
        context: &'a ExchangeContext,
    ) -> Self {
        Exchange {
            attacker,
            defender,
            strike,
            context,
        }
    }

    /// Runs the nine steps in the locked order.
    #[must_use]
    pub fn damage(&self) -> ExchangeOutcome {
        let mut trace: Vec<StageNote> = Vec::new();

        // ── 1: base damage ───────────────────────────────────────────────
        let base = self.strike.base.clone();
        trace.push(StageNote {
            stage: 1,
            label: "base damage",
            detail: format!("{}", base.value().value()),
        });

        // ── 2: × scaling coefficient ─────────────────────────────────────
        // A 0% coefficient means the skill's damage does not scale at all —
        // which is exactly why Sneak Attack ignores the Hide-exit penalty.
        let scaled = base
            .clone()
            .zip_with(self.strike.scaling.clone(), apply_scaling); // probe: scaling-coefficient
        trace.push(StageNote {
            stage: 2,
            label: "scaling coefficient",
            detail: format!(
                "× {}% → {}",
                self.strike.scaling.value().value(),
                scaled.value().value()
            ),
        });

        // ── 3: + Physical Power Bonus (with situational adjustment) ──────
        let power = self
            .attacker
            .physical_power_bonus
            .clone()
            .zip_with(self.context.power_bonus_adjust.clone(), |ppb, adjust| {
                ppb + adjust
            });
        let powered = scaled.clone().zip_with(power.clone(), |damage, percent| {
            apply_percent(damage, percent)
        });
        trace.push(StageNote {
            stage: 3,
            label: "physical power bonus",
            detail: format!("× (100{:+})% → {}", power.value(), powered.value().value()),
        });

        // ── 4: + flat buff weapon damage ─────────────────────────────────
        let with_flat = powered
            .clone()
            .zip_with(self.strike.flat_bonus.clone(), |damage, flat| damage + flat);
        trace.push(StageNote {
            stage: 4,
            label: "flat weapon damage",
            detail: format!(
                "{:+} → {}",
                self.strike.flat_bonus.value().value(),
                with_flat.value().value()
            ),
        });

        // ── 5: defender's armor rating, reduced by penetration ───────────
        let armor = self
            .defender
            .armor_rating
            .clone()
            .map(ArmorRating::new)
            .zip_with(self.strike.armor_pen.clone(), penetrate);
        trace.push(StageNote {
            stage: 5,
            label: "armor penetration",
            detail: format!(
                "armor {} − pen {}% → {}",
                self.defender.armor_rating.value(),
                self.strike.armor_pen.value().value(),
                armor.value().value()
            ),
        });

        // ── 6: → PDR from the curve, capped ──────────────────────────────
        // The defender's resolved PDR already went through curve and cap
        // (ADR-005 stage 7). Penetration changes the armor rating, so the
        // reduction is rescaled by how much rating survived — a linear
        // approximation the dataset arc replaces with a re-sample of the
        // real curve at the penetrated rating.
        let base_pdr = self.defender.pdr.clone().map(PdrPercent::new);
        let pdr_after_pen = base_pdr.clone().zip_with(
            self.defender
                .armor_rating
                .clone()
                .zip_with(armor.clone(), |full, penetrated| (full, penetrated)),
            |pdr, (full, penetrated)| {
                if full.is_zero() {
                    pdr
                } else {
                    PdrPercent::new(pdr.value().mul_div_half_even(penetrated.value(), full))
                }
            },
        );
        trace.push(StageNote {
            stage: 6,
            label: "PDR from curve",
            detail: format!(
                "{} → {} after penetration",
                base_pdr.value().value(),
                pdr_after_pen.value().value()
            ),
        });

        // ── 7: × PDR Mod (multiplicative on the PDR — see module doc) ────
        let effective_pdr = pdr_after_pen
            .clone()
            .zip_with(self.context.pdr_mod.clone(), apply_pdr_mod);
        let reduced = with_flat
            .clone()
            .zip_with(effective_pdr.clone(), reduce_by_pdr);
        trace.push(StageNote {
            stage: 7,
            label: "PDR mod",
            detail: format!(
                "PDR mod {}% → effective PDR {}% → {}",
                self.context.pdr_mod.value().value(),
                effective_pdr.value().value(),
                reduced.value().value()
            ),
        });

        // ── 8: + true damage, AFTER the reduction chain ──────────────────
        // True Damage bypasses armor by definition, so it joins the chain
        // only once reduction is done. Feeding `with_flat` here instead is
        // what the true_damage_pre_reduction probe does.
        let post_reduction = reduced.clone(); // probe: true-damage-post-reduction
        let with_true = post_reduction
            .zip_with(self.strike.true_damage.clone(), |damage, true_dmg| {
                damage + Damage::new(true_dmg.value())
            });
        trace.push(StageNote {
            stage: 8,
            label: "true damage",
            detail: format!(
                "{:+} (bypasses armor) → {}",
                self.strike.true_damage.value().value(),
                with_true.value().value()
            ),
        });

        // ── 9: × hit location multiplier ─────────────────────────────────
        let final_damage = with_true
            .clone()
            .zip_with(self.context.hit_location_bonus.clone(), |damage, bonus| {
                apply_percent(damage, bonus)
            });
        trace.push(StageNote {
            stage: 9,
            label: "hit location",
            detail: format!(
                "× (100{:+})% → {}",
                self.context.hit_location_bonus.value(),
                final_damage.value().value()
            ),
        });

        ExchangeOutcome {
            damage: final_damage,
            effective_pdr,
            trace,
        }
    }
}

/// Renders an exchange outcome's trace for `--explain`.
#[must_use]
pub fn explain(outcome: &ExchangeOutcome) -> String {
    let mut out = String::new();
    for note in &outcome.trace {
        out.push_str(&format!(
            "{}. {}: {}\n",
            note.stage, note.label, note.detail
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::vec::Vec;

    use super::*;
    use crate::confidence::ConfidenceLevel;
    use crate::schema::AttributeBlock;

    fn fx(units: i64) -> Fixed {
        Fixed::from_int(units)
    }

    /// A resolved block with only the fields the exchange reads.
    fn combatant(power_bonus: i64, armor: i64, pdr: i64) -> Resolved {
        Resolved {
            attributes: Confidence::Verified(AttributeBlock::default()),
            physical_power_bonus: Confidence::Verified(fx(power_bonus)),
            action_speed: Confidence::Verified(Fixed::ZERO),
            move_speed: Confidence::Verified(Fixed::ZERO),
            health: Confidence::Verified(fx(100)),
            armor_rating: Confidence::Verified(fx(armor)),
            pdr: Confidence::Verified(fx(pdr)),
            trace: Vec::new(),
        }
    }

    fn strike(base: i64, scaling: i64, flat: i64, pen: i64, true_dmg: i64) -> Strike {
        Strike {
            base: Confidence::Verified(Damage::new(fx(base))),
            scaling: Confidence::Verified(ScalingCoefficient::new(fx(scaling))),
            flat_bonus: Confidence::Verified(Damage::new(fx(flat))),
            armor_pen: Confidence::Verified(ArmorPen::new(fx(pen))),
            true_damage: Confidence::Verified(TrueDamage::new(fx(true_dmg))),
        }
    }

    fn damage_of(
        attacker: &Resolved,
        defender: &Resolved,
        strike: &Strike,
        context: &ExchangeContext,
    ) -> Fixed {
        Exchange::new(attacker, defender, strike, context)
            .damage()
            .damage
            .value()
            .value()
    }

    #[test]
    fn plain_swing_walks_the_whole_chain() {
        // base 20 × 100% × (100+0)% + 0 flat, no armor, no mods → 20.
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 0, 0);
        let s = strike(20, 100, 0, 0, 0);
        assert_eq!(
            damage_of(&attacker, &defender, &s, &ExchangeContext::default()),
            fx(20)
        );
    }

    #[test]
    fn sneak_attack_zero_scaling_ignores_the_hide_penalty() {
        // THE mechanic the scaling_ignored probe protects. Sneak Attack is
        // modelled as flat damage (step 4) with 0% scaling (step 2), so the
        // Hide-exit −30% Physical Power Bonus cannot touch it.
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 0, 0);
        let sneak = Strike {
            base: Confidence::Verified(Damage::new(fx(10))),
            scaling: Confidence::Verified(ScalingCoefficient::new(Fixed::ZERO)),
            flat_bonus: Confidence::Verified(Damage::new(fx(15))),
            armor_pen: Confidence::Verified(ArmorPen::new(Fixed::ZERO)),
            true_damage: Confidence::Verified(TrueDamage::new(Fixed::ZERO)),
        };
        let neutral = ExchangeContext::default();
        let hide_exit = ExchangeContext {
            power_bonus_adjust: Confidence::Verified(fx(-30)),
            ..ExchangeContext::default()
        };
        let without = damage_of(&attacker, &defender, &sneak, &neutral);
        let with_penalty = damage_of(&attacker, &defender, &sneak, &hide_exit);
        assert_eq!(without, fx(15), "0% scaling: only the flat 15 survives");
        assert_eq!(
            with_penalty, without,
            "0% scaling must be immune to the Hide-exit power penalty"
        );

        // A scaling strike of the same nominal size IS penalised — that is
        // the contrast the Season 10 opener rests on.
        let scaling_strike = strike(15, 100, 0, 0, 0);
        assert!(
            damage_of(&attacker, &defender, &scaling_strike, &hide_exit)
                < damage_of(&attacker, &defender, &scaling_strike, &neutral)
        );
    }

    #[test]
    fn true_damage_lands_after_reduction() {
        // Against 50% PDR: 20 base → 10 after reduction, +5 true → 15.
        // Adding it before reduction would give (20+5)×0.5 = 12.5.
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 100, 50);
        let s = strike(20, 100, 0, 0, 5);
        assert_eq!(
            damage_of(&attacker, &defender, &s, &ExchangeContext::default()),
            "15".parse().unwrap()
        );
    }

    #[test]
    fn pdr_mod_is_multiplicative_not_additive() {
        // Lethal Mark −30 against 60% PDR: effective 42%, damage × 0.58.
        // The additive reading would give 30% and damage × 0.70.
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 100, 60);
        let s = strike(100, 100, 0, 0, 0);
        let marked = ExchangeContext {
            pdr_mod: Confidence::Verified(PdrMod::new(fx(-30))),
            ..ExchangeContext::default()
        };
        let outcome = Exchange::new(&attacker, &defender, &s, &marked).damage();
        assert_eq!(outcome.effective_pdr.value().value(), fx(42));
        assert_eq!(outcome.damage.value().value(), fx(58));
    }

    #[test]
    fn armor_penetration_scales_the_rating_not_the_damage() {
        // 15% pen against 100 armor → 85 rating; the resolved 40% PDR
        // rescales to 34%, so damage × 0.66.
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 100, 40);
        let s = strike(100, 100, 0, 15, 0);
        let outcome = Exchange::new(&attacker, &defender, &s, &ExchangeContext::default()).damage();
        assert_eq!(outcome.effective_pdr.value().value(), fx(34));
        assert_eq!(outcome.damage.value().value(), fx(66));
    }

    #[test]
    fn back_attack_and_headshot_apply_at_their_steps() {
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 0, 0);
        let s = strike(100, 100, 0, 0, 0);
        let context = ExchangeContext {
            power_bonus_adjust: Confidence::Verified(fx(30)),
            hit_location_bonus: Confidence::Verified(fx(2)),
            ..ExchangeContext::default()
        };
        // 100 × 1.30 = 130, then × 1.02 = 132.6.
        assert_eq!(
            damage_of(&attacker, &defender, &s, &context),
            "132.6".parse().unwrap()
        );
    }

    #[test]
    fn negative_pdr_increases_damage() {
        // A naked Rogue sits at −22% PDR; light armor genuinely takes more.
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 0, -22);
        let s = strike(100, 100, 0, 0, 0);
        assert_eq!(
            damage_of(&attacker, &defender, &s, &ExchangeContext::default()),
            fx(122)
        );
    }

    #[test]
    fn confidence_degrades_through_the_exchange() {
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 0, 0);
        let mut s = strike(20, 100, 0, 0, 0);
        s.scaling = Confidence::Unverified(ScalingCoefficient::new(fx(100)));
        let outcome = Exchange::new(&attacker, &defender, &s, &ExchangeContext::default()).damage();
        assert_eq!(outcome.damage.level(), ConfidenceLevel::Unverified);
    }

    #[test]
    fn every_step_leaves_a_trace() {
        let attacker = combatant(0, 0, 0);
        let defender = combatant(0, 0, 0);
        let s = strike(20, 100, 0, 0, 0);
        let outcome = Exchange::new(&attacker, &defender, &s, &ExchangeContext::default()).damage();
        for step in 1..=9u8 {
            assert!(
                outcome.trace.iter().any(|n| n.stage == step),
                "step {step} missing from trace"
            );
        }
        assert!(explain(&outcome).contains("scaling coefficient"));
    }
}
