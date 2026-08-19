//! Derived stats as weighted ratings (ADR-012).
//!
//! A derived stat is a weighted sum of inputs, run through a curve, offset
//! and clamped. One shape covers every case the game uses: the 1:1
//! conversions are a single weight of 1.00, and gear-sourced quantities like
//! Armor Rating enter as seeds rather than curve outputs.
//!
//! ```text
//! rating   = Σ (weight × input)     each product rounds half-to-even (ADR-001)
//! derived  = curve.sample(rating)
//! derived += offset                 +25 health · 300 move-speed baseline
//! derived  = clamp(derived, floor, cap)
//! ```
//!
//! Offsets are deliberately not baked into the curve points: keeping them
//! apart lets a patch diff say *"baseline changed"* rather than *"curve
//! changed"*, which is the distinction ADR-008 level 1 exists to draw.
//!
//! Inputs may name other derived stats, so evaluation runs in dependency
//! order. Cycles are a dataset error, never a runtime loop.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::fmt;

use crate::confidence::Confidence;
use crate::curve::Curve;
use crate::fixed::Fixed;
use crate::ids::{CurveId, DerivedStatId};
use crate::schema::{AttributeBlock, AttributeKind};

/// Conventional ids the pipeline and the exchange model look up by name.
/// A dataset may define any derived stats it likes; these are the ones other
/// layers reference, and their absence is an explicit error rather than a
/// silent zero.
pub mod well_known {
    /// Gear-sourced armour rating; seeded by the pipeline, not computed.
    pub const ARMOR_RATING: &str = "derived.armor_rating";
    /// Physical damage reduction in percentage points.
    pub const PDR: &str = "derived.pdr";
    /// Physical power bonus in percentage points.
    pub const PHYSICAL_POWER_BONUS: &str = "derived.physical_power_bonus";
    /// Action speed in percentage points.
    pub const ACTION_SPEED: &str = "derived.action_speed";
    /// Absolute move speed; stages 5 and 6 adjust this entry.
    pub const MOVE_SPEED: &str = "derived.move_speed";
    /// Absolute health.
    pub const HEALTH: &str = "derived.health";
}

/// What feeds a rating.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum RatingInput {
    /// One of the seven character attributes, after ADR-005 stage 3.
    Attribute(AttributeKind),
    /// Another derived stat: Armor Rating to PDR, Will to Magic Resistance
    /// to Magical Damage Reduction.
    Derived(DerivedStatId),
}

/// Definition of one derived stat (ADR-012).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DerivedStatDef {
    /// Stable identity (`derived.action_speed`).
    pub id: DerivedStatId,
    /// Weighted inputs; `BTreeMap` so the sum is byte-reproducible
    /// (ADR-001 rev 2).
    pub weights: BTreeMap<RatingInput, Fixed>,
    /// Curve mapping the rating to the stat.
    pub curve: CurveId,
    /// Flat offset applied after the curve (+25 health, 300 move speed).
    pub offset: Fixed,
    /// Lower clamp, if the stat has one.
    pub floor: Option<Fixed>,
    /// Upper clamp, if the stat has one. Perks may raise it
    /// (ADR-005 stage 7).
    pub cap: Option<Fixed>,
}

/// Why a set of derived-stat definitions could not be evaluated.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DerivedError {
    /// A definition references a curve the dataset does not contain.
    UnknownCurve(CurveId),
    /// A definition references a derived stat that is neither defined nor
    /// seeded: the stat, then the missing input.
    UnknownInput(DerivedStatId, DerivedStatId),
    /// The dependency graph contains a cycle; the listed ids take part.
    CyclicDependency(Vec<DerivedStatId>),
    /// A definition has no weights, so it has no rating to speak of.
    NoWeights(DerivedStatId),
}

impl fmt::Display for DerivedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DerivedError::UnknownCurve(id) => write!(f, "curve not in dataset: {id}"),
            DerivedError::UnknownInput(stat, input) => {
                write!(f, "{stat} references undefined derived input: {input}")
            }
            DerivedError::CyclicDependency(ids) => {
                write!(
                    f,
                    "cyclic derived-stat dependency among {} stats",
                    ids.len()
                )
            }
            DerivedError::NoWeights(id) => write!(f, "{id} has no weights"),
        }
    }
}

/// Evaluates one definition against the final attribute block and the
/// derived values computed so far.
fn evaluate(
    def: &DerivedStatDef,
    attributes: &Confidence<AttributeBlock>,
    computed: &BTreeMap<DerivedStatId, Confidence<Fixed>>,
    curve: &Confidence<Curve>,
    cap_override: Option<Fixed>,
) -> Confidence<Fixed> {
    // Rating: the weighted sum. Each product is one Fixed multiplication and
    // rounds half to even; the terms then sum exactly. For the weights the
    // game ships - quarters - the products are exact and nothing rounds.
    let mut rating = Confidence::Verified(Fixed::ZERO);
    for (input, weight) in &def.weights {
        let weight = *weight;
        let term = match input {
            RatingInput::Attribute(kind) => {
                let kind = *kind;
                attributes
                    .clone()
                    .map(|block| Fixed::from_int(i64::from(block.get(kind).points())) * weight)
            }
            RatingInput::Derived(id) => computed
                .get(id)
                .cloned()
                // Presence is guaranteed by the scheduling loop below, which
                // only evaluates a definition once every input is available.
                .unwrap_or(Confidence::Verified(Fixed::ZERO))
                .map(|value| value * weight),
        };
        rating = rating.zip_with(term, |acc, term| acc + term);
    }

    let offset = def.offset;
    let floor = def.floor;
    let cap = cap_override.or(def.cap);
    rating.zip_with(curve.clone(), move |rating, curve| {
        let mut value = curve.sample(rating) + offset;
        if let Some(floor) = floor {
            value = value.max(floor);
        }
        if let Some(cap) = cap {
            value = value.min(cap);
        }
        value
    })
}

/// Evaluates every definition in dependency order, starting from `seeded`
/// values (gear-sourced quantities such as Armor Rating).
///
/// `cap_overrides` carries perk-raised caps (ADR-005 stage 7); `curves`
/// resolves curve ids. A definition whose id is already seeded is skipped —
/// a seeded value is provided, not computed.
pub fn evaluate_all(
    defs: &[DerivedStatDef],
    attributes: &Confidence<AttributeBlock>,
    seeded: BTreeMap<DerivedStatId, Confidence<Fixed>>,
    cap_overrides: &BTreeMap<DerivedStatId, Fixed>,
    mut curves: impl FnMut(&CurveId) -> Option<Confidence<Curve>>,
) -> Result<BTreeMap<DerivedStatId, Confidence<Fixed>>, DerivedError> {
    for def in defs {
        if def.weights.is_empty() && !seeded.contains_key(&def.id) {
            return Err(DerivedError::NoWeights(def.id.clone()));
        }
    }

    let defined: BTreeSet<DerivedStatId> = defs.iter().map(|d| d.id.clone()).collect();
    let mut computed = seeded;
    let mut pending: Vec<&DerivedStatDef> = defs
        .iter()
        .filter(|d| !computed.contains_key(&d.id))
        .collect();

    // A dataset defines a handful of derived stats, so a repeated ready-scan
    // is clearer than building an explicit graph — and it can name the
    // remaining set verbatim when it stalls.
    while !pending.is_empty() {
        let mut progressed = false;
        let mut still_pending: Vec<&DerivedStatDef> = Vec::new();
        for def in pending {
            let mut ready = true;
            for input in def.weights.keys() {
                if let RatingInput::Derived(id) = input
                    && !computed.contains_key(id)
                {
                    if !defined.contains(id) {
                        return Err(DerivedError::UnknownInput(def.id.clone(), id.clone()));
                    }
                    ready = false;
                }
            }
            if ready {
                let curve = curves(&def.curve)
                    .ok_or_else(|| DerivedError::UnknownCurve(def.curve.clone()))?;
                let value = evaluate(
                    def,
                    attributes,
                    &computed,
                    &curve,
                    cap_overrides.get(&def.id).copied(),
                );
                computed.insert(def.id.clone(), value);
                progressed = true;
            } else {
                still_pending.push(def);
            }
        }
        if !progressed {
            return Err(DerivedError::CyclicDependency(
                still_pending.iter().map(|d| d.id.clone()).collect(),
            ));
        }
        pending = still_pending;
    }
    Ok(computed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::vec;

    use super::*;

    fn fx(units: i64) -> Fixed {
        Fixed::from_int(units)
    }

    fn quarter_weights() -> BTreeMap<RatingInput, Fixed> {
        // Action Speed Rating = 0.25 AGI + 0.75 DEX (the real shape).
        let mut w = BTreeMap::new();
        w.insert(
            RatingInput::Attribute(AttributeKind::Agility),
            "0.25".parse().unwrap(),
        );
        w.insert(
            RatingInput::Attribute(AttributeKind::Dexterity),
            "0.75".parse().unwrap(),
        );
        w
    }

    fn rogue_block() -> Confidence<AttributeBlock> {
        let mut block = AttributeBlock::default();
        block.add(AttributeKind::Strength, 9);
        block.add(AttributeKind::Vigor, 6);
        block.add(AttributeKind::Agility, 25);
        block.add(AttributeKind::Dexterity, 20);
        Confidence::Unverified(block)
    }

    /// The real Action Speed curve from the wiki, as a point set.
    fn action_speed_curve() -> Confidence<Curve> {
        Confidence::Unverified(
            Curve::linear(vec![
                (fx(0), fx(-38)),
                (fx(10), fx(-8)),
                (fx(13), fx(-2)),
                (fx(15), fx(0)),
                (fx(33), "22.5".parse().unwrap()),
                (fx(45), "34.5".parse().unwrap()),
                (fx(49), "37.5".parse().unwrap()),
                (fx(100), fx(63)),
            ])
            .unwrap(),
        )
    }

    #[test]
    fn hybrid_rating_reproduces_the_games_action_speed() {
        // The whole point of ADR-012: 0.25 x 25 + 0.75 x 20 = 21.25 rating,
        // 1.25% per point above 15 -> 7.8125%. A curve over Agility alone
        // cannot express this.
        let def = DerivedStatDef {
            id: DerivedStatId::new(well_known::ACTION_SPEED),
            weights: quarter_weights(),
            curve: CurveId::new("curve.action_speed"),
            offset: Fixed::ZERO,
            floor: None,
            cap: None,
        };
        let out = evaluate_all(
            &[def],
            &rogue_block(),
            BTreeMap::new(),
            &BTreeMap::new(),
            |_| Some(action_speed_curve()),
        )
        .unwrap();
        assert_eq!(
            *out[&DerivedStatId::new(well_known::ACTION_SPEED)].value(),
            "7.8125".parse().unwrap()
        );
    }

    #[test]
    fn offset_and_cap_apply_after_the_curve() {
        let mut weights = BTreeMap::new();
        weights.insert(RatingInput::Attribute(AttributeKind::Agility), Fixed::ONE);
        let def = DerivedStatDef {
            id: DerivedStatId::new(well_known::MOVE_SPEED),
            weights,
            curve: CurveId::new("curve.move_speed"),
            offset: fx(300),
            floor: None,
            cap: Some(fx(330)),
        };
        // Curve gives +6 at Agility 25 -> 306 with the baseline offset.
        let curve = Confidence::Unverified(
            Curve::linear(vec![(fx(0), fx(-10)), (fx(15), fx(0)), (fx(75), fx(36))]).unwrap(),
        );
        let out = evaluate_all(
            core::slice::from_ref(&def),
            &rogue_block(),
            BTreeMap::new(),
            &BTreeMap::new(),
            |_| Some(curve.clone()),
        )
        .unwrap();
        assert_eq!(
            *out[&DerivedStatId::new(well_known::MOVE_SPEED)].value(),
            fx(306)
        );

        // Same definition, an Agility high enough to push past the hard cap.
        let mut fast = AttributeBlock::default();
        fast.add(AttributeKind::Agility, 75);
        let capped = evaluate_all(
            &[def],
            &Confidence::Unverified(fast),
            BTreeMap::new(),
            &BTreeMap::new(),
            |_| Some(curve.clone()),
        )
        .unwrap();
        assert_eq!(
            *capped[&DerivedStatId::new(well_known::MOVE_SPEED)].value(),
            fx(330),
            "336 must clamp to the 330 hard cap"
        );
    }

    #[test]
    fn seeded_inputs_feed_dependent_stats() {
        // Armor Rating is gear-sourced: seeded, then PDR reads it.
        let mut weights = BTreeMap::new();
        weights.insert(
            RatingInput::Derived(DerivedStatId::new(well_known::ARMOR_RATING)),
            Fixed::ONE,
        );
        let pdr = DerivedStatDef {
            id: DerivedStatId::new(well_known::PDR),
            weights,
            curve: CurveId::new("curve.pdr"),
            offset: Fixed::ZERO,
            floor: None,
            cap: Some(fx(60)),
        };
        let curve = Confidence::Unverified(
            Curve::linear(vec![(fx(0), fx(-22)), (fx(100), fx(20)), (fx(400), fx(83))]).unwrap(),
        );
        let mut seeded = BTreeMap::new();
        seeded.insert(
            DerivedStatId::new(well_known::ARMOR_RATING),
            Confidence::Verified(fx(100)),
        );
        let out = evaluate_all(&[pdr], &rogue_block(), seeded, &BTreeMap::new(), |_| {
            Some(curve.clone())
        })
        .unwrap();
        assert_eq!(*out[&DerivedStatId::new(well_known::PDR)].value(), fx(20));
    }

    #[test]
    fn cap_overrides_raise_the_ceiling() {
        // Defense Mastery: 60% -> 75%, confirmed in game.
        let mut weights = BTreeMap::new();
        weights.insert(
            RatingInput::Derived(DerivedStatId::new(well_known::ARMOR_RATING)),
            Fixed::ONE,
        );
        let pdr = DerivedStatDef {
            id: DerivedStatId::new(well_known::PDR),
            weights,
            curve: CurveId::new("curve.pdr"),
            offset: Fixed::ZERO,
            floor: None,
            cap: Some(fx(60)),
        };
        let curve =
            Confidence::Unverified(Curve::linear(vec![(fx(0), fx(0)), (fx(400), fx(90))]).unwrap());
        let mut seeded = BTreeMap::new();
        seeded.insert(
            DerivedStatId::new(well_known::ARMOR_RATING),
            Confidence::Verified(fx(400)),
        );
        let base = evaluate_all(
            core::slice::from_ref(&pdr),
            &rogue_block(),
            seeded.clone(),
            &BTreeMap::new(),
            |_| Some(curve.clone()),
        )
        .unwrap();
        assert_eq!(*base[&DerivedStatId::new(well_known::PDR)].value(), fx(60));

        let mut overrides = BTreeMap::new();
        overrides.insert(DerivedStatId::new(well_known::PDR), fx(75));
        let raised = evaluate_all(&[pdr], &rogue_block(), seeded, &overrides, |_| {
            Some(curve.clone())
        })
        .unwrap();
        assert_eq!(
            *raised[&DerivedStatId::new(well_known::PDR)].value(),
            fx(75),
            "the curve must be able to exceed 65% - confirmed in game"
        );
    }

    #[test]
    fn cycles_and_dangling_references_are_dataset_errors() {
        let link = |from: &str, to: &str| {
            let mut w = BTreeMap::new();
            w.insert(RatingInput::Derived(DerivedStatId::new(to)), Fixed::ONE);
            DerivedStatDef {
                id: DerivedStatId::new(from),
                weights: w,
                curve: CurveId::new("curve.any"),
                offset: Fixed::ZERO,
                floor: None,
                cap: None,
            }
        };
        let curve =
            Confidence::Unverified(Curve::linear(vec![(fx(0), fx(0)), (fx(1), fx(1))]).unwrap());

        let cyclic = [
            link("derived.a", "derived.b"),
            link("derived.b", "derived.a"),
        ];
        assert!(matches!(
            evaluate_all(
                &cyclic,
                &rogue_block(),
                BTreeMap::new(),
                &BTreeMap::new(),
                |_| { Some(curve.clone()) }
            ),
            Err(DerivedError::CyclicDependency(_))
        ));

        let dangling = [link("derived.a", "derived.missing")];
        assert!(matches!(
            evaluate_all(
                &dangling,
                &rogue_block(),
                BTreeMap::new(),
                &BTreeMap::new(),
                |_| Some(curve.clone())
            ),
            Err(DerivedError::UnknownInput(_, _))
        ));
    }
}
