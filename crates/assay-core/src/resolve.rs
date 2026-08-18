//! The resolution pipeline (ADR-005). **Critical — the stage order is locked
//! and must not change without a new ADR.**
//!
//! ```text
//! 1. class base attributes
//! 2. + attributes from gear rolls
//! 3. + attributes from perks/skills/party      ← attribute sum FINAL here
//! 4. attributes → derived stats via curves
//! 5. + flat adds                               (move speed add, …)
//! 6. + percentage bonuses
//! 7. defensive chain: armor rating → PDR curve → cap
//! 8. situational mods stay SEPARATE            (PDR Mod — exchange layer)
//! ```
//!
//! Stage 3 before stage 4 is not negotiable: Fortified Ground and Jokester
//! together are +5 All Attributes, and applying them after the curve lookup
//! makes Physical Power Bonus systematically wrong — precisely the synergy
//! the Rogue/Fighter duo is built on. The `pipeline_order` probe breaks this
//! ordering on purpose and expects the tests to fail.
//!
//! Stage 8 stays separate because Lethal Mark is not part of the defender's
//! stat block; it is a modifier on the *exchange*. Mixing it in would make
//! "what is this player's PDR" unanswerable independently of who attacks
//! (ADR-006 owns that layer).
//!
//! Every stage is inspectable: the pipeline records a [`StageNote`] trail
//! that `assay resolve --explain` renders. Without it, "why is this number
//! like that" is unanswerable and the tool cannot be debugged against the
//! in-game character sheet.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::confidence::Confidence;
use crate::fixed::Fixed;
use crate::ids::{ClassId, CurveId, ItemId, PerkId, SkillId};
use crate::loadout::{Loadout, Roll};
use crate::schema::{AttributeBlock, AttributeKind, DatasetSource, Effect};

/// Why a loadout could not be resolved against a dataset. An unknown entity
/// is useful information in itself (ADR-009: an id can vanish in a later
/// patch — `EntityNotInDataset` with the version is part of the answer).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ResolveError {
    /// The loadout's class id is not in the dataset.
    UnknownClass(ClassId),
    /// An equipped item id is not in the dataset.
    UnknownItem(ItemId),
    /// A slotted or party perk id is not in the dataset.
    UnknownPerk(PerkId),
    /// A slotted or party skill id is not in the dataset.
    UnknownSkill(SkillId),
    /// A curve referenced by the class is not in the dataset.
    UnknownCurve(CurveId),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::UnknownClass(id) => write!(f, "class not in dataset: {id}"),
            ResolveError::UnknownItem(id) => write!(f, "item not in dataset: {id}"),
            ResolveError::UnknownPerk(id) => write!(f, "perk not in dataset: {id}"),
            ResolveError::UnknownSkill(id) => write!(f, "skill not in dataset: {id}"),
            ResolveError::UnknownCurve(id) => write!(f, "curve not in dataset: {id}"),
        }
    }
}

/// One inspectable pipeline step for `--explain` (ADR-005: every stage has
/// input, transformation and output on record).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StageNote {
    /// ADR-005 stage number (1–8).
    pub stage: u8,
    /// Short stage label.
    pub label: &'static str,
    /// What happened, rendered with exact decimals.
    pub detail: String,
}

/// The resolved stat block. Presentation-independent; the canonical encoding
/// (ADR-001 rev 2 §3) is derived from this, the trace is not part of it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolved {
    /// Final attribute block after stage 3.
    pub attributes: Confidence<AttributeBlock>,
    /// Physical Power Bonus in percent points (stage 4, Strength curve).
    pub physical_power_bonus: Confidence<Fixed>,
    /// Action Speed in percent points (stage 4, Agility curve).
    pub action_speed: Confidence<Fixed>,
    /// Move Speed, absolute, after stages 4–6.
    pub move_speed: Confidence<Fixed>,
    /// Health, absolute (stage 4, Vigor curve).
    pub health: Confidence<Fixed>,
    /// Summed armor rating entering the defensive chain (stage 7).
    pub armor_rating: Confidence<Fixed>,
    /// PDR in percent points, capped (stage 7).
    pub pdr: Confidence<Fixed>,
    /// The `--explain` trail, in stage order.
    pub trace: Vec<StageNote>,
}

/// Resolves a loadout against a dataset, in the locked stage order.
pub fn resolve(loadout: &Loadout, data: &impl DatasetSource) -> Result<Resolved, ResolveError> {
    let mut trace: Vec<StageNote> = Vec::new();
    let class = data
        .class(&loadout.class)
        .ok_or_else(|| ResolveError::UnknownClass(loadout.class.clone()))?;

    // ── Stage 1: class base attributes ───────────────────────────────────
    let base_attributes = class.base_attributes.clone();
    trace.push(StageNote {
        stage: 1,
        label: "class base attributes",
        detail: format!(
            "{} base block: {}",
            class.name,
            render_block(base_attributes.value())
        ),
    });

    // ── Stage 2: attributes from gear rolls ──────────────────────────────
    // Explicit rolls are facts of the question, so they do not degrade
    // confidence (see `loadout::Roll`).
    let mut attributes_after_gear = base_attributes;
    for piece in &loadout.armor {
        let item = data
            .item(&piece.id)
            .ok_or_else(|| ResolveError::UnknownItem(piece.id.clone()))?;
        for roll in &piece.rolls {
            if let Roll::Attribute(kind, points) = roll {
                let (kind, points) = (*kind, *points);
                attributes_after_gear = attributes_after_gear.map(|mut block| {
                    block.add(kind, points);
                    block
                });
                trace.push(StageNote {
                    stage: 2,
                    label: "gear attribute roll",
                    detail: format!("{}: {} {points:+}", item.name, kind.as_str()),
                });
            }
        }
    }

    trace.push(StageNote {
        stage: 2,
        label: "gear rolls applied",
        detail: render_block(attributes_after_gear.value()),
    });

    // ── Stage 3: attributes from perks/skills/party ──────────────────────
    // Gather every effect list once; later stages reuse the same list.
    let effects = collect_effects(loadout, data)?;
    let mut attributes_final = attributes_after_gear.clone();
    for sourced in &effects {
        match sourced.effect.value() {
            Effect::AllAttributes(points) => {
                let points = *points;
                attributes_final =
                    attributes_final.zip_with(sourced.effect.clone(), |mut block, _| {
                        block.add_all(points);
                        block
                    });
                trace.push(StageNote {
                    stage: 3,
                    label: "perk/skill/party attributes",
                    detail: format!("{}: {points:+} all attributes", sourced.name),
                });
            }
            Effect::Attribute(kind, points) => {
                let (kind, points) = (*kind, *points);
                attributes_final =
                    attributes_final.zip_with(sourced.effect.clone(), |mut block, _| {
                        block.add(kind, points);
                        block
                    });
                trace.push(StageNote {
                    stage: 3,
                    label: "perk/skill/party attributes",
                    detail: format!("{}: {} {points:+}", sourced.name, kind.as_str()),
                });
            }
            _ => {}
        }
    }
    trace.push(StageNote {
        stage: 3,
        label: "attribute sum final",
        detail: render_block(attributes_final.value()),
    });

    // ── Stage 4: attributes → derived stats via curves ───────────────────
    // The single most consequential line in the file: curves read the
    // attribute sum AFTER stage 3.
    let curve_input = &attributes_final; // probe: pipeline-order
    let physical_power_bonus = sample_attribute_curve(
        data,
        &class.curves.strength_to_physical_power,
        curve_input,
        AttributeKind::Strength,
    )?;
    let action_speed = sample_attribute_curve(
        data,
        &class.curves.agility_to_action_speed,
        curve_input,
        AttributeKind::Agility,
    )?;
    let move_speed_base = sample_attribute_curve(
        data,
        &class.curves.agility_to_move_speed,
        curve_input,
        AttributeKind::Agility,
    )?;
    let health = sample_attribute_curve(
        data,
        &class.curves.vigor_to_health,
        curve_input,
        AttributeKind::Vigor,
    )?;
    trace.push(StageNote {
        stage: 4,
        label: "curves",
        detail: format!(
            "physical power bonus {} · action speed {} · move speed {} · health {}",
            physical_power_bonus.value(),
            action_speed.value(),
            move_speed_base.value(),
            health.value()
        ),
    });

    // ── Stage 5: flat adds ───────────────────────────────────────────────
    let mut move_speed_adds: Vec<Confidence<Fixed>> = Vec::new();
    for piece in &loadout.armor {
        let item = data
            .item(&piece.id)
            .ok_or_else(|| ResolveError::UnknownItem(piece.id.clone()))?;
        if let Some(add) = &item.move_speed_add {
            move_speed_adds.push(add.clone());
        }
        for roll in &piece.rolls {
            if let Roll::MoveSpeedAdd(add) = roll {
                move_speed_adds.push(Confidence::Verified(*add));
            }
        }
    }
    for sourced in &effects {
        if let Effect::MoveSpeedAdd(add) = sourced.effect.value() {
            let add = *add;
            move_speed_adds.push(sourced.effect.clone().map(|_| add));
        }
    }
    let flat_adds = fold_sum(move_speed_adds);
    let move_speed_flat = move_speed_base.zip_with(flat_adds.clone(), |base, add| base + add);
    trace.push(StageNote {
        stage: 5,
        label: "flat adds",
        detail: format!(
            "move speed {:+} → {}",
            flat_adds.value(),
            move_speed_flat.value()
        ),
    });

    // ── Stage 6: percentage bonuses ──────────────────────────────────────
    let mut ms_bonuses: Vec<Confidence<Fixed>> = Vec::new();
    for sourced in &effects {
        if let Effect::MoveSpeedBonus(bonus) = sourced.effect.value() {
            let bonus = *bonus;
            ms_bonuses.push(sourced.effect.clone().map(|_| bonus));
        }
    }
    let bonus_sum = fold_sum(ms_bonuses);
    // percent points: ms × (100 + Σ) / 100, one rounding.
    let hundred = Fixed::from_int(100);
    let move_speed = move_speed_flat.zip_with(bonus_sum.clone(), |ms, bonus| {
        ms.mul_div_half_even(hundred + bonus, hundred)
    });
    trace.push(StageNote {
        stage: 6,
        label: "percentage bonuses",
        detail: format!(
            "move speed ×(100{:+})% → {}",
            bonus_sum.value(),
            move_speed.value()
        ),
    });

    // ── Stage 7: defensive chain ─────────────────────────────────────────
    let mut ar_parts: Vec<Confidence<Fixed>> = Vec::new();
    for piece in &loadout.armor {
        let item = data
            .item(&piece.id)
            .ok_or_else(|| ResolveError::UnknownItem(piece.id.clone()))?;
        if let Some(ar) = &item.armor_rating {
            ar_parts.push(ar.clone());
        }
    }
    let armor_rating = fold_sum(ar_parts);
    let pdr_curve = data
        .curve(&class.curves.armor_to_pdr)
        .ok_or_else(|| ResolveError::UnknownCurve(class.curves.armor_to_pdr.clone()))?;
    let pdr_uncapped = armor_rating
        .clone()
        .zip_with(pdr_curve.clone(), |ar, curve| curve.sample(ar));
    let mut cap = class.pdr_cap.clone();
    for sourced in &effects {
        if let Effect::RaisePdrCap(raised) = sourced.effect.value() {
            let raised = *raised;
            // A cap-raiser lifts the cap; it never lowers it.
            cap = cap.zip_with(sourced.effect.clone(), |current, _| current.max(raised));
        }
    }
    let pdr = pdr_uncapped.zip_with(cap.clone(), Fixed::min);
    trace.push(StageNote {
        stage: 7,
        label: "defensive chain",
        detail: format!(
            "armor rating {} → PDR {} (cap {})",
            armor_rating.value(),
            pdr.value(),
            cap.value()
        ),
    });

    // ── Stage 8: situational mods stay separate ──────────────────────────
    trace.push(StageNote {
        stage: 8,
        label: "situational mods",
        detail: String::from(
            "kept separate: PDR Mod, healing mods and debuff durations are exchange-layer (ADR-006)",
        ),
    });

    Ok(Resolved {
        attributes: attributes_final,
        physical_power_bonus,
        action_speed,
        move_speed,
        health,
        armor_rating,
        pdr,
        trace,
    })
}

/// An effect with the display name of whatever granted it (for the trace).
struct SourcedEffect {
    name: String,
    effect: Confidence<Effect>,
}

/// Gathers every effect from own perks, own skills, party perks and party
/// skills — in that order, each list in loadout declaration order. The order
/// is deterministic by construction and part of the pipeline's contract.
fn collect_effects(
    loadout: &Loadout,
    data: &impl DatasetSource,
) -> Result<Vec<SourcedEffect>, ResolveError> {
    let mut out: Vec<SourcedEffect> = Vec::new();
    for id in &loadout.perks {
        let perk = data
            .perk(id)
            .ok_or_else(|| ResolveError::UnknownPerk(id.clone()))?;
        for effect in &perk.effects {
            out.push(SourcedEffect {
                name: perk.name.clone(),
                effect: effect.clone(),
            });
        }
    }
    for id in &loadout.skills {
        let skill = data
            .skill(id)
            .ok_or_else(|| ResolveError::UnknownSkill(id.clone()))?;
        for effect in &skill.effects {
            out.push(SourcedEffect {
                name: skill.name.clone(),
                effect: effect.clone(),
            });
        }
    }
    for id in &loadout.party.perks {
        let perk = data
            .perk(id)
            .ok_or_else(|| ResolveError::UnknownPerk(id.clone()))?;
        for effect in &perk.effects {
            out.push(SourcedEffect {
                name: format!("{} (party)", perk.name),
                effect: effect.clone(),
            });
        }
    }
    for id in &loadout.party.skills {
        let skill = data
            .skill(id)
            .ok_or_else(|| ResolveError::UnknownSkill(id.clone()))?;
        for effect in &skill.effects {
            out.push(SourcedEffect {
                name: format!("{} (party)", skill.name),
                effect: effect.clone(),
            });
        }
    }
    Ok(out)
}

/// Samples a class curve at one attribute's final value, propagating both the
/// attribute block's and the curve's confidence (minimum rule).
fn sample_attribute_curve(
    data: &impl DatasetSource,
    curve_id: &CurveId,
    attributes: &Confidence<AttributeBlock>,
    kind: AttributeKind,
) -> Result<Confidence<Fixed>, ResolveError> {
    let curve = data
        .curve(curve_id)
        .ok_or_else(|| ResolveError::UnknownCurve(curve_id.clone()))?;
    Ok(attributes.clone().zip_with(curve.clone(), |block, curve| {
        curve.sample(Fixed::from_int(i64::from(block.get(kind).points())))
    }))
}

/// Sums graded values; the sum's grade is the minimum over the inputs.
/// An empty sum is `Verified(0)` — the absence of modifiers is a certain
/// fact, not a guess.
fn fold_sum(values: Vec<Confidence<Fixed>>) -> Confidence<Fixed> {
    let mut acc = Confidence::Verified(Fixed::ZERO);
    for value in values {
        acc = acc.zip_with(value, |a, b| a + b);
    }
    acc
}

/// Renders an attribute block for the trace: `STR 9 VIG 6 …`.
fn render_block(block: &AttributeBlock) -> String {
    let mut out = String::new();
    for kind in AttributeKind::ALL {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("{} {}", kind.as_str(), block.get(kind).points()));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::string::ToString;
    use alloc::vec;

    use super::*;
    use crate::confidence::ConfidenceLevel;
    use crate::curve::Curve;
    use crate::ids::{ClassId, CurveId, ItemId, PerkId, SkillId};
    use crate::loadout::{ArmorPiece, PartyBuffs};
    use crate::schema::{ClassDef, DerivedCurves, InMemoryDataset, ItemDef, PerkDef, SkillDef};
    use crate::stats::Attribute;

    fn fx(units: i64) -> Fixed {
        Fixed::from_int(units)
    }

    /// Slice test dataset. Curve shapes are test-authored placeholders,
    /// graded Unverified exactly as real wiki-derived curves will be — the
    /// tests assert pipeline MECHANICS, not game truth (that is the golden
    /// fixture arc's job, against the in-game character sheet).
    fn test_dataset() -> InMemoryDataset {
        let mut data = InMemoryDataset::new();

        let rogue_base = AttributeBlock {
            strength: Attribute::new(9),
            vigor: Attribute::new(6),
            agility: Attribute::new(25),
            dexterity: Attribute::new(20),
            will: Attribute::new(10),
            knowledge: Attribute::new(10),
            resourcefulness: Attribute::new(25),
        };
        data.insert_class(ClassDef {
            id: ClassId::new("class.rogue"),
            name: "Rogue".to_string(),
            base_attributes: Confidence::Unverified(rogue_base),
            pdr_cap: Confidence::Unverified(fx(60)),
            curves: DerivedCurves {
                strength_to_physical_power: CurveId::new("curve.test.str_to_ppb"),
                agility_to_action_speed: CurveId::new("curve.test.agi_to_as"),
                agility_to_move_speed: CurveId::new("curve.test.agi_to_ms"),
                vigor_to_health: CurveId::new("curve.test.vig_to_hp"),
                armor_to_pdr: CurveId::new("curve.test.ar_to_pdr"),
            },
        });

        // Linear placeholder curves; strictly increasing so ordering
        // assertions are meaningful.
        data.insert_curve(
            CurveId::new("curve.test.str_to_ppb"),
            Confidence::Unverified(
                Curve::linear(vec![(fx(0), fx(-32)), (fx(50), fx(68))]).unwrap(),
            ),
        );
        data.insert_curve(
            CurveId::new("curve.test.agi_to_as"),
            Confidence::Unverified(
                Curve::linear(vec![(fx(0), "-17.1875".parse().unwrap()), (fx(32), fx(14))])
                    .unwrap(),
            ),
        );
        data.insert_curve(
            CurveId::new("curve.test.agi_to_ms"),
            Confidence::Unverified(
                Curve::linear(vec![(fx(0), fx(281)), (fx(25), fx(306)), (fx(75), fx(331))])
                    .unwrap(),
            ),
        );
        data.insert_curve(
            CurveId::new("curve.test.vig_to_hp"),
            Confidence::Unverified(
                Curve::linear(vec![
                    (fx(0), fx(90)),
                    (fx(6), "108.5".parse().unwrap()),
                    (fx(50), fx(220)),
                ])
                .unwrap(),
            ),
        );
        data.insert_curve(
            CurveId::new("curve.test.ar_to_pdr"),
            Confidence::Unverified(
                Curve::linear(vec![(fx(0), fx(-22)), (fx(100), fx(20)), (fx(400), fx(83))])
                    .unwrap(),
            ),
        );

        data.insert_item(ItemDef {
            id: ItemId::new("item.dark_leather_leggings"),
            name: "Dark Leather Leggings".to_string(),
            armor_rating: Some(Confidence::Unverified(fx(36))),
            move_speed_add: Some(Confidence::Unverified(fx(-4))),
        });

        data.insert_perk(PerkDef {
            id: PerkId::new("perk.rogue.jokester"),
            name: "Jokester".to_string(),
            effects: vec![Confidence::Unverified(Effect::AllAttributes(2))],
        });
        data.insert_perk(PerkDef {
            id: PerkId::new("perk.fighter.defense_mastery"),
            name: "Defense Mastery".to_string(),
            effects: vec![Confidence::Unverified(Effect::RaisePdrCap(fx(75)))],
        });
        data.insert_skill(SkillDef {
            id: SkillId::new("skill.fighter.fortified_ground"),
            name: "Fortified Ground".to_string(),
            effects: vec![Confidence::Unverified(Effect::AllAttributes(3))],
        });

        data
    }

    fn naked_rogue() -> Loadout {
        Loadout {
            name: "naked-rogue".to_string(),
            class: ClassId::new("class.rogue"),
            perks: vec![],
            skills: vec![],
            armor: vec![],
            party: PartyBuffs::default(),
        }
    }

    #[test]
    fn naked_baseline_is_pure_curve_output() {
        let resolved = resolve(&naked_rogue(), &test_dataset()).unwrap();
        // STR 9 on the 0→−32, 50→68 line: −32 + 100·9/50 = −14.
        assert_eq!(*resolved.physical_power_bonus.value(), fx(-14));
        // AGI 25 hits the curve point exactly: 306.
        assert_eq!(*resolved.move_speed.value(), fx(306));
        // VIG 6 hits the 108.5 point exactly.
        assert_eq!(*resolved.health.value(), "108.5".parse().unwrap());
        // No armor: AR 0 → −22, below the cap.
        assert_eq!(*resolved.pdr.value(), fx(-22));
        // Wiki-graded inputs make every output Unverified.
        assert_eq!(
            resolved.physical_power_bonus.level(),
            ConfidenceLevel::Unverified
        );
        assert_eq!(resolved.pdr.level(), ConfidenceLevel::Unverified);
    }

    #[test]
    fn party_buffs_shift_curve_inputs() {
        // THE stage-3-before-stage-4 test — the pipeline_order probe breaks
        // exactly this. Jokester +2 and Fortified Ground +3 land on the
        // attribute sum BEFORE the strength curve is read: STR 9 → 14,
        // PPB −32 + 100·14/50 = −4, not −14.
        let mut loadout = naked_rogue();
        loadout.perks = vec![PerkId::new("perk.rogue.jokester")];
        loadout.party.skills = vec![SkillId::new("skill.fighter.fortified_ground")];
        let resolved = resolve(&loadout, &test_dataset()).unwrap();
        assert_eq!(
            resolved
                .attributes
                .value()
                .get(AttributeKind::Strength)
                .points(),
            14
        );
        assert_eq!(*resolved.physical_power_bonus.value(), fx(-4));
        let naked = resolve(&naked_rogue(), &test_dataset()).unwrap();
        assert!(
            *resolved.physical_power_bonus.value() > *naked.physical_power_bonus.value(),
            "party attributes must raise derived output through a rising curve"
        );
    }

    #[test]
    fn gear_rolls_and_flat_move_speed_apply_at_their_stages() {
        let mut loadout = naked_rogue();
        loadout.armor = vec![ArmorPiece {
            id: ItemId::new("item.dark_leather_leggings"),
            rolls: vec![
                Roll::Attribute(AttributeKind::Dexterity, 4),
                Roll::MoveSpeedAdd(fx(2)),
            ],
        }];
        let resolved = resolve(&loadout, &test_dataset()).unwrap();
        // DEX roll lands in the attribute block (stage 2).
        assert_eq!(
            resolved
                .attributes
                .value()
                .get(AttributeKind::Dexterity)
                .points(),
            24
        );
        // Move speed: curve 306, item −4, roll +2 → 304 (stage 5).
        assert_eq!(*resolved.move_speed.value(), fx(304));
        // AR 36 → −22 + 42·36/100 = −6.88 (stage 7).
        assert_eq!(*resolved.armor_rating.value(), fx(36));
        assert_eq!(*resolved.pdr.value(), "-6.88".parse().unwrap());
    }

    #[test]
    fn pdr_is_capped_and_cap_raisers_lift_it() {
        // Enough armor to exceed the base cap: AR 400 → curve 83.
        let heavy = ItemDef {
            id: ItemId::new("item.test_plate"),
            name: "Test Plate".to_string(),
            armor_rating: Some(Confidence::Unverified(fx(400))),
            move_speed_add: None,
        };
        let mut data = test_dataset();
        data.insert_item(heavy);
        let mut loadout = naked_rogue();
        loadout.armor = vec![ArmorPiece {
            id: ItemId::new("item.test_plate"),
            rolls: vec![],
        }];
        let capped = resolve(&loadout, &data).unwrap();
        assert_eq!(*capped.pdr.value(), fx(60), "base cap 60 must bite");

        loadout.perks = vec![PerkId::new("perk.fighter.defense_mastery")];
        let raised = resolve(&loadout, &data).unwrap();
        assert_eq!(
            *raised.pdr.value(),
            fx(75),
            "Defense Mastery raises the cap to 75"
        );
    }

    #[test]
    fn unknown_ids_are_named_errors() {
        let data = test_dataset();
        let mut loadout = naked_rogue();
        loadout.class = ClassId::new("class.bard");
        assert_eq!(
            resolve(&loadout, &data),
            Err(ResolveError::UnknownClass(ClassId::new("class.bard")))
        );
        let mut loadout = naked_rogue();
        loadout.perks = vec![PerkId::new("perk.rogue.creep")];
        assert_eq!(
            resolve(&loadout, &data),
            Err(ResolveError::UnknownPerk(PerkId::new("perk.rogue.creep")))
        );
    }

    #[test]
    fn every_stage_leaves_a_trace() {
        let mut loadout = naked_rogue();
        loadout.perks = vec![PerkId::new("perk.rogue.jokester")];
        let resolved = resolve(&loadout, &test_dataset()).unwrap();
        for stage in 1..=8u8 {
            assert!(
                resolved.trace.iter().any(|n| n.stage == stage),
                "stage {stage} missing from trace"
            );
        }
    }
}
