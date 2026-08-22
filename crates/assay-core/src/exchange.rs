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
//! ## Where the PDR Mod applies
//!
//! ADR-006's step list reads as though step 7 multiplied the *damage* by the
//! mod. It does not: the mod is a multiplicative layer on the *PDR*,
//! producing an effective PDR that reduces damage once — ADR-002's locked
//! `apply_pdr_mod(PdrPercent, PdrMod) -> EffectivePdr`. Lethal Mark (−30%)
//! against 60% PDR gives effective PDR 42% and damage × 0.58, not
//! × 0.40 × 0.70.
//!
//! Settled in `docs/adr/ADR-006-amendment-pdr-mod-layer.md`: the literal
//! reading is wrong in direction, since it would make an attacker's debuff
//! *reduce* the attacker's damage. The magnitude is still wiki-sourced and
//! therefore `Unverified` (ADR-007) until tested in game.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::confidence::Confidence;
use crate::derived::well_known;
use crate::fixed::Fixed;
use crate::ids::{AbilityId, ClassId, CurveId, DerivedStatId, ItemId};
use crate::resolve::{Resolved, StageNote};
use crate::schema::{DatasetSource, WeaponProfile};
pub use crate::stats::{DamageTag, DamageType};

use alloc::collections::BTreeSet;

use crate::stats::{
    ArmorPen, ArmorRating, Damage, EffectivePdr, PdrMod, PdrPercent, ScalingCoefficient,
    TrueDamage, apply_item_armor_bonus, apply_pdr_mod, apply_percent, apply_scaling, penetrate,
    reduce_by_pdr,
};

/// One attack, as ADR-006 step 1 through 4 describe it: what is being
/// swung and with what modifiers.
/// What the attacker brings to one exchange: the strike being made, as
/// opposed to the attacker's standing stat block.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Strike {
    /// What kind of damage this is, which decides the stats steps 3 and
    /// 5 through 7 read.
    pub damage_type: DamageType,
    /// Base damage of the weapon or skill (step 1).
    pub base: Confidence<Damage>,
    /// The skill's scaling coefficient (step 2). 100% for a plain weapon
    /// swing; 0% for Sneak Attack.
    pub scaling: Confidence<ScalingCoefficient>,
    /// Flat Buff Weapon Damage (step 4).
    pub flat_bonus: Confidence<Damage>,
    /// What this blow is made of: a kind if it is physical, one or more
    /// schools if it is magical (ADR-014).
    ///
    /// A set, because the source says magical damage is *one or more* of the
    /// schools. Whether that means one blow carrying two or a spell dealing
    /// two blows of one each is unmeasured, and a set is safe against both
    /// readings — it holds a single element for every card anyone has read.
    ///
    /// Empty means nothing is known about the blow, or that it is Neutral:
    /// magical damage of no school. Either way a gated effect stays shut,
    /// because a bonus that fires when we could not tell is worse than one
    /// that does not fire.
    pub tags: BTreeSet<DamageTag>,
    /// Whether this attack is one specific blow rather than the weapon
    /// simply swinging: a named skill, or a swing picked out of the chain.
    ///
    /// Set, the chain count stays quiet. Asking "what does the third swing
    /// do" and being answered about a whole chain is not an answer.
    pub pinned: bool,
    /// Which weapon this came from, when it came from one. Carried so the
    /// swing time can be looked up: damage says how hard, and the weapon
    /// says how often, and a fight is both.
    pub weapon: Option<ItemId>,
    /// Penetration carried by the strike (step 5). Which defence it
    /// reduces follows from the damage type: armour rating for a physical
    /// strike, magic resistance for a magic one. Keeping `armor_pen` as the
    /// name on a field a magic attack also uses would be the sort of small
    /// lie that survives for years.
    pub penetration: Confidence<ArmorPen>,
    /// True damage, applied after the whole reduction chain (step 8).
    pub true_damage: Confidence<TrueDamage>,
}

impl Strike {
    /// A plain weapon swing: the weapon's own damage and penetration, 100%
    /// scaling, nothing else.
    ///
    /// Skills bring their own scaling coefficient, flat bonus and true
    /// damage (ADR-006 steps 2, 4 and 8); this is the unmodified attack, and
    /// building it here means a weapon in the dataset reaches the damage
    /// model instead of sitting inert next to it.
    #[must_use]
    pub fn cast() -> Strike {
        Strike {
            damage_type: DamageType::Physical,
            pinned: true,
            tags: BTreeSet::new(),
            // Nothing is held, so nothing supplies a swing time and the
            // chain has no meaning: a spell is one blow, and `pinned` says
            // so rather than leaving a chain count to be computed from a
            // weapon that is not there.
            weapon: None,
            // Zero, and it stays zero unless something says otherwise. A
            // caster with nothing behind the strike deals nothing, which is
            // the honest answer — inventing a base would be the tool
            // guessing at the one number it exists to look up.
            base: Confidence::Verified(Damage::new(Fixed::ZERO)),
            scaling: Confidence::Verified(ScalingCoefficient::new(Fixed::from_int(100))),
            flat_bonus: Confidence::Verified(Damage::new(Fixed::ZERO)),
            penetration: Confidence::Verified(ArmorPen::new(Fixed::ZERO)),
            true_damage: Confidence::Verified(TrueDamage::new(Fixed::ZERO)),
        }
    }

    /// The blow a weapon makes, unmodified.
    #[must_use]
    pub fn basic_swing(id: &ItemId, weapon: &WeaponProfile) -> Strike {
        Strike {
            // A weapon swings physically unless something says otherwise,
            // which is the same thing `DamageType::default()` says.
            damage_type: DamageType::Physical,
            // A plain swing is the weapon doing its normal thing, so the
            // chain applies. Anything more specific sets this.
            pinned: false,
            // A plain swing is the chain's first swing, so it carries that
            // swing's kind. Leaving this empty would mean a mace's ordinary
            // blow was not Blunt, and every gate keyed on Blunt would sit
            // silent for the weapon it was written for.
            tags: weapon
                .combo
                .first()
                .map(|hit| hit.kind)
                .into_iter()
                .collect(),
            weapon: Some(id.clone()),
            base: weapon.base_damage.clone().map(Damage::new),
            // 100%: an unmodified swing scales fully. Sneak Attack's 0% is a
            // property of the skill, never a default (ADR-006 step 2).
            scaling: Confidence::Verified(ScalingCoefficient::new(Fixed::from_int(100))),
            flat_bonus: Confidence::Verified(Damage::new(Fixed::ZERO)),
            penetration: weapon.armor_pen.clone().map(ArmorPen::new),
            true_damage: Confidence::Verified(TrueDamage::new(Fixed::ZERO)),
        }
    }
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
    /// Item Armor Rating Bonus the attacker imposes on the defender for
    /// this strike, in percentage points, keyed by the ability that imposed
    /// it: Rogue's Weakpoint Attack is −30 under
    /// `skill.rogue.weakpoint_attack`.
    ///
    /// Keyed rather than summed because the same ability never stacks
    /// across the people carrying it — two rogues with Weakpoint apply −30,
    /// not −60. A map key cannot appear twice, so the rule holds without
    /// anyone remembering to check it. Different abilities are different
    /// keys and do sum.
    ///
    /// Whether a debuff is live is a fact about the moment, not about
    /// either build, so it is stated here and never inferred from the
    /// attacker owning the skill (ADR-006 amendment: item armor debuff).
    pub item_armor_bonus_mods: BTreeMap<AbilityId, Confidence<Fixed>>,
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
            item_armor_bonus_mods: BTreeMap::new(),
            hit_location_bonus: Confidence::Verified(Fixed::ZERO),
        }
    }
}

/// One damage exchange: attacker, defender, context, dataset.
///
/// Four inputs rather than ADR-006's original three: step 6 must re-sample
/// the defender's PDR curve at the penetrated armour rating, and that curve
/// lives in the dataset. The value object still owns nothing and mutates
/// nothing, which is what the purity clause was protecting
/// (`docs/adr/ADR-006-amendment-penetration-resampling.md`).
pub struct Exchange<'a, D: DatasetSource> {
    attacker: &'a Resolved,
    defender: &'a Resolved,
    strike: &'a Strike,
    context: &'a ExchangeContext,
    data: &'a D,
}

/// What one swing came to: the nine steps' output, before anything asks
/// how many of them a fight takes.
struct Swing {
    damage: Confidence<Damage>,
    effective_pdr: Confidence<EffectivePdr>,
    trace: Vec<StageNote>,
}

/// Why an exchange could not be computed. Everything it needs must be
/// present and consistent: a missing stat is an explicit error, never a
/// silent zero, and a dataset from another build is refused outright.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExchangeError {
    /// A derived stat the exchange reads is not defined.
    MissingStat(DerivedStatId),
    /// The dataset is not the one a combatant was resolved against
    /// (ADR-006 amendment: penetration re-sampling).
    DatasetMismatch {
        /// The build the combatant was resolved against.
        resolved_against: String,
        /// The build of the dataset handed to the exchange.
        given: String,
    },
    /// The defender's class is not in the dataset.
    UnknownClass(ClassId),
    /// The defender's PDR definition names a curve the dataset lacks.
    UnknownCurve(CurveId),
    /// The defender's class defines no PDR stat, so armour cannot be
    /// resolved at a penetrated rating.
    NoPdrDefinition(ClassId),
}

impl fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExchangeError::MissingStat(id) => write!(
                f,
                "exchange needs derived stat {id}, which the loadout does not define"
            ),
            ExchangeError::DatasetMismatch {
                resolved_against,
                given,
            } => write!(
                f,
                "combatant was resolved against build {resolved_against}, but the exchange was given build {given}; damage would silently use the wrong curves"
            ),
            ExchangeError::UnknownClass(id) => write!(f, "class not in dataset: {id}"),
            ExchangeError::UnknownCurve(id) => write!(f, "curve not in dataset: {id}"),
            ExchangeError::NoPdrDefinition(id) => {
                write!(f, "{id} defines no {} stat", well_known::PDR)
            }
        }
    }
}

/// The outcome of an exchange, with the same inspectable trail the
/// resolution pipeline produces.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExchangeOutcome {
    /// Final damage dealt.
    pub damage: Confidence<Damage>,
    /// The defender's effective PDR for this exchange, after the mod layer.
    pub effective_pdr: Confidence<EffectivePdr>,
    /// How many of this strike the defender takes before dying.
    ///
    /// `None` when the attack cannot kill at all — a fully resisted hit
    /// does not kill in a very large number of swings, it does not kill.
    pub hits_to_kill: Option<i64>,
    /// How many swings of the weapon's chain it takes, when this is the
    /// weapon simply swinging and the chain is known.
    ///
    /// Kept beside `hits_to_kill` rather than replacing it, because they
    /// answer different questions: one is "this blow, repeated", the other
    /// is "this weapon, used". A tool that silently switched between them
    /// depending on the weapon would give two readers different arithmetic
    /// under the same heading.
    pub chain_to_kill: Option<i64>,
    /// Seconds to land the chain's swings.
    pub chain_time_to_kill: Option<Confidence<Fixed>>,
    /// Seconds to land those hits, when the weapon's swing time is known.
    ///
    /// This is what makes two builds comparable as a race rather than as a
    /// pair of numbers: needing two more hits is not worse if the hits come
    /// faster. `None` when nothing has measured the weapon.
    pub time_to_kill: Option<Confidence<Fixed>>,
    /// The `--explain` trail, in step order.
    pub trace: Vec<StageNote>,
}

impl<'a, D: DatasetSource> Exchange<'a, D> {
    /// Builds an exchange from two resolved stat blocks, a strike, a context
    /// and the dataset both combatants were resolved against.
    #[must_use]
    pub fn new(
        attacker: &'a Resolved,
        defender: &'a Resolved,
        strike: &'a Strike,
        context: &'a ExchangeContext,
        data: &'a D,
    ) -> Self {
        Exchange {
            attacker,
            defender,
            strike,
            context,
            data,
        }
    }

    /// Reads one required stat from a resolved block.
    /// Reads a stat and adds whatever gated bonuses this swing switches on.
    ///
    /// The gates live on `Resolved` rather than in its numbers because they
    /// depend on the swing, not the character. This is the moment the swing
    /// is finally known, so this is where they are worth anything.
    ///
    /// A strike of unknown kind switches nothing on. Firing a gate because
    /// we could not tell would put a bonus in a number nobody can check.
    fn gated(
        &self,
        who: &Resolved,
        id: &str,
        trace: &mut Vec<StageNote>,
        stage: u8,
    ) -> Result<Confidence<Fixed>, ExchangeError> {
        let mut value = Self::require(who, id)?;
        if self.strike.tags.is_empty() {
            return Ok(value);
        }
        for bonus in &who.conditional {
            let fires = self.strike.tags.contains(&bonus.tag); // probe: gate-checks-tag
            if !fires || bonus.stat.as_str() != id {
                continue;
            }
            let before = *value.value();
            value = value.zip_with(bonus.value.clone(), |a, b| a + b);
            trace.push(StageNote {
                stage,
                label: "gated bonus",
                detail: format!(
                    "{} applies to a {} blow: {id} {before} +{} → {}",
                    bonus.source,
                    bonus.tag.as_str(),
                    bonus.value.value(),
                    value.value()
                ),
            });
        }
        Ok(value)
    }

    fn require(resolved: &Resolved, id: &str) -> Result<Confidence<Fixed>, ExchangeError> {
        resolved
            .stat(id)
            .cloned()
            .ok_or_else(|| ExchangeError::MissingStat(DerivedStatId::new(id)))
    }

    /// Refuses a dataset that is not the one a combatant was resolved
    /// against: the curves would be from another patch.
    fn require_same_build(&self, combatant: &Resolved) -> Result<(), ExchangeError> {
        if combatant.build == self.data.build() {
            Ok(())
        } else {
            Err(ExchangeError::DatasetMismatch {
                resolved_against: combatant.build.clone(),
                given: self.data.build().into(),
            })
        }
    }

    /// The defender's PDR at an arbitrary armour rating: the real curve,
    /// re-sampled, offset and clamped exactly as resolution would have.
    fn defender_pdr_at(&self, armor: Fixed) -> Result<Confidence<Fixed>, ExchangeError> {
        let class = self
            .data
            .class(&self.defender.class)
            .ok_or_else(|| ExchangeError::UnknownClass(self.defender.class.clone()))?;
        let pdr_id = DerivedStatId::new(self.strike.damage_type.reduction());
        let def = class
            .derived
            .iter()
            .find(|d| d.id == pdr_id)
            .ok_or_else(|| ExchangeError::NoPdrDefinition(self.defender.class.clone()))?;
        let curve = self
            .data
            .curve(&def.curve)
            .ok_or_else(|| ExchangeError::UnknownCurve(def.curve.clone()))?;

        let offset = def.offset;
        let floor = def.floor;
        // The cap in force, which a perk may have raised at resolve time.
        let cap = self.defender.caps.get(&pdr_id).copied().or(def.cap);
        Ok(curve.clone().map(move |curve| {
            let mut value = curve.sample(armor) + offset;
            if let Some(floor) = floor {
                value = value.max(floor);
            }
            if let Some(cap) = cap {
                value = value.min(cap);
            }
            value
        }))
    }

    /// How long `hits` swings take, if anything has measured the weapon.
    ///
    /// Action Speed makes you faster, so it divides: at +100% a swing takes
    /// half the time. Multiplying by `(100 - speed)` would make +100% take
    /// no time at all, which is the kind of formula that looks right until
    /// someone reaches the number that breaks it.
    ///
    /// Every swing costs its own time, so `hits` swings cost `hits × t`.
    /// Whether the first blow lands at zero or after one swing decides a
    /// close race by one interval, and nothing here has measured which —
    /// recorded in `data/README.md` rather than settled by preference.
    fn time_for(&self, hits: i64) -> Option<Confidence<Fixed>> {
        let weapon = self
            .data
            .item(self.strike.weapon.as_ref()?)?
            .weapon
            .as_ref()?;
        let swing = weapon.swing_time.clone()?;
        let speed = self.attacker.stat(well_known::ACTION_SPEED)?.clone();
        let count = Fixed::from_int(hits);
        Some(swing.zip_with(speed, move |t, percent| {
            let hundred = Fixed::from_int(100);
            (t * count).mul_div_half_even(hundred, hundred + percent)
        }))
    }

    /// How many swings of the weapon's own chain it takes to kill.
    ///
    /// `None` when the weapon has no chain recorded, when this attack is
    /// not the weapon simply swinging (a skill or a pinned swing is a
    /// question about one blow, and answering about a different one would
    /// be answering something nobody asked), or when the chain cannot kill
    /// at all.
    ///
    /// The chain repeats: swing four is swing one again. Whether the game
    /// resets a chain on a miss or after a pause is unmeasured and would
    /// only ever make this number larger, so an uninterrupted chain is the
    /// floor rather than a guess — noted in `data/README.md`.
    fn chain_to_kill(&self, health: Fixed) -> Result<Option<i64>, ExchangeError> {
        if self.strike.pinned {
            return Ok(None);
        }
        let Some(weapon) = self
            .strike
            .weapon
            .as_ref()
            .and_then(|id| self.data.item(id))
            .and_then(|item| item.weapon.as_ref())
        else {
            return Ok(None);
        };
        if weapon.combo.is_empty() {
            return Ok(None);
        }

        // Each swing costs its own nine steps. They differ only in scaling,
        // but the difference does not survive being applied at the end:
        // the flat bonus is added after the multiply, so scaling the final
        // damage would quietly overcharge every weapon carrying one.
        let mut per_swing = Vec::with_capacity(weapon.combo.len());
        for hit in &weapon.combo {
            let mut swing = self.strike.clone();
            swing.scaling = hit.scaling.clone().map(ScalingCoefficient::new);
            per_swing.push(self.steps(&swing)?.damage.value().value());
        }

        // A chain that does nothing does not kill in a very large number of
        // swings; it does not kill. Checking the whole cycle rather than
        // one swing matters for a chain that only lands on its last blow.
        let cycle: Fixed = per_swing.iter().fold(Fixed::ZERO, |a, b| a + *b);
        if cycle <= Fixed::ZERO {
            return Ok(None);
        }

        // Whole laps by division, not by walking them. A chain taking a
        // sliver off a large pool would otherwise loop millions of times to
        // arrive at a number arithmetic already knows, and a fight is not a
        // good place to find that out.
        let laps = cycle.whole_multiples_in(health).unwrap_or(0);
        let per_lap = i64::try_from(per_swing.len()).unwrap_or(i64::MAX);
        let mut dealt = cycle * Fixed::from_int(laps);
        let mut swings = laps.saturating_mul(per_lap);

        // `laps` is the floor, so what is left is less than one full lap and
        // this pass always finishes it.
        for damage in &per_swing {
            if dealt >= health {
                break;
            }
            dealt = dealt + *damage;
            swings = swings.saturating_add(1);
        }
        Ok(Some(swings))
    }

    /// Runs the nine steps in the locked order for one swing.
    ///
    /// Takes the strike rather than reading `self.strike` so a chain can
    /// run the same nine steps once per swing: the swings differ only in
    /// their scaling, and every other step has to apply to each of them
    /// identically or the chain is not the same weapon.
    fn steps(&self, strike: &Strike) -> Result<Swing, ExchangeError> {
        self.require_same_build(self.attacker)?;
        self.require_same_build(self.defender)?;
        let kind = strike.damage_type; // probe: damage-type
        let mut trace: Vec<StageNote> = Vec::new();
        // Each read picks up the gates that fire for this swing, at the step
        // that uses it — so the trace shows the gate next to the number it
        // moved rather than in a preamble nobody reads.
        let attacker_power = self.gated(self.attacker, kind.power_bonus(), &mut trace, 3)?;
        let defender_armor = self.gated(self.defender, kind.rating(), &mut trace, 5)?;
        let defender_pdr = self.gated(self.defender, kind.reduction(), &mut trace, 6)?;

        // ── 1: base damage ───────────────────────────────────────────────
        let base = strike.base.clone();
        trace.push(StageNote {
            stage: 1,
            label: "base damage",
            detail: format!("{}", base.value().value()),
        });

        // ── 2: × scaling coefficient ─────────────────────────────────────
        // A 0% coefficient means the skill's damage does not scale at all —
        // which is exactly why Sneak Attack ignores the Hide-exit penalty.
        let scaled = base.clone().zip_with(strike.scaling.clone(), apply_scaling); // probe: scaling-coefficient
        trace.push(StageNote {
            stage: 2,
            label: "scaling coefficient",
            detail: format!(
                "× {}% → {}",
                strike.scaling.value().value(),
                scaled.value().value()
            ),
        });

        // ── 3: + Physical Power Bonus (with situational adjustment) ──────
        let power = attacker_power
            .clone()
            .zip_with(self.context.power_bonus_adjust.clone(), |ppb, adjust| {
                ppb + adjust
            });
        let powered = scaled.clone().zip_with(power.clone(), |damage, percent| {
            apply_percent(damage, percent)
        });
        trace.push(StageNote {
            stage: 3,
            label: match kind {
                DamageType::Physical => "physical power bonus",
                DamageType::Magic => "magic power bonus",
            },
            detail: format!("× (100{:+})% → {}", power.value(), powered.value().value()),
        });

        // ── 4: + flat buff weapon damage ─────────────────────────────────
        let with_flat = powered
            .clone()
            .zip_with(strike.flat_bonus.clone(), |damage, flat| damage + flat);
        trace.push(StageNote {
            stage: 4,
            label: "flat weapon damage",
            detail: format!(
                "{:+} → {}",
                strike.flat_bonus.value().value(),
                with_flat.value().value()
            ),
        });

        // ── 5: defender's armor rating, debuffed then penetrated ─────────
        // Order is not arbitrary. The bonus is defender-side state — what
        // their armour is worth right now, before anyone specific swings —
        // so it composes first. Penetration is a property of the strike and
        // subtracts from whatever armour it meets
        // (`docs/adr/ADR-006-amendment-item-armor-debuff.md`).
        //
        // The recomposition is physical by nature: an Item Armor Rating
        // Bonus applies to *armour*, which is what the game calls it. Magic
        // resistance has no such multiplier and no two buckets, so a magic
        // strike meets the rating as it resolved. Running the armour
        // composition for a magic attack reduced it by the defender's
        // armour rating — the right shape reading the wrong stat, which is
        // the sort of thing only running it finds.
        let composition = &self.defender.armor;
        let mut net_bonus = composition.bonus.clone();
        for mod_value in self.context.item_armor_bonus_mods.values() {
            net_bonus = net_bonus.zip_with(mod_value.clone(), |a, b| a + b);
        }
        // The debuff's base is the item bucket, exactly as resolution's was.
        let bonus_base = composition.item.clone(); // probe: debuff-item-base
        let debuffed = match kind {
            DamageType::Physical => bonus_base
                .zip_with(net_bonus.clone(), apply_item_armor_bonus)
                .zip_with(composition.other.clone(), |scaled, other| scaled + other),
            DamageType::Magic => defender_armor.clone(),
        };
        if !self.context.item_armor_bonus_mods.is_empty() && kind == DamageType::Physical {
            trace.push(StageNote {
                stage: 5,
                label: "armor debuffed",
                detail: format!(
                    "armor {} = item {} ×(100{:+})% + other {} → {} [{}]",
                    defender_armor.value(),
                    composition.item.value(),
                    net_bonus.value(),
                    composition.other.value(),
                    debuffed.value(),
                    self.context
                        .item_armor_bonus_mods
                        .iter()
                        .map(|(id, v)| format!("{}: {}", id.as_str(), v.value()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
        let armor = debuffed
            .clone()
            .map(ArmorRating::new)
            .zip_with(strike.penetration.clone(), penetrate);
        trace.push(StageNote {
            stage: 5,
            label: "penetration",
            detail: format!(
                "{} {} − pen {}% → {}",
                match kind {
                    DamageType::Physical => "armor",
                    DamageType::Magic => "magic resistance",
                },
                debuffed.value(),
                strike.penetration.value().value(),
                armor.value().value()
            ),
        });

        // ── 6: → PDR from the curve, capped ──────────────────────────────
        // The defender's resolved PDR came from this curve at their full
        // armour rating (ADR-005 stage 7). Penetration lowered the rating,
        // so the honest answer is the curve re-sampled there — not the
        // resolved PDR scaled by how much armour survived, which was wrong
        // in direction whenever PDR is negative
        // (`docs/adr/ADR-006-amendment-penetration-resampling.md`).
        let base_pdr = defender_pdr.clone().map(PdrPercent::new);
        let resampled = self.defender_pdr_at(armor.value().value())?;
        let pdr_after_pen = armor
            .clone()
            .zip_with(resampled, |_, pdr| PdrPercent::new(pdr));
        trace.push(StageNote {
            stage: 6,
            label: match kind {
                DamageType::Physical => "PDR from curve",
                DamageType::Magic => "magical DR from curve",
            },
            detail: format!(
                "{} → {} re-sampled at the penetrated rating",
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
            label: match kind {
                DamageType::Physical => "PDR mod",
                DamageType::Magic => "magical DR mod",
            },
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
        let with_true = post_reduction.zip_with(strike.true_damage.clone(), |damage, true_dmg| {
            damage + Damage::new(true_dmg.value())
        });
        trace.push(StageNote {
            stage: 8,
            label: "true damage",
            detail: format!(
                "{:+} (bypasses the reduction) → {}",
                strike.true_damage.value().value(),
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

        Ok(Swing {
            damage: final_damage,
            effective_pdr,
            trace,
        })
    }

    /// Runs the nine steps, then answers the question a player is actually
    /// asking: how many of these, and how long.
    pub fn damage(&self) -> Result<ExchangeOutcome, ExchangeError> {
        let Swing {
            damage: final_damage,
            effective_pdr,
            mut trace,
        } = self.steps(self.strike)?;

        // ── the fight, rather than the hit ───────────────────────────────
        // Damage answers "how hard"; a player is asking "how long". Both
        // come out of numbers already computed, and leaving them out was
        // asking a reader to do arithmetic the tool exists to do.
        // Health is not required to compute damage, so a class that does not
        // define it still gets an answer to the question that was asked.
        // Demanding it here turned "how hard does this hit" into "how hard
        // does this hit, and also how long is the fight" — a different
        // question, and not always askable.
        let health = self.defender.stat(well_known::HEALTH).cloned();
        let hits_to_kill = health
            .as_ref()
            .and_then(|h| final_damage.value().value().hits_to_cover(*h.value()));
        let time_to_kill = hits_to_kill.and_then(|hits| self.time_for(hits));

        // The same fight with the weapon doing what it actually does. A
        // chained weapon never lands the same blow twice in a row, so
        // "this swing, repeated" is a hit count nobody will ever take —
        // it answers the question asked and not the fight fought.
        let chain = health
            .as_ref()
            .and_then(|h| self.chain_to_kill(*h.value()).transpose())
            .transpose()?;
        let chain_time = chain.and_then(|hits| self.time_for(hits));

        if let (Some(hits), Some(health)) = (hits_to_kill, health.as_ref()) {
            trace.push(StageNote {
                stage: 9,
                label: "hits to kill",
                detail: match &time_to_kill {
                    Some(t) => format!(
                        "{} health ÷ {} → {hits} hit(s), {}s",
                        health.value(),
                        final_damage.value().value(),
                        t.value()
                    ),
                    None => format!(
                        "{} health ÷ {} → {hits} hit(s); no swing time for this weapon, so how long that takes is unknown",
                        health.value(),
                        final_damage.value().value()
                    ),
                },
            });
        }
        if let Some(hits) = chain {
            trace.push(StageNote {
                stage: 9,
                label: "swings to kill",
                detail: match &chain_time {
                    Some(t) => format!(
                        "running the weapon's chain: {hits} swing(s), {}s",
                        t.value()
                    ),
                    None => format!("running the weapon's chain: {hits} swing(s)"),
                },
            });
        }

        Ok(ExchangeOutcome {
            damage: final_damage,
            effective_pdr,
            hits_to_kill,
            time_to_kill,
            chain_to_kill: chain,
            chain_time_to_kill: chain_time,
            trace,
        })
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

    use alloc::string::ToString;
    use alloc::vec;

    use proptest::prelude::*;

    use super::*;
    use crate::confidence::ConfidenceLevel;
    use crate::curve::Curve;
    use crate::derived::{DerivedStatDef, RatingInput};
    use crate::ids::{ClassId, CurveId, ItemId};
    use crate::loadout::{GearPiece, Loadout, PartyBuffs, Slot, Weapons};
    use crate::resolve::resolve;
    use crate::schema::{ClassDef, InMemoryDataset, ItemDef};
    use alloc::collections::BTreeMap;

    const BUILD: &str = "test.build";

    fn fx(units: i64) -> Fixed {
        Fixed::from_int(units)
    }

    /// Armour rating to PDR: −20% at 0, 30% at 100, 90% at 400. Chosen so the
    /// values the tests need are exact — 0 armour gives −20%, 40 gives 0%,
    /// 150/200/250 give 40/50/60% — and so the curve has a negative region,
    /// which is where the old rescaling went wrong.
    fn pdr_curve() -> Curve {
        Curve::linear(vec![(fx(0), fx(-20)), (fx(100), fx(30)), (fx(400), fx(90))]).unwrap()
    }

    /// A dataset with one class whose only derived stat is PDR, seeded from
    /// gear-sourced armour rating, plus armour pieces at the ratings the
    /// tests want.
    fn dataset() -> InMemoryDataset {
        let mut data = InMemoryDataset::new(BUILD);
        let mut weights = BTreeMap::new();
        weights.insert(
            RatingInput::Derived(DerivedStatId::new(well_known::ARMOR_RATING)),
            Fixed::ONE,
        );
        data.insert_class(ClassDef {
            id: ClassId::new("class.test"),
            name: "Test".to_string(),
            base_attributes: Confidence::Verified(crate::schema::AttributeBlock::default()),
            derived: vec![
                DerivedStatDef {
                    id: DerivedStatId::new(well_known::PDR),
                    weights,
                    curve: CurveId::new("curve.pdr"),
                    offset: Fixed::ZERO,
                    floor: None,
                    cap: Some(fx(60)),
                },
                DerivedStatDef {
                    id: DerivedStatId::new(well_known::PHYSICAL_POWER_BONUS),
                    weights: BTreeMap::from([(
                        RatingInput::Attribute(crate::schema::AttributeKind::Strength),
                        Fixed::ONE,
                    )]),
                    curve: CurveId::new("curve.flat"),
                    offset: Fixed::ZERO,
                    floor: None,
                    cap: None,
                },
            ],
        });
        data.insert_curve(CurveId::new("curve.pdr"), Confidence::Verified(pdr_curve()));
        // Physical power bonus stays 0 for every attribute value, so the
        // tests isolate the defensive chain.
        data.insert_curve(
            CurveId::new("curve.flat"),
            Confidence::Verified(Curve::linear(vec![(fx(0), fx(0)), (fx(100), fx(0))]).unwrap()),
        );
        data.insert_perk(crate::schema::PerkDef {
            id: crate::ids::PerkId::new("perk.test.mastery"),
            name: "Test Mastery".to_string(),
            required_classes: Vec::new(),
            effects: vec![crate::schema::StackedEffect::once(Confidence::Verified(
                crate::schema::Effect::ItemArmorBonus(fx(15)),
            ))],
        });
        for rating in [0i64, 40, 150, 200, 250] {
            data.insert_item(ItemDef {
                id: ItemId::new(alloc::format!("item.armor_{rating}")),
                name: alloc::format!("Armor {rating}"),
                rarity: None,
                required_classes: Vec::new(),
                slot: None,
                attributes: None,
                grants: BTreeMap::from([(
                    DerivedStatId::new(well_known::ARMOR_RATING),
                    Confidence::Verified(fx(rating)),
                )]),
                move_speed_add: None,
                weapon: None,
            });
        }
        data
    }

    /// A combatant wearing the piece with the given armour rating.
    fn combatant(data: &InMemoryDataset, armor: i64) -> Resolved {
        let loadout = Loadout {
            name: alloc::format!("armor-{armor}"),
            class: ClassId::new("class.test"),
            perks: vec![],
            skills: vec![],
            gear: vec![GearPiece {
                slot: Slot::Legs,
                id: ItemId::new(alloc::format!("item.armor_{armor}")),
                rolls: vec![],
            }],
            weapons: Weapons::default(),
            stacks: BTreeMap::new(),
            party: PartyBuffs::default(),
        };
        resolve(&loadout, data).expect("test loadout resolves")
    }

    /// A defender wearing `armor` on the piece with `enchant` rolled onto
    /// the same copy, optionally holding the +15% Item Armor Rating Bonus
    /// perk. The two buckets differ, which is what the debuff tests need.
    fn layered(data: &InMemoryDataset, armor: i64, enchant: i64, mastery: bool) -> Resolved {
        let loadout = Loadout {
            name: alloc::format!("layered-{armor}-{enchant}"),
            class: ClassId::new("class.test"),
            perks: if mastery {
                vec![crate::ids::PerkId::new("perk.test.mastery")]
            } else {
                vec![]
            },
            skills: vec![],
            gear: vec![GearPiece {
                slot: Slot::Legs,
                id: ItemId::new(alloc::format!("item.armor_{armor}")),
                rolls: vec![crate::loadout::Roll::Derived(
                    DerivedStatId::new(well_known::ARMOR_RATING),
                    fx(enchant),
                )],
            }],
            weapons: Weapons::default(),
            stacks: BTreeMap::new(),
            party: PartyBuffs::default(),
        };
        resolve(&loadout, data).expect("layered loadout resolves")
    }

    /// Weakpoint Attack at the given percentage points, keyed by its ability.
    fn weakpoint(points: i64) -> BTreeMap<AbilityId, Confidence<Fixed>> {
        BTreeMap::from([(
            AbilityId::new("skill.rogue.weakpoint_attack"),
            Confidence::Verified(fx(points)),
        )])
    }

    fn debuff(mods: BTreeMap<AbilityId, Confidence<Fixed>>) -> ExchangeContext {
        ExchangeContext {
            item_armor_bonus_mods: mods,
            ..ExchangeContext::default()
        }
    }

    fn strike(base: i64, scaling: i64, flat: i64, pen: i64, true_dmg: i64) -> Strike {
        Strike {
            pinned: false,
            tags: BTreeSet::new(),
            weapon: None,
            damage_type: DamageType::Physical,
            base: Confidence::Verified(Damage::new(fx(base))),
            scaling: Confidence::Verified(ScalingCoefficient::new(fx(scaling))),
            flat_bonus: Confidence::Verified(Damage::new(fx(flat))),
            penetration: Confidence::Verified(ArmorPen::new(fx(pen))),
            true_damage: Confidence::Verified(TrueDamage::new(fx(true_dmg))),
        }
    }

    fn damage_of(
        data: &InMemoryDataset,
        attacker: &Resolved,
        defender: &Resolved,
        strike: &Strike,
        context: &ExchangeContext,
    ) -> Fixed {
        Exchange::new(attacker, defender, strike, context, data)
            .damage()
            .expect("test combatants carry every required stat")
            .damage
            .value()
            .value()
    }

    #[test]
    fn plain_swing_walks_the_whole_chain() {
        // Armour 40 sits exactly at 0% PDR, so nothing is reduced.
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 40));
        let s = strike(20, 100, 0, 0, 0);
        assert_eq!(
            damage_of(&data, &a, &d, &s, &ExchangeContext::default()),
            fx(20)
        );
    }

    #[test]
    fn sneak_attack_zero_scaling_ignores_the_hide_penalty() {
        // THE mechanic the scaling_ignored probe protects. Sneak Attack is
        // flat damage (step 4) with 0% scaling (step 2), so the Hide-exit
        // −30% Physical Power Bonus cannot touch it.
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 40));
        let sneak = Strike {
            pinned: false,
            tags: BTreeSet::new(),
            weapon: None,
            damage_type: DamageType::Physical,
            base: Confidence::Verified(Damage::new(fx(10))),
            scaling: Confidence::Verified(ScalingCoefficient::new(Fixed::ZERO)),
            flat_bonus: Confidence::Verified(Damage::new(fx(15))),
            penetration: Confidence::Verified(ArmorPen::new(Fixed::ZERO)),
            true_damage: Confidence::Verified(TrueDamage::new(Fixed::ZERO)),
        };
        let neutral = ExchangeContext::default();
        let hide_exit = ExchangeContext {
            power_bonus_adjust: Confidence::Verified(fx(-30)),
            ..ExchangeContext::default()
        };
        let without = damage_of(&data, &a, &d, &sneak, &neutral);
        let with_penalty = damage_of(&data, &a, &d, &sneak, &hide_exit);
        assert_eq!(without, fx(15), "0% scaling: only the flat 15 survives");
        assert_eq!(
            with_penalty, without,
            "0% scaling must be immune to the Hide-exit power penalty"
        );

        let scaling_strike = strike(15, 100, 0, 0, 0);
        assert!(
            damage_of(&data, &a, &d, &scaling_strike, &hide_exit)
                < damage_of(&data, &a, &d, &scaling_strike, &neutral)
        );
    }

    #[test]
    fn true_damage_lands_after_reduction() {
        // Armour 200 is 50% PDR: 20 base → 10, +5 true → 15. Adding it
        // before reduction would give (20+5) × 0.5 = 12.5.
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 200));
        let s = strike(20, 100, 0, 0, 5);
        assert_eq!(
            damage_of(&data, &a, &d, &s, &ExchangeContext::default()),
            fx(15)
        );
    }

    #[test]
    fn pdr_mod_is_multiplicative_not_additive() {
        // Armour 250 is 60% PDR. Lethal Mark −30 gives effective 42%, so
        // damage × 0.58. The additive reading would give 30% and × 0.70.
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 250));
        let s = strike(100, 100, 0, 0, 0);
        let marked = ExchangeContext {
            pdr_mod: Confidence::Verified(PdrMod::new(fx(-30))),
            ..ExchangeContext::default()
        };
        let outcome = Exchange::new(&a, &d, &s, &marked, &data).damage().unwrap();
        assert_eq!(outcome.effective_pdr.value().value(), fx(42));
        assert_eq!(outcome.damage.value().value(), fx(58));
    }

    #[test]
    fn penetration_re_samples_the_curve() {
        // Armour 200 is 50% PDR. 15% penetration leaves rating 170, and the
        // curve at 170 gives 44% — NOT 50% × 0.85 = 42.5%, which is what
        // rescaling the resolved PDR would have produced.
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 200));
        let s = strike(100, 100, 0, 15, 0);
        let outcome = Exchange::new(&a, &d, &s, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();
        assert_eq!(outcome.effective_pdr.value().value(), fx(44));
        assert_eq!(outcome.damage.value().value(), fx(56));
    }

    #[test]
    fn penetration_never_helps_a_defender_with_negative_pdr() {
        // The defect this amendment fixed. Armour 0 resolves to −20% PDR;
        // penetrating it must make the hit LARGER, never smaller.
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 0));
        let none = damage_of(
            &data,
            &a,
            &d,
            &strike(100, 100, 0, 0, 0),
            &ExchangeContext::default(),
        );
        let some = damage_of(
            &data,
            &a,
            &d,
            &strike(100, 100, 0, 50, 0),
            &ExchangeContext::default(),
        );
        assert_eq!(none, fx(120), "−20% PDR amplifies the hit");
        assert!(
            some >= none,
            "penetration made a hit smaller: {some} < {none}"
        );
    }

    #[test]
    fn back_attack_and_headshot_apply_at_their_steps() {
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 40));
        let s = strike(100, 100, 0, 0, 0);
        let context = ExchangeContext {
            power_bonus_adjust: Confidence::Verified(fx(30)),
            hit_location_bonus: Confidence::Verified(fx(2)),
            ..ExchangeContext::default()
        };
        // 100 × 1.30 = 130, then × 1.02 = 132.6.
        assert_eq!(
            damage_of(&data, &a, &d, &s, &context),
            "132.6".parse().unwrap()
        );
    }

    #[test]
    fn negative_pdr_increases_damage() {
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 0));
        let s = strike(100, 100, 0, 0, 0);
        assert_eq!(
            damage_of(&data, &a, &d, &s, &ExchangeContext::default()),
            fx(120)
        );
    }

    #[test]
    fn the_defenders_bonus_and_the_attackers_debuff_are_one_quantity() {
        // The game calls both Item Armor Rating Bonus, so they sum:
        // +15 - 30 = -15. Not "apply one and discard the other", and not
        // two multiplications in sequence.
        let data = dataset();
        let attacker = combatant(&data, 0);
        let defender = layered(&data, 200, 0, true);
        let strike = strike(100, 100, 0, 0, 0);

        let neutral = ExchangeContext::default();
        let debuffed = debuff(weakpoint(-30));
        // 200 x 1.15 = 230 undamaged; 200 x 0.85 = 170 under the debuff.
        assert!(
            damage_of(&data, &attacker, &defender, &strike, &debuffed)
                > damage_of(&data, &attacker, &defender, &strike, &neutral),
            "a debuff on the defender must help the attacker"
        );

        // The sum is genuinely -15 and not -30: a defender without the perk
        // drops to 200 x 0.70 = 140, strictly less armour and so strictly
        // more damage. The perk still buys something under the debuff.
        let unperked = layered(&data, 200, 0, false);
        assert!(
            damage_of(&data, &attacker, &unperked, &strike, &debuffed)
                > damage_of(&data, &attacker, &defender, &strike, &debuffed),
            "the perk must still be worth something under the debuff"
        );
    }

    #[test]
    fn the_debuff_reaches_worn_armour_and_not_its_enchantments() {
        // The three-way discrimination the resolve-side test makes, from the
        // other side: 200 item + 50 enchant under -30% is 140 + 50 = 190.
        // Debuffing the combined 250 would give 175; debuffing nothing, 250.
        let data = dataset();
        let attacker = combatant(&data, 0);
        let split = layered(&data, 200, 50, false);
        let all_item = layered(&data, 250, 0, false);
        let strike = strike(100, 100, 0, 0, 0);
        let debuffed = debuff(weakpoint(-30));

        // Both defenders resolve to the same armour rating undamaged...
        assert_eq!(
            split.stat(well_known::ARMOR_RATING).unwrap().value(),
            all_item.stat(well_known::ARMOR_RATING).unwrap().value()
        );
        // ...but the one whose armour is partly enchantment keeps more of it
        // through the debuff, and so takes strictly less damage.
        assert!(
            damage_of(&data, &attacker, &split, &strike, &debuffed)
                < damage_of(&data, &attacker, &all_item, &strike, &debuffed),
            "an enchantment must survive a debuff that worn armour does not"
        );
    }

    #[test]
    fn the_same_ability_cannot_be_applied_twice() {
        // Two rogues both landing Weakpoint apply -30, not -60. The map key
        // enforces it, so this asserts the structure was not worked around,
        // and that two different abilities still sum.
        let data = dataset();
        let attacker = combatant(&data, 0);
        let defender = layered(&data, 200, 0, false);
        let strike = strike(100, 100, 0, 0, 0);

        let mut twice = weakpoint(-30);
        twice.insert(
            AbilityId::new("skill.rogue.weakpoint_attack"),
            Confidence::Verified(fx(-30)),
        );
        assert_eq!(twice.len(), 1, "one ability, one entry");
        assert_eq!(
            damage_of(&data, &attacker, &defender, &strike, &debuff(twice)),
            damage_of(
                &data,
                &attacker,
                &defender,
                &strike,
                &debuff(weakpoint(-30))
            ),
            "a second rogue with the same skill adds nothing"
        );

        let mut two_abilities = weakpoint(-30);
        two_abilities.insert(
            AbilityId::new("perk.other.sunder"),
            Confidence::Verified(fx(-10)),
        );
        assert!(
            damage_of(&data, &attacker, &defender, &strike, &debuff(two_abilities))
                > damage_of(
                    &data,
                    &attacker,
                    &defender,
                    &strike,
                    &debuff(weakpoint(-30))
                ),
            "a different ability is a different debuff and must sum"
        );
    }

    #[test]
    fn a_magic_strike_reads_the_magic_chain() {
        // The type changes which stats the nine steps read and nothing else:
        // no step is added, removed or reordered, which is what lets ADR-006
        // stay locked while this exists.
        let data = dataset();
        let attacker = combatant(&data, 0);
        let defender = layered(&data, 200, 0, false);
        let strike = strike(100, 100, 0, 0, 0);
        let neutral = ExchangeContext::default();

        let physical = Exchange::new(&attacker, &defender, &strike, &neutral, &data)
            .damage()
            .expect("physical resolves");
        let magic_strike = Strike {
            pinned: false,
            tags: BTreeSet::new(),
            damage_type: DamageType::Magic,
            ..strike.clone()
        };
        // The test dataset defines no magic chain, so a magic strike is a
        // named error rather than a silent zero — the same way a missing
        // PDR already is.
        let refused = Exchange::new(&attacker, &defender, &magic_strike, &neutral, &data).damage();
        assert!(
            matches!(refused, Err(ExchangeError::MissingStat(_))),
            "a class with no magic chain must say so: {refused:?}"
        );
        // And the physical answer is untouched by the type existing.
        assert!(physical.damage.value().value() > Fixed::ZERO);
    }

    #[test]
    fn every_step_names_the_stat_it_actually_read() {
        // The first version of this amendment selected the right stats and
        // kept the physical labels, so a magic attack printed "armor 36"
        // while reducing by magic resistance. A trace that names the wrong
        // stat is worse than none: it invites a reader to check the wrong
        // number and find it correct.
        let data = dataset();
        let attacker = combatant(&data, 0);
        let defender = layered(&data, 200, 0, false);
        let out = Exchange::new(
            &attacker,
            &defender,
            &strike(100, 100, 0, 0, 0),
            &ExchangeContext::default(),
            &data,
        )
        .damage()
        .expect("resolves");
        let five = out.trace.iter().find(|n| n.stage == 5).expect("step 5");
        assert!(five.detail.starts_with("armor "), "{}", five.detail);
        let three = out.trace.iter().find(|n| n.stage == 3).expect("step 3");
        assert_eq!(three.label, "physical power bonus");
    }

    #[test]
    fn a_fight_is_a_race_and_not_a_damage_comparison() {
        // The reason both directions are shown. A weaker attack that needs
        // one more hit still wins if the hits come faster, and no single
        // number says that — which is the illusion a build tool has to
        // puncture rather than reproduce.
        let data = dataset();
        let attacker = combatant(&data, 0);
        let defender = combatant(&data, 0);
        let out = Exchange::new(
            &attacker,
            &defender,
            &strike(20, 100, 0, 0, 0),
            &ExchangeContext::default(),
            &data,
        )
        .damage()
        .expect("resolves");

        // The test class defines no health, so the fight is not askable and
        // says so rather than answering with a zero.
        assert_eq!(out.hits_to_kill, None);
        assert_eq!(out.time_to_kill, None);
        // ...and the damage is still there, because that was the question.
        assert!(out.damage.value().value() > Fixed::ZERO);
    }

    #[test]
    fn an_attack_that_takes_nothing_off_never_finishes() {
        // Reporting a very large number of hits would be worse than saying
        // it does not kill: one is an answer and the other is the truth.
        let data = dataset();
        let attacker = combatant(&data, 0);
        let defender = combatant(&data, 250);
        let out = Exchange::new(
            &attacker,
            &defender,
            &strike(0, 100, 0, 0, 0),
            &ExchangeContext::default(),
            &data,
        )
        .damage()
        .expect("resolves");
        assert_eq!(out.hits_to_kill, None);
    }

    #[test]
    fn a_dataset_from_another_build_is_refused() {
        // The failure the three-input form could not have. Silently using
        // another patch's curves is exactly what this guard exists to stop.
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 40));
        let other = InMemoryDataset::new("other.build");
        let s = strike(20, 100, 0, 0, 0);
        let err = Exchange::new(&a, &d, &s, &ExchangeContext::default(), &other)
            .damage()
            .unwrap_err();
        assert!(
            matches!(err, ExchangeError::DatasetMismatch { .. }),
            "{err}"
        );
    }

    /// Gives the test class a flat pool of health.
    ///
    /// The shared fixture has none, so hit counts came back `None` there —
    /// correct behaviour (damage does not require health) but useless for
    /// testing the counting itself.
    fn with_health(data: &mut InMemoryDataset, hp: i64) {
        let mut class = data
            .class(&ClassId::new("class.test"))
            .expect("test class")
            .clone();
        class.derived.push(DerivedStatDef {
            id: DerivedStatId::new(well_known::HEALTH),
            // A stat with no inputs is refused outright, and rightly — a
            // derived stat that derives from nothing is a definition
            // someone forgot to finish. So it reads Strength through the
            // curve that maps everything to zero, and the offset is the
            // whole pool.
            weights: BTreeMap::from([(
                RatingInput::Attribute(crate::schema::AttributeKind::Strength),
                Fixed::ONE,
            )]),
            curve: CurveId::new("curve.flat"),
            offset: fx(hp),
            floor: None,
            cap: None,
        });
        data.insert_class(class);
    }

    /// A weapon that swings 100/105/110, in the dataset so the exchange can
    /// find it the way a real one is found.
    fn chained_weapon(data: &mut InMemoryDataset, id: &str, base: i64) -> ItemId {
        let item = ItemId::new(id);
        data.insert_item(ItemDef {
            id: item.clone(),
            name: "Chained".to_string(),
            rarity: None,
            required_classes: Vec::new(),
            slot: None,
            attributes: None,
            grants: BTreeMap::new(),
            move_speed_add: None,
            weapon: Some(crate::schema::WeaponProfile {
                base_damage: Confidence::Verified(fx(base)),
                armor_pen: Confidence::Verified(Fixed::ZERO),
                combo: [100i64, 105, 110]
                    .into_iter()
                    .map(|pct| crate::schema::ComboHit {
                        kind: DamageTag::Blunt,
                        scaling: Confidence::Unverified(fx(pct)),
                    })
                    .collect(),
                swing_time: None,
            }),
        });
        item
    }

    #[test]
    fn the_chain_kills_sooner_than_the_same_blow_repeated() {
        // The whole reason the chain is worth counting. Repeating the first
        // swing needs five; the weapon as it is actually used needs four,
        // because swings two and three hit harder than swing one. A tool
        // that reported five would be describing a fight nobody has.
        let mut data = dataset();
        with_health(&mut data, 90);
        let weapon = chained_weapon(&mut data, "item.chained", 24);
        let (a, d) = (combatant(&data, 0), combatant(&data, 0));
        let mut s = strike(24, 100, 0, 0, 0);
        s.weapon = Some(weapon);

        let out = Exchange::new(&a, &d, &s, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();
        // One swing lands 28.8 here — the fixture's PDR curve reads -20% at
        // rating 0, so an unarmoured target takes more. Repeating it:
        // 28.8, 57.6, 86.4 — three swings leave 3.6 of the 90 standing.
        assert_eq!(out.damage.value().value(), fx(288).div_half_even(fx(10)));
        assert_eq!(out.hits_to_kill, Some(4));
        // The chain: 28.8, then 30.24, then 31.68 — 90.72 by the third,
        // and the fight is over a swing earlier. This is the difference the
        // whole feature exists to show, so it is asserted rather than
        // assumed.
        assert_eq!(out.chain_to_kill, Some(3));
    }

    #[test]
    fn a_pinned_blow_is_not_asked_about_the_chain() {
        // "What does the third swing do" is a question about one blow, and
        // answering it with a whole chain's arithmetic answers something
        // else. A skill is pinned for the same reason: it replaces the
        // swing rather than joining it.
        let mut data = dataset();
        with_health(&mut data, 100);
        let weapon = chained_weapon(&mut data, "item.chained", 24);
        let (a, d) = (combatant(&data, 0), combatant(&data, 0));
        let mut s = strike(24, 110, 0, 0, 0);
        s.weapon = Some(weapon);
        s.pinned = true;
        let out = Exchange::new(&a, &d, &s, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();
        assert_eq!(out.chain_to_kill, None);
        assert!(out.hits_to_kill.is_some());
    }

    #[test]
    fn a_chain_that_takes_nothing_off_does_not_kill_eventually() {
        // A fully resisted chain does not kill in a very large number of
        // swings. Returning a huge number here would loop for a long time
        // and then lie; the check is on the whole cycle, so a chain that
        // only lands on its last swing still counts.
        let mut data = dataset();
        with_health(&mut data, 100);
        let weapon = chained_weapon(&mut data, "item.chained", 0);
        let (a, d) = (combatant(&data, 0), combatant(&data, 250));
        let mut s = strike(0, 100, 0, 0, 0);
        s.weapon = Some(weapon);
        let out = Exchange::new(&a, &d, &s, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();
        assert_eq!(out.chain_to_kill, None);
        assert_eq!(out.hits_to_kill, None);
    }

    /// Adds a perk that grants +5 physical power, but only on the named
    /// kind of swing — Blunt Weapon Mastery's shape.
    fn gated_perk(data: &mut InMemoryDataset, tag: DamageTag) {
        data.insert_perk(crate::schema::PerkDef {
            id: crate::ids::PerkId::new("perk.test.mastery"),
            name: "Test Mastery".to_string(),
            required_classes: Vec::new(),
            effects: vec![crate::schema::StackedEffect {
                effect: Confidence::Verified(crate::schema::Effect::DerivedBonus(
                    DerivedStatId::new(well_known::PHYSICAL_POWER_BONUS),
                    fx(5),
                )),
                max_stacks: None,
                when_tag: Some(tag),
            }],
        });
    }

    /// A combatant holding the gated perk.
    fn gated_attacker(data: &InMemoryDataset) -> Resolved {
        let loadout = Loadout {
            name: "gated".to_string(),
            class: ClassId::new("class.test"),
            perks: vec![crate::ids::PerkId::new("perk.test.mastery")],
            skills: vec![],
            gear: vec![],
            weapons: Weapons::default(),
            stacks: BTreeMap::new(),
            party: PartyBuffs::default(),
        };
        resolve(&loadout, data).expect("gated loadout resolves")
    }

    #[test]
    fn a_gate_fires_on_its_kind_and_stays_shut_on_the_others() {
        let mut data = dataset();
        gated_perk(&mut data, DamageTag::Blunt);
        let (a, d) = (gated_attacker(&data), combatant(&data, 0));

        let mut blunt = strike(20, 100, 0, 0, 0);
        blunt.tags = [DamageTag::Blunt].into_iter().collect();
        let hit = Exchange::new(&a, &d, &blunt, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();

        let mut slash = strike(20, 100, 0, 0, 0);
        slash.tags = [DamageTag::Slash].into_iter().collect();
        let miss = Exchange::new(&a, &d, &slash, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();

        assert!(
            hit.damage.value().value() > miss.damage.value().value(),
            "the gate did not move the number: {} vs {}",
            hit.damage.value().value(),
            miss.damage.value().value()
        );
        assert!(explain(&hit).contains("Test Mastery applies to a blunt blow"));
        assert!(!explain(&miss).contains("gated bonus"));
    }

    #[test]
    fn a_swing_of_unknown_kind_switches_nothing_on() {
        // Firing because we could not tell would put a bonus into a number
        // nobody can check — an unarmed strike is not secretly blunt.
        let mut data = dataset();
        gated_perk(&mut data, DamageTag::Blunt);
        let (a, d) = (gated_attacker(&data), combatant(&data, 0));
        let unknown = strike(20, 100, 0, 0, 0);
        assert!(unknown.tags.is_empty());
        let out = Exchange::new(&a, &d, &unknown, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();
        assert!(!explain(&out).contains("gated bonus"));
    }

    #[test]
    fn a_gated_bonus_never_reaches_the_character_sheet() {
        // The sheet has no swing, so it cannot hold a term that depends on
        // one. Folding it in would make the sheet lie for every weapon the
        // character is not currently holding.
        let mut data = dataset();
        gated_perk(&mut data, DamageTag::Blunt);
        let a = gated_attacker(&data);
        assert_eq!(
            a.stat(well_known::PHYSICAL_POWER_BONUS).map(|v| *v.value()),
            Some(Fixed::ZERO)
        );
        assert_eq!(a.conditional.len(), 1);
        assert_eq!(a.conditional[0].tag, DamageTag::Blunt);
    }

    #[test]
    fn confidence_degrades_through_the_exchange() {
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 40));
        let mut s = strike(20, 100, 0, 0, 0);
        s.scaling = Confidence::Unverified(ScalingCoefficient::new(fx(100)));
        let outcome = Exchange::new(&a, &d, &s, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();
        assert_eq!(outcome.damage.level(), ConfidenceLevel::Unverified);
    }

    #[test]
    fn every_step_leaves_a_trace() {
        let data = dataset();
        let (a, d) = (combatant(&data, 0), combatant(&data, 40));
        let s = strike(20, 100, 0, 0, 0);
        let outcome = Exchange::new(&a, &d, &s, &ExchangeContext::default(), &data)
            .damage()
            .unwrap();
        for step in 1..=9u8 {
            assert!(
                outcome.trace.iter().any(|n| n.stage == step),
                "step {step} missing from trace"
            );
        }
        assert!(explain(&outcome).contains("scaling coefficient"));
    }

    proptest! {
        /// The invariant the defect violated, stated directly: more armour
        /// penetration never yields less damage. A golden fixture at one
        /// penetration value could not have caught a direction error.
        #[test]
        /// A bigger debuff never leaves the attacker with less damage.
        /// The sibling of the penetration property, written for the same
        /// defect class: this amendment puts a second signed quantity into
        /// the same subtraction, and that is precisely where the
        /// penetration direction bug lived.
        #[test]
        fn a_bigger_debuff_never_deals_less_damage(
            enchant in 0i64..100,
            small in 0i64..40,
            extra in 0i64..40,
            mastery in proptest::bool::ANY,
        ) {
            let data = dataset();
            let attacker = combatant(&data, 0);
            let defender = layered(&data, 200, enchant, mastery);
            let strike = strike(100, 100, 0, 0, 0);
            let at = |points: i64| {
                damage_of(&data, &attacker, &defender, &strike, &debuff(weakpoint(-points)))
            };
            prop_assert!(at(small + extra) >= at(small));
        }

        fn more_penetration_never_deals_less_damage(
            armor in prop::sample::select(vec![0i64, 40, 150, 200, 250]),
            pen_a in 0i64..=100,
            pen_b in 0i64..=100,
        ) {
            let data = dataset();
            let attacker = combatant(&data, 0);
            let defender = combatant(&data, armor);
            let (low, high) = if pen_a <= pen_b { (pen_a, pen_b) } else { (pen_b, pen_a) };
            let context = ExchangeContext::default();
            let less = damage_of(&data, &attacker, &defender, &strike(100, 100, 0, low, 0), &context);
            let more = damage_of(&data, &attacker, &defender, &strike(100, 100, 0, high, 0), &context);
            prop_assert!(
                more >= less,
                "armor {armor}: pen {high} dealt {more}, less than pen {low} at {less}"
            );
        }
    }
}
