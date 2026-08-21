//! The resolution pipeline (ADR-005, refined by ADR-012). **The stage order
//! is locked and must not change without a new ADR.**
//!
//! ```text
//! 1.  class base attributes
//! 2.  + attributes from gear rolls
//! 3.  + attributes from perks/skills/party      ← attribute sum FINAL here
//! 4a. attributes → ratings                      weighted sums (ADR-012)
//! 4b. ratings    → derived stats                curve → offset → clamp
//! 5.  + flat adds                               (move speed add, …)
//! 6.  + percentage bonuses
//! 7.  defensive chain: armor rating seeds the PDR stat, caps may be raised
//! 8.  situational mods stay SEPARATE            (PDR Mod — exchange layer)
//! ```
//!
//! Stage 3 before stage 4 is not negotiable, and ADR-012 makes the lock
//! stricter rather than looser: a rating now folds *two* attributes into one
//! number, so applying party buffs late corrupts more than it used to.
//! Fortified Ground and Jokester together are +5 All Attributes, and the
//! Rogue/Fighter duo is built on exactly that synergy. The `pipeline_order`
//! probe breaks this ordering on purpose and expects the tests to fail.
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

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use crate::confidence::Confidence;
use crate::derived::{DerivedError, Evaluated, StatBreakdown, evaluate_all, well_known};
use crate::fixed::Fixed;
use crate::ids::{ClassId, DerivedStatId, ItemId, PerkId, SkillId};
use crate::loadout::{Loadout, Roll, Slot};
use crate::schema::{AttributeBlock, AttributeKind, DatasetSource, Effect, StackedEffect};
use crate::stats::{DamageTag, apply_item_armor_bonus};

/// Why a loadout could not be resolved against a dataset. An unknown entity
/// is useful information in itself (ADR-009: an id can vanish in a later
/// patch — naming it, with the version, is part of the answer).
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
    /// The derived-stat graph could not be evaluated (ADR-012).
    Derived(DerivedError),
    /// A piece is restricted to classes this character is not: the item,
    /// and who may carry it.
    WrongClass {
        /// The item in question.
        item: ItemId,
        /// The classes its card allows.
        allowed: Vec<ClassId>,
    },
    /// A perk slotted by a class that cannot take it: the perk and the
    /// classes that can.
    WrongClassPerk {
        /// The perk in question.
        perk: PerkId,
        /// The classes it belongs to.
        allowed: Vec<ClassId>,
    },
    /// A skill slotted by a class that cannot take it.
    WrongClassSkill {
        /// The skill in question.
        skill: SkillId,
        /// The classes it belongs to.
        allowed: Vec<ClassId>,
    },
    /// A piece is worn in a slot its own card does not name: the item,
    /// where the loadout put it, and where it belongs.
    WrongSlot {
        /// The item in question.
        item: ItemId,
        /// Where the loadout put it.
        worn: Slot,
        /// Where its card says it goes.
        belongs: Slot,
    },
    /// The loadout claims more stacks than the effect can carry.
    TooManyStacks {
        /// The perk or skill the stacks belong to.
        source: String,
        /// What the loadout asked for.
        requested: u32,
        /// What the dataset allows.
        max: u32,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::UnknownClass(id) => write!(f, "class not in dataset: {id}"),
            ResolveError::UnknownItem(id) => write!(f, "item not in dataset: {id}"),
            ResolveError::UnknownPerk(id) => write!(f, "perk not in dataset: {id}"),
            ResolveError::UnknownSkill(id) => write!(f, "skill not in dataset: {id}"),
            ResolveError::Derived(e) => write!(f, "derived stats: {e}"),
            ResolveError::WrongClassSkill { skill, allowed } => {
                let names: Vec<&str> = allowed.iter().map(ClassId::as_str).collect();
                write!(f, "{skill} may only be slotted by {}", names.join(", "))
            }
            ResolveError::WrongClassPerk { perk, allowed } => {
                let names: Vec<&str> = allowed.iter().map(ClassId::as_str).collect();
                write!(f, "{perk} may only be slotted by {}", names.join(", "))
            }
            ResolveError::WrongClass { item, allowed } => {
                let names: Vec<&str> = allowed.iter().map(ClassId::as_str).collect();
                write!(f, "{item} may only be carried by {}", names.join(", "))
            }
            ResolveError::WrongSlot {
                item,
                worn,
                belongs,
            } => write!(
                f,
                "{item} goes in the {} slot, not {}",
                belongs.as_str(),
                worn.as_str()
            ),
            ResolveError::TooManyStacks {
                source,
                requested,
                max,
            } => write!(f, "{source} stacks at most {max} times, not {requested}"),
        }
    }
}

impl From<DerivedError> for ResolveError {
    fn from(e: DerivedError) -> Self {
        ResolveError::Derived(e)
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

/// What stage 7 combined into the armour rating, kept apart.
///
/// The combined figure alone cannot be re-composed at a different Item
/// Armor Rating Bonus, because a percentage that applies to worn armour and
/// not to enchantments has no meaning once the two are one number. An
/// exchange needs exactly that when the attacker debuffs the defender
/// (ADR-006 amendment: item armor debuff), so resolution carries the parts
/// forward — the same reason `caps` is here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArmorComposition {
    /// Armour from the equipped pieces themselves: the multiplier's base.
    pub item: Confidence<Fixed>,
    /// The defender's own Item Armor Rating Bonus, in percentage points.
    pub bonus: Confidence<Fixed>,
    /// Every other source, enchantment rolls included. Outside the
    /// multiplier by definition.
    pub other: Confidence<Fixed>,
}

/// The resolved stat block. Presentation-independent; the canonical encoding
/// (ADR-001 rev 2 §3) is derived from this, the trace is not part of it.
///
/// Derived stats live in a sorted map rather than named fields (ADR-012), so
/// a dataset that defines Magic Resistance or Memory Capacity gets them in
/// the output without a code change.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolved {
    /// The class this block resolved, so a later re-evaluation can find its
    /// derived-stat definitions (ADR-006 amendment).
    pub class: ClassId,
    /// The dataset build this was resolved against. Computing an exchange
    /// against a different build is a named error, not a wrong number.
    pub build: String,
    /// Final attribute block after stage 3.
    pub attributes: Confidence<AttributeBlock>,
    /// Every derived stat the class defines, by id.
    pub derived: BTreeMap<DerivedStatId, Confidence<Fixed>>,
    /// The cap actually in force per capped stat, after any perk raised it
    /// (Defense Mastery, PDR 60% → 75%). A re-evaluation at another input
    /// must clamp the same way, and the raise came from the loadout rather
    /// than from the dataset.
    pub caps: BTreeMap<DerivedStatId, Fixed>,
    /// What stage 7 combined to reach `derived.armor_rating`.
    pub armor: ArmorComposition,
    /// How each computed stat reached its value: the rating, what the curve
    /// made of it, and the flat bonuses added on top. The game's character
    /// sheet prints exactly this decomposition, and it is what makes one of
    /// our numbers checkable against one of its numbers rather than merely
    /// comparable (ADR-012).
    pub breakdown: BTreeMap<DerivedStatId, StatBreakdown>,
    /// Bonuses that depend on the swing rather than on the character.
    ///
    /// Blunt Weapon Mastery's 5% attack power is worth nothing while its
    /// holder is swinging a sword, so folding it into `derived` would make
    /// the sheet lie for every weapon they are not currently holding. It
    /// waits here and the exchange applies the matching ones at the moment
    /// it reads the stat (ADR-006 damage-kind amendment).
    pub conditional: Vec<ConditionalBonus>,
    /// The `--explain` trail, in stage order.
    pub trace: Vec<StageNote>,
}

impl Resolved {
    /// Looks up one derived stat. Callers that require a stat must handle
    /// its absence explicitly — a missing stat is never silently zero.
    #[must_use]
    pub fn stat(&self, id: &str) -> Option<&Confidence<Fixed>> {
        self.derived.get(&DerivedStatId::new(id))
    }
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

    // ── Stage 2: attributes from gear ────────────────────────────────────
    // Two sources with different grades. What is printed on the item is a
    // dataset claim about every copy and carries the item's grade; a roll is
    // a fact about the copy in the question and does not degrade anything
    // (ADR-005 amendment: gear attributes). Folding them into one field
    // would promote one or demote the other, so they stay apart right up to
    // the addition — which commutes, so their order does not matter.
    let mut attributes_after_gear = base_attributes;
    for piece in &loadout.gear {
        let item = data
            .item(&piece.id)
            .ok_or_else(|| ResolveError::UnknownItem(piece.id.clone()))?;
        if !item.required_classes.is_empty() && !item.required_classes.contains(&loadout.class) {
            return Err(ResolveError::WrongClass {
                item: piece.id.clone(),
                allowed: item.required_classes.clone(),
            });
        }
        if let Some(belongs) = item.slot
            && belongs != piece.slot
        {
            return Err(ResolveError::WrongSlot {
                item: piece.id.clone(),
                worn: piece.slot,
                belongs,
            });
        }
        if let Some(printed) = &item.attributes {
            let delta = printed.value().clone();
            attributes_after_gear =
                attributes_after_gear
                    .clone()
                    .zip_with(printed.clone(), |mut block, _| {
                        for (kind, points) in &delta {
                            block.add(*kind, *points);
                        }
                        block
                    });
            trace.push(StageNote {
                stage: 2,
                label: "gear attributes",
                detail: format!("{}: {}", item.name, render_delta(printed.value())),
            });
        }
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
    let CollectedEffects { effects, ignored } = collect_effects(loadout, data)?;
    trace.push(StageNote {
        stage: 3,
        label: "duplicate abilities",
        detail: if ignored.is_empty() {
            String::from("none: every ability in the loadout is distinct")
        } else {
            format!(
                "already in force, so contributed nothing: {}",
                ignored.join(", ")
            )
        },
    });
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

    // ── Stage 7 (prepared): gear-sourced armour rating and cap raises ────
    // The armour rating seeds the derived graph rather than being computed
    // from it, and cap-raising perks are collected before evaluation so a
    // raised cap clamps the value the first time it is produced.
    // Two buckets, kept apart on purpose: an Item Armor Rating Bonus
    // multiplies armour that came from the pieces themselves and nothing
    // else. Summing first would erase the distinction the formula needs
    // (ADR-005 amendment: item armor bonus).
    // Gear seeds the derived graph. Two buckets per stat, because the game
    // draws that line itself: the value printed on an item is one thing and
    // `+11 Additional Armor Rating` rolled onto it is another, and only the
    // printed one is multiplied by an Item Armor Rating Bonus.
    let mut item_parts: BTreeMap<DerivedStatId, Vec<Confidence<Fixed>>> = BTreeMap::new();
    let mut other_parts: BTreeMap<DerivedStatId, Vec<Confidence<Fixed>>> = BTreeMap::new();
    for piece in &loadout.gear {
        let item = data
            .item(&piece.id)
            .ok_or_else(|| ResolveError::UnknownItem(piece.id.clone()))?;
        for (id, value) in &item.grants {
            item_parts
                .entry(id.clone())
                .or_default()
                .push(value.clone());
        }
        for roll in &piece.rolls {
            if let Roll::Derived(id, value) = roll {
                other_parts
                    .entry(id.clone())
                    .or_default()
                    .push(Confidence::Verified(*value));
            }
        }
    }
    // Abilities contribute to the same `From Bonuses` row gear does, so
    // they join the other bucket rather than getting a path of their own.
    let mut conditional: Vec<ConditionalBonus> = Vec::new();
    for sourced in &effects {
        if let Effect::DerivedBonus(id, value) = sourced.effect.value() {
            let value = *value;
            // A gated bonus is not a property of the character, so it must
            // not reach the curve or the clamp. It waits for a strike.
            if let Some(tag) = sourced.when_tag {
                conditional.push(ConditionalBonus {
                    source: sourced.name.clone(),
                    stat: id.clone(),
                    tag,
                    value: sourced.effect.clone().map(|_| value),
                });
                continue;
            }
            other_parts
                .entry(id.clone())
                .or_default()
                .push(sourced.effect.clone().map(|_| value)); // probe: ability-bonus
        }
    }
    let armor_id = DerivedStatId::new(well_known::ARMOR_RATING);
    let item_ar = fold_sum(item_parts.get(&armor_id).cloned().unwrap_or_default());
    let other_ar = fold_sum(other_parts.get(&armor_id).cloned().unwrap_or_default());

    let mut bonus_parts: Vec<Confidence<Fixed>> = Vec::new();
    for sourced in &effects {
        if let Effect::ItemArmorBonus(bonus) = sourced.effect.value() {
            let bonus = *bonus;
            bonus_parts.push(sourced.effect.clone().map(|_| bonus));
        }
    }
    let item_bonus = fold_sum(bonus_parts);

    // The multiplier's base is the item bucket and nothing else. Widening
    // it is the defect this line exists to prevent, and the probe of the
    // same name widens it to prove the tests notice.
    let bonus_base = item_ar.clone(); // probe: item-armor-bonus-base
    let armor_rating = bonus_base
        .zip_with(item_bonus.clone(), apply_item_armor_bonus)
        .zip_with(other_ar.clone(), |scaled, other| scaled + other);
    let mut cap_overrides: BTreeMap<DerivedStatId, Fixed> = BTreeMap::new();
    for sourced in &effects {
        if let Effect::RaiseCap(id, raised) = sourced.effect.value() {
            // A cap-raiser lifts the ceiling; it never lowers it.
            let entry = cap_overrides.entry(id.clone()).or_insert(*raised);
            *entry = (*entry).max(*raised);
        }
    }

    let mut seeded: BTreeMap<DerivedStatId, Confidence<Fixed>> = BTreeMap::new();
    seeded.insert(armor_id.clone(), armor_rating.clone());
    // Every other gear-granted stat is a plain sum of its two buckets. The
    // multiply belongs to armour rating alone, which is what the effect's
    // own name says: *Item Armor Rating Bonus*.
    for id in item_parts.keys().chain(other_parts.keys()) {
        if *id == armor_id || seeded.contains_key(id) {
            continue;
        }
        let mut parts = item_parts.get(id).cloned().unwrap_or_default();
        parts.extend(other_parts.get(id).cloned().unwrap_or_default());
        seeded.insert(id.clone(), fold_sum(parts));
    }

    // ── Stage 4a/4b: attributes → ratings → derived stats ────────────────
    // The single most consequential line in the file: ratings read the
    // attribute sum AFTER stage 3.
    let rating_input = &attributes_final; // probe: pipeline-order
    let Evaluated {
        values: mut derived,
        breakdown,
    } = evaluate_all(&class.derived, rating_input, seeded, &cap_overrides, |id| {
        data.curve(id).cloned()
    })?;
    trace.push(StageNote {
        stage: 4,
        label: "ratings → derived stats",
        detail: render_derived(&derived),
    });

    // ── Stage 5: flat adds ───────────────────────────────────────────────
    let mut move_speed_adds: Vec<Confidence<Fixed>> = Vec::new();
    for piece in &loadout.gear {
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
    let move_speed_id = DerivedStatId::new(well_known::MOVE_SPEED);
    if let Some(move_speed) = derived.get(&move_speed_id).cloned() {
        let adjusted = move_speed.zip_with(flat_adds.clone(), |base, add| base + add);
        trace.push(StageNote {
            stage: 5,
            label: "flat adds",
            detail: format!("move speed {:+} → {}", flat_adds.value(), adjusted.value()),
        });
        derived.insert(move_speed_id.clone(), adjusted);
    } else {
        trace.push(StageNote {
            stage: 5,
            label: "flat adds",
            detail: String::from("no move speed stat defined; nothing to adjust"),
        });
    }

    // ── Stage 6: percentage bonuses ──────────────────────────────────────
    let mut ms_bonuses: Vec<Confidence<Fixed>> = Vec::new();
    for sourced in &effects {
        if let Effect::MoveSpeedBonus(bonus) = sourced.effect.value() {
            let bonus = *bonus;
            ms_bonuses.push(sourced.effect.clone().map(|_| bonus));
        }
    }
    let bonus_sum = fold_sum(ms_bonuses);
    let hundred = Fixed::from_int(100);
    if let Some(move_speed) = derived.get(&move_speed_id).cloned() {
        // percent points: ms × (100 + Σ) / 100, one rounding.
        let adjusted = move_speed.zip_with(bonus_sum.clone(), |ms, bonus| {
            ms.mul_div_half_even(hundred + bonus, hundred)
        });
        trace.push(StageNote {
            stage: 6,
            label: "percentage bonuses",
            detail: format!(
                "move speed ×(100{:+})% → {}",
                bonus_sum.value(),
                adjusted.value()
            ),
        });
        derived.insert(move_speed_id, adjusted);
    } else {
        trace.push(StageNote {
            stage: 6,
            label: "percentage bonuses",
            detail: String::from("no move speed stat defined; nothing to adjust"),
        });
    }

    // ── Stage 7: defensive chain (evaluated at 4b, reported here) ────────
    let pdr_id = DerivedStatId::new(well_known::PDR);
    trace.push(StageNote {
        stage: 7,
        label: "defensive chain",
        detail: match derived.get(&pdr_id) {
            Some(pdr) => format!(
                "armor rating {} = item {} ×(100{:+})% + other {} → PDR {}{}",
                armor_rating.value(),
                item_ar.value(),
                item_bonus.value(),
                other_ar.value(),
                pdr.value(),
                match cap_overrides.get(&pdr_id) {
                    Some(cap) => format!(" (cap raised to {cap})"),
                    None => String::new(),
                }
            ),
            None => format!("armor rating {}; no PDR stat defined", armor_rating.value()),
        },
    });

    // ── Stage 8: situational mods stay separate ──────────────────────────
    trace.push(StageNote {
        stage: 8,
        label: "situational mods",
        detail: String::from(
            "kept separate: PDR Mod, healing mods and debuff durations are exchange-layer (ADR-006)",
        ),
    });

    // The cap in force is the definition's own, unless a perk raised it.
    let mut caps: BTreeMap<DerivedStatId, Fixed> = BTreeMap::new();
    for def in &class.derived {
        if let Some(cap) = cap_overrides.get(&def.id).copied().or(def.cap) {
            caps.insert(def.id.clone(), cap);
        }
    }

    Ok(Resolved {
        class: loadout.class.clone(),
        build: data.build().to_string(),
        attributes: attributes_final,
        derived,
        caps,
        breakdown,
        armor: ArmorComposition {
            item: item_ar,
            bonus: item_bonus,
            other: other_ar,
        },
        conditional,
        trace,
    })
}

/// An effect with the display name of whatever granted it (for the trace),
/// already scaled to the number of stacks in force.
struct SourcedEffect {
    name: String,
    effect: Confidence<Effect>,
    /// Carried through from the dataset entry, because the gate travels
    /// with the effect and not with the ability.
    when_tag: Option<DamageTag>,
}

/// Resolves one dataset effect against the loadout's stack counts.
///
/// A non-stacking effect applies once. A stacking one applies at the count
/// the loadout states; if it states none, it applies at the maximum and the
/// value is downgraded to `Unknown` with the assumption written out, so the
/// number never travels without it.
fn apply_stacks(
    entry: &StackedEffect,
    source_id: &str,
    source_name: &str,
    stacks: &BTreeMap<String, u32>,
) -> Result<(Confidence<Effect>, Option<u32>), ResolveError> {
    let Some(max) = entry.max_stacks else {
        return Ok((entry.effect.clone(), None));
    };
    match stacks.get(source_id).copied() {
        Some(requested) if requested > max => Err(ResolveError::TooManyStacks {
            source: source_id.to_string(),
            requested,
            max,
        }),
        Some(requested) => Ok((
            entry.effect.clone().map(|e| e.scaled(requested)),
            Some(requested),
        )),
        None => {
            let note = format!(
                "{source_name} resolved at {max} of {max} stacks; the loadout does not say how \
                 many are active"
            );
            let assumed = entry.effect.value().scaled(max);
            Ok((
                Confidence::Unknown {
                    assumed,
                    note: match entry.effect.note() {
                        Some(existing) => format!("{existing}; {note}"),
                        None => note,
                    },
                },
                Some(max),
            ))
        }
    }
}

/// A derived-stat bonus that only applies to one kind of swing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConditionalBonus {
    /// The ability it came from, so a trace can name it. A gate that fires
    /// unattributed is as hard to check as one that never fires.
    pub source: String,
    /// The stat it adds to.
    pub stat: DerivedStatId,
    /// The tag that switches it on — a kind for a physical effect, a
    /// school for a magical one.
    pub tag: DamageTag,
    /// How much, at the stack count already resolved.
    pub value: Confidence<Fixed>,
}

/// Everything the loadout's abilities contribute, and what was left out.
struct CollectedEffects {
    effects: Vec<SourcedEffect>,
    /// Abilities skipped because the same one was already in force, by
    /// display name. Kept so the trace can say so: a perk that silently
    /// does nothing is exactly the kind of thing a player would go on
    /// believing.
    ignored: Vec<String>,
}

/// Gathers every effect from own perks, own skills, party perks and party
/// skills — in that order, each list in loadout declaration order. The order
/// is deterministic by construction and part of the pipeline's contract.
///
/// **An ability applies once, however many people bring it.** Two Jokesters
/// in a party are +2 All Attributes, not +4, and the same holds for a perk
/// you have that a teammate also took. Order gives precedence: your own copy
/// is the one that counts, so the trace names it without the party suffix.
///
/// This is not the same question as stacking. `max_stacks` is one ability
/// applied repeatedly by its owner — Sprint at three stacks — and is a fact
/// about the moment. Duplicate *sources* of one ability are not stacks of
/// it; both mechanisms are keyed on the ability id and they do not interact.
fn collect_effects(
    loadout: &Loadout,
    data: &impl DatasetSource,
) -> Result<CollectedEffects, ResolveError> {
    let mut out: Vec<SourcedEffect> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut ignored: Vec<String> = Vec::new();
    let push_perk = |out: &mut Vec<SourcedEffect>,
                     seen: &mut BTreeSet<String>,
                     ignored: &mut Vec<String>,
                     id: &PerkId,
                     party: bool| {
        let perk = data
            .perk(id)
            .ok_or_else(|| ResolveError::UnknownPerk(id.clone()))?;
        let name = if party {
            format!("{} (party)", perk.name)
        } else {
            perk.name.clone()
        };
        if !perk.required_classes.is_empty() && !perk.required_classes.contains(&loadout.class) {
            return Err(ResolveError::WrongClassPerk {
                perk: id.clone(),
                allowed: perk.required_classes.clone(),
            });
        }
        let first_holder = seen.insert(id.as_str().to_string()); // probe: ability-dedupe
        if !first_holder {
            ignored.push(name);
            return Ok(());
        }
        for entry in &perk.effects {
            let (effect, _) = apply_stacks(entry, id.as_str(), &name, &loadout.stacks)?;
            out.push(SourcedEffect {
                name: name.clone(),
                effect,
                when_tag: entry.when_tag,
            });
        }
        Ok::<(), ResolveError>(())
    };
    for id in &loadout.perks {
        push_perk(&mut out, &mut seen, &mut ignored, id, false)?;
    }
    let push_skill = |out: &mut Vec<SourcedEffect>,
                      seen: &mut BTreeSet<String>,
                      ignored: &mut Vec<String>,
                      id: &SkillId,
                      party: bool| {
        let skill = data
            .skill(id)
            .ok_or_else(|| ResolveError::UnknownSkill(id.clone()))?;
        let name = if party {
            format!("{} (party)", skill.name)
        } else {
            skill.name.clone()
        };
        // A party member's skill is theirs to have, so only what this
        // character slots is checked against this character's class.
        if !party
            && !skill.required_classes.is_empty()
            && !skill.required_classes.contains(&loadout.class)
        {
            return Err(ResolveError::WrongClassSkill {
                skill: id.clone(),
                allowed: skill.required_classes.clone(),
            });
        }
        let first_holder = seen.insert(id.as_str().to_string()); // probe: ability-dedupe
        if !first_holder {
            ignored.push(name);
            return Ok(());
        }
        for entry in &skill.effects {
            let (effect, _) = apply_stacks(entry, id.as_str(), &name, &loadout.stacks)?;
            out.push(SourcedEffect {
                name: name.clone(),
                effect,
                when_tag: entry.when_tag,
            });
        }
        Ok::<(), ResolveError>(())
    };
    for id in &loadout.skills {
        push_skill(&mut out, &mut seen, &mut ignored, id, false)?;
    }
    for id in &loadout.party.perks {
        push_perk(&mut out, &mut seen, &mut ignored, id, true)?;
    }
    for id in &loadout.party.skills {
        push_skill(&mut out, &mut seen, &mut ignored, id, true)?;
    }
    Ok(CollectedEffects {
        effects: out,
        ignored,
    })
}

/// Renders a sparse attribute delta for the trace: `strength +2 will +9`.
fn render_delta(delta: &crate::schema::AttributeBlockDelta) -> String {
    let mut out = String::new();
    for (kind, points) in delta {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("{} {points:+}", kind.as_str()));
    }
    if out.is_empty() {
        out.push_str("nothing");
    }
    out
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

/// Renders an attribute block for the trace: `strength 9 vigor 6 …`.
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

/// Renders the derived map for the trace, in sorted id order.
fn render_derived(derived: &BTreeMap<DerivedStatId, Confidence<Fixed>>) -> String {
    let mut out = String::new();
    for (id, value) in derived {
        if !out.is_empty() {
            out.push_str(" · ");
        }
        out.push_str(&format!("{id} {}", value.value()));
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
    use crate::derived::{DerivedStatDef, RatingInput};
    use crate::ids::{ClassId, CurveId, ItemId, PerkId, SkillId};
    use crate::loadout::{GearPiece, PartyBuffs, Slot, Weapons};
    use crate::schema::{ClassDef, InMemoryDataset, ItemDef, PerkDef, SkillDef, StackedEffect};
    use crate::stats::Attribute;
    use proptest::prelude::*;

    fn fx(units: i64) -> Fixed {
        Fixed::from_int(units)
    }

    fn weights(pairs: &[(RatingInput, &str)]) -> BTreeMap<RatingInput, Fixed> {
        pairs
            .iter()
            .map(|(input, w)| (input.clone(), w.parse().unwrap()))
            .collect()
    }

    /// The real Patch 6.12 / Hotfix 123 shapes from the wiki, so the tests
    /// assert against the game rather than against invented curves.
    fn test_dataset() -> InMemoryDataset {
        let mut data = InMemoryDataset::new("test.build");

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
            derived: vec![
                DerivedStatDef {
                    id: DerivedStatId::new(well_known::PHYSICAL_POWER_BONUS),
                    weights: weights(&[(RatingInput::Attribute(AttributeKind::Strength), "1")]),
                    curve: CurveId::new("curve.ppb"),
                    offset: Fixed::ZERO,
                    floor: Some(fx(-100)),
                    cap: None,
                },
                DerivedStatDef {
                    id: DerivedStatId::new(well_known::ACTION_SPEED),
                    weights: weights(&[
                        (RatingInput::Attribute(AttributeKind::Agility), "0.25"),
                        (RatingInput::Attribute(AttributeKind::Dexterity), "0.75"),
                    ]),
                    curve: CurveId::new("curve.action_speed"),
                    offset: Fixed::ZERO,
                    floor: None,
                    cap: None,
                },
                DerivedStatDef {
                    id: DerivedStatId::new(well_known::MOVE_SPEED),
                    weights: weights(&[(RatingInput::Attribute(AttributeKind::Agility), "1")]),
                    curve: CurveId::new("curve.move_speed"),
                    offset: fx(300),
                    floor: None,
                    cap: Some(fx(330)),
                },
                DerivedStatDef {
                    id: DerivedStatId::new(well_known::HEALTH),
                    weights: weights(&[
                        (RatingInput::Attribute(AttributeKind::Strength), "0.25"),
                        (RatingInput::Attribute(AttributeKind::Vigor), "0.75"),
                    ]),
                    curve: CurveId::new("curve.health"),
                    offset: fx(25),
                    floor: None,
                    cap: None,
                },
                DerivedStatDef {
                    id: DerivedStatId::new(well_known::PDR),
                    weights: weights(&[(
                        RatingInput::Derived(DerivedStatId::new(well_known::ARMOR_RATING)),
                        "1",
                    )]),
                    curve: CurveId::new("curve.pdr"),
                    offset: Fixed::ZERO,
                    floor: None,
                    cap: Some(fx(60)),
                },
            ],
        });

        // Wiki curves (Patch 6.12 / Hotfix 123).
        data.insert_curve(
            CurveId::new("curve.ppb"),
            Confidence::Unverified(
                Curve::linear(vec![
                    (fx(0), fx(-80)),
                    (fx(5), fx(-30)),
                    (fx(7), fx(-20)),
                    (fx(11), fx(-8)),
                    (fx(15), fx(0)),
                    (fx(50), fx(35)),
                    (fx(60), fx(40)),
                    (fx(100), fx(50)),
                ])
                .unwrap(),
            ),
        );
        data.insert_curve(
            CurveId::new("curve.action_speed"),
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
            ),
        );
        data.insert_curve(
            CurveId::new("curve.move_speed"),
            Confidence::Unverified(
                Curve::linear(vec![
                    (fx(0), fx(-10)),
                    (fx(10), fx(-5)),
                    (fx(15), fx(0)),
                    (fx(75), fx(36)),
                    (fx(100), "43.5".parse().unwrap()),
                ])
                .unwrap(),
            ),
        );
        data.insert_curve(
            CurveId::new("curve.health"),
            Confidence::Unverified(
                Curve::linear(vec![
                    (fx(0), fx(70)),
                    (fx(15), fx(100)),
                    (fx(21), "110.5".parse().unwrap()),
                    (fx(44), fx(145)),
                    (fx(48), fx(150)),
                    (fx(64), fx(166)),
                    (fx(100), fx(184)),
                ])
                .unwrap(),
            ),
        );
        // Placeholder: the exact AR→PDR table is not extracted yet.
        data.insert_curve(
            CurveId::new("curve.pdr"),
            Confidence::Unverified(
                Curve::linear(vec![(fx(0), fx(-22)), (fx(100), fx(20)), (fx(400), fx(83))])
                    .unwrap(),
            ),
        );

        data.insert_item(ItemDef {
            id: ItemId::new("item.dark_leather_leggings"),
            name: "Dark Leather Leggings".to_string(),
            rarity: None,
            required_classes: Vec::new(),
            slot: None,
            attributes: None,
            grants: BTreeMap::from([(
                DerivedStatId::new(well_known::ARMOR_RATING),
                Confidence::Unverified(fx(36)),
            )]),
            move_speed_add: Some(Confidence::Unverified(fx(-4))),
            weapon: None,
        });

        data.insert_perk(PerkDef {
            id: PerkId::new("perk.rogue.jokester"),
            name: "Jokester".to_string(),
            required_classes: Vec::new(),
            effects: vec![StackedEffect::once(Confidence::Unverified(
                Effect::AllAttributes(2),
            ))],
        });
        data.insert_perk(PerkDef {
            id: PerkId::new("perk.fighter.defense_mastery"),
            name: "Defense Mastery".to_string(),
            required_classes: Vec::new(),
            effects: vec![
                StackedEffect::once(Confidence::Verified(Effect::RaiseCap(
                    DerivedStatId::new(well_known::PDR),
                    fx(75),
                ))),
                StackedEffect::once(Confidence::Unverified(Effect::ItemArmorBonus(fx(15)))),
            ],
        });
        data.insert_skill(SkillDef {
            id: SkillId::new("skill.fighter.fortified_ground"),
            name: "Fortified Ground".to_string(),
            required_classes: Vec::new(),
            effects: vec![StackedEffect::once(Confidence::Unverified(
                Effect::AllAttributes(3),
            ))],
            strike: None,
        });

        data
    }

    fn naked_rogue() -> Loadout {
        Loadout {
            name: "naked-rogue".to_string(),
            class: ClassId::new("class.rogue"),
            perks: vec![],
            skills: vec![],
            gear: vec![],
            weapons: Weapons::default(),
            stacks: BTreeMap::new(),
            party: PartyBuffs::default(),
        }
    }

    #[test]
    fn naked_baseline_matches_the_games_character_sheet() {
        // The wiki publishes these four for the Rogue at Patch 6.12 / HF123.
        // Reproducing them from the wiki's own curves is what ADR-012 is for.
        let resolved = resolve(&naked_rogue(), &test_dataset()).unwrap();
        assert_eq!(
            *resolved
                .stat(well_known::PHYSICAL_POWER_BONUS)
                .unwrap()
                .value(),
            fx(-14)
        );
        assert_eq!(
            *resolved.stat(well_known::ACTION_SPEED).unwrap().value(),
            "7.8125".parse().unwrap(),
            "the hybrid rating 0.25 AGI + 0.75 DEX is what produces this"
        );
        assert_eq!(
            *resolved.stat(well_known::HEALTH).unwrap().value(),
            "108.5".parse().unwrap()
        );
        assert_eq!(
            *resolved.stat(well_known::MOVE_SPEED).unwrap().value(),
            fx(306)
        );
    }

    #[test]
    fn action_speed_needs_dexterity_not_just_agility() {
        // Raising Dexterity alone must move Action Speed — impossible under
        // the old single-attribute model, and the reason ADR-012 exists.
        let data = test_dataset();
        let base = resolve(&naked_rogue(), &data).unwrap();
        let mut dex_loadout = naked_rogue();
        dex_loadout.gear = vec![GearPiece {
            slot: Slot::Legs,
            id: ItemId::new("item.dark_leather_leggings"),
            rolls: vec![Roll::Attribute(AttributeKind::Dexterity, 4)],
        }];
        let with_dex = resolve(&dex_loadout, &data).unwrap();
        assert!(
            with_dex.stat(well_known::ACTION_SPEED).unwrap().value()
                > base.stat(well_known::ACTION_SPEED).unwrap().value()
        );
        // 0.75 × 4 = +3 rating × 1.25%/pt = +3.75 points.
        assert_eq!(
            *with_dex.stat(well_known::ACTION_SPEED).unwrap().value(),
            "11.5625".parse().unwrap()
        );
    }

    #[test]
    fn party_buffs_shift_rating_inputs() {
        // The stage-3-before-4 lock, now over ratings. Jokester +2 and
        // Fortified Ground +3 land before the ratings are formed: STR 9 → 14.
        // On the real curve that interpolates between (11, −8) and (15, 0):
        // −8 + 8 × 3/4 = −2, up from −14 naked.
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
        assert_eq!(
            *resolved
                .stat(well_known::PHYSICAL_POWER_BONUS)
                .unwrap()
                .value(),
            fx(-2)
        );
        let naked = resolve(&naked_rogue(), &test_dataset()).unwrap();
        assert!(
            resolved
                .stat(well_known::PHYSICAL_POWER_BONUS)
                .unwrap()
                .value()
                > naked
                    .stat(well_known::PHYSICAL_POWER_BONUS)
                    .unwrap()
                    .value(),
            "party attributes must raise derived output through a rising curve"
        );
    }

    #[test]
    fn gear_rolls_and_flat_move_speed_apply_at_their_stages() {
        let mut loadout = naked_rogue();
        loadout.gear = vec![GearPiece {
            slot: Slot::Legs,
            id: ItemId::new("item.dark_leather_leggings"),
            rolls: vec![
                Roll::Attribute(AttributeKind::Dexterity, 4),
                Roll::MoveSpeedAdd(fx(2)),
            ],
        }];
        let resolved = resolve(&loadout, &test_dataset()).unwrap();
        assert_eq!(
            resolved
                .attributes
                .value()
                .get(AttributeKind::Dexterity)
                .points(),
            24
        );
        // Curve 306, item −4, roll +2 → 304 (stage 5).
        assert_eq!(
            *resolved.stat(well_known::MOVE_SPEED).unwrap().value(),
            fx(304)
        );
        assert_eq!(
            *resolved.stat(well_known::ARMOR_RATING).unwrap().value(),
            fx(36)
        );
    }

    #[test]
    fn pdr_is_capped_and_defense_mastery_raises_it_to_75() {
        // Confirmed in game: base cap 60%, 75% with Defense Mastery, and the
        // curve genuinely reaches it.
        let heavy = ItemDef {
            id: ItemId::new("item.test_plate"),
            name: "Test Plate".to_string(),
            rarity: None,
            required_classes: Vec::new(),
            slot: None,
            attributes: None,
            grants: BTreeMap::from([(
                DerivedStatId::new(well_known::ARMOR_RATING),
                Confidence::Unverified(fx(400)),
            )]),
            move_speed_add: None,
            weapon: None,
        };
        let mut data = test_dataset();
        data.insert_item(heavy);
        let mut loadout = naked_rogue();
        loadout.gear = vec![GearPiece {
            slot: Slot::Legs,
            id: ItemId::new("item.test_plate"),
            rolls: vec![],
        }];
        let capped = resolve(&loadout, &data).unwrap();
        assert_eq!(*capped.stat(well_known::PDR).unwrap().value(), fx(60));

        loadout.perks = vec![PerkId::new("perk.fighter.defense_mastery")];
        let raised = resolve(&loadout, &data).unwrap();
        assert_eq!(*raised.stat(well_known::PDR).unwrap().value(), fx(75));
    }

    #[test]
    fn the_item_bonus_multiplies_worn_armour_and_not_its_enchantments() {
        // The reason the two buckets exist. 100 armour on the piece itself
        // and 100 enchanted onto the same copy, with +50%: 100×1.5 + 100.
        // Had the multiplier reached the enchantment the answer would be
        // 300, and had it reached neither, 200 — so this one number tells
        // the three cases apart.
        let mut data = test_dataset();
        data.insert_item(ItemDef {
            id: ItemId::new("item.test_cuirass"),
            name: "Test Cuirass".to_string(),
            rarity: None,
            required_classes: Vec::new(),
            slot: None,
            attributes: None,
            grants: BTreeMap::from([(
                DerivedStatId::new(well_known::ARMOR_RATING),
                Confidence::Verified(fx(100)),
            )]),
            move_speed_add: None,
            weapon: None,
        });
        data.insert_perk(PerkDef {
            id: PerkId::new("perk.test.armor_bonus"),
            name: "Test Armor Bonus".to_string(),
            required_classes: Vec::new(),
            effects: vec![StackedEffect::once(Confidence::Verified(
                Effect::ItemArmorBonus(fx(50)),
            ))],
        });
        let mut loadout = naked_rogue();
        loadout.gear = vec![GearPiece {
            slot: Slot::Legs,
            id: ItemId::new("item.test_cuirass"),
            rolls: vec![Roll::Derived(
                DerivedStatId::new(well_known::ARMOR_RATING),
                fx(100),
            )],
        }];
        loadout.perks = vec![PerkId::new("perk.test.armor_bonus")];
        let resolved = resolve(&loadout, &data).unwrap();
        assert_eq!(
            *resolved.stat(well_known::ARMOR_RATING).unwrap().value(),
            fx(250)
        );
    }

    #[test]
    fn the_defensive_trace_shows_both_buckets() {
        // A number nobody can take apart is a number nobody can check
        // against the character sheet.
        let mut data = test_dataset();
        data.insert_item(ItemDef {
            id: ItemId::new("item.test_cuirass"),
            name: "Test Cuirass".to_string(),
            rarity: None,
            required_classes: Vec::new(),
            slot: None,
            attributes: None,
            grants: BTreeMap::from([(
                DerivedStatId::new(well_known::ARMOR_RATING),
                Confidence::Verified(fx(100)),
            )]),
            move_speed_add: None,
            weapon: None,
        });
        let mut loadout = naked_rogue();
        loadout.gear = vec![GearPiece {
            slot: Slot::Legs,
            id: ItemId::new("item.test_cuirass"),
            rolls: vec![Roll::Derived(
                DerivedStatId::new(well_known::ARMOR_RATING),
                fx(10),
            )],
        }];
        loadout.perks = vec![PerkId::new("perk.fighter.defense_mastery")];
        let resolved = resolve(&loadout, &data).unwrap();
        let note = resolved
            .trace
            .iter()
            .find(|n| n.stage == 7)
            .expect("defensive chain leaves a note");
        assert!(
            note.detail.contains("item 100") && note.detail.contains("other 10"),
            "stage 7 must show what it multiplied and what it did not: {}",
            note.detail
        );
        assert!(
            note.detail.contains("+15"),
            "and the bonus it applied: {}",
            note.detail
        );
    }

    proptest! {
        /// Armour is armour: raising the Item Armor Rating Bonus can never
        /// leave a character with less armour rating than a smaller bonus
        /// would have. The sibling of the penetration property in
        /// `exchange`, and the same class of defect it was written for.
        #[test]
        fn more_item_bonus_never_means_less_armour(
            item_ar in 0i64..2_000,
            other_ar in 0i64..2_000,
            low in 0i64..200,
            extra in 0i64..200,
        ) {
            let mut data = test_dataset();
            data.insert_item(ItemDef {
                id: ItemId::new("item.prop_plate"),
                name: "Prop Plate".to_string(),
                rarity: None,
                required_classes: Vec::new(),
                slot: None,
                attributes: None,
                grants: BTreeMap::from([(DerivedStatId::new(well_known::ARMOR_RATING), Confidence::Verified(fx(item_ar)))]),
                move_speed_add: None,
                weapon: None,
            });
            let build = |bonus: i64| {
                let mut data = data.clone();
                data.insert_perk(PerkDef {
                    id: PerkId::new("perk.prop.bonus"),
                    name: "Prop Bonus".to_string(),
                    required_classes: Vec::new(),
                    effects: vec![StackedEffect::once(Confidence::Verified(
                        Effect::ItemArmorBonus(fx(bonus)),
                    ))],
                });
                let mut loadout = naked_rogue();
                loadout.gear = vec![GearPiece {
                    slot: Slot::Legs,
                    id: ItemId::new("item.prop_plate"),
                    rolls: vec![Roll::Derived(DerivedStatId::new(well_known::ARMOR_RATING), fx(other_ar))],
                }];
                loadout.perks = vec![PerkId::new("perk.prop.bonus")];
                *resolve(&loadout, &data)
                    .unwrap()
                    .stat(well_known::ARMOR_RATING)
                    .unwrap()
                    .value()
            };
            prop_assert!(build(low + extra) >= build(low));
        }
    }

    #[test]
    fn one_ability_applies_once_however_many_people_bring_it() {
        // Two Jokesters in a party are +2 All Attributes, not +4. The perk
        // is the same perk; a second holder of it is not a second copy.
        let data = test_dataset();
        let mut solo = naked_rogue();
        solo.perks = vec![PerkId::new("perk.rogue.jokester")];
        let mut shared = solo.clone();
        shared.party.perks = vec![PerkId::new("perk.rogue.jokester")];
        assert_eq!(
            resolve(&shared, &data).unwrap().attributes.value().strength,
            resolve(&solo, &data).unwrap().attributes.value().strength,
            "a teammate holding your own perk must change nothing"
        );

        // And the trace has to say the party copy did nothing, or the player
        // goes on believing it helped.
        let resolved = resolve(&shared, &data).unwrap();
        let note = resolved
            .trace
            .iter()
            .find(|n| n.label == "duplicate abilities")
            .expect("stage 3 reports duplicates");
        assert!(
            note.detail.contains("Jokester (party)"),
            "the ignored ability must be named: {}",
            note.detail
        );
    }

    #[test]
    fn distinct_abilities_still_both_apply() {
        // The guard against over-correcting: de-duplication keys on the
        // ability, so two different ones must survive it untouched.
        let data = test_dataset();
        let mut loadout = naked_rogue();
        loadout.perks = vec![PerkId::new("perk.rogue.jokester")];
        loadout.party.skills = vec![SkillId::new("skill.fighter.fortified_ground")];
        let both = resolve(&loadout, &data).unwrap();
        let mut alone = naked_rogue();
        alone.perks = vec![PerkId::new("perk.rogue.jokester")];
        let one = resolve(&alone, &data).unwrap();
        assert!(
            both.attributes.value().strength.points() > one.attributes.value().strength.points(),
            "Fortified Ground is a different ability and must still land"
        );
    }

    #[test]
    fn every_computed_stat_can_say_what_it_was_made_of() {
        // The game prints each stat as a total over two rows, and a total
        // that cannot be taken apart cannot be checked against those rows.
        let data = test_dataset();
        let resolved = resolve(&naked_rogue(), &data).unwrap();
        for (id, parts) in &resolved.breakdown {
            let value = resolved.stat(id.as_str()).expect("stat exists");
            // Nothing here is bonused, so the curve's answer is the answer.
            assert_eq!(
                *parts.from_bonuses.value(),
                Fixed::ZERO,
                "{id} has bonuses from nowhere"
            );
            assert_eq!(*parts.from_rating.value(), *value.value(), "{id}");
        }
        assert!(
            !resolved.breakdown.is_empty(),
            "a class with derived stats must produce a breakdown"
        );
    }

    #[test]
    fn an_ability_grants_the_from_bonuses_row() {
        // The row exists on the character sheet under every stat that has
        // one; until now nothing in the model could put anything in it.
        let mut data = test_dataset();
        data.insert_perk(PerkDef {
            id: PerkId::new("perk.test.tough"),
            name: "Tough".to_string(),
            required_classes: Vec::new(),
            effects: vec![StackedEffect::once(Confidence::Verified(
                Effect::DerivedBonus(DerivedStatId::new(well_known::PDR), fx(7)),
            ))],
        });
        let mut loadout = naked_rogue();
        loadout.perks = vec![PerkId::new("perk.test.tough")];
        let resolved = resolve(&loadout, &data).unwrap();

        let pdr = DerivedStatId::new(well_known::PDR);
        let parts = resolved.breakdown.get(&pdr).expect("pdr breakdown");
        assert_eq!(*parts.from_bonuses.value(), fx(7));
        // And it lands where the sheet says: on the result, not the rating.
        let bare = resolve(&naked_rogue(), &data).unwrap();
        assert_eq!(
            *resolved.stat(well_known::PDR).unwrap().value(),
            *bare.stat(well_known::PDR).unwrap().value() + fx(7)
        );
    }

    #[test]
    fn a_bonus_cannot_push_a_stat_past_its_cap() {
        // The clamp applies to the sum. Adding after it would let a perk
        // hand out reduction the game does not allow, and clamping twice is
        // not clamping once when the bonus is negative.
        let mut data = test_dataset();
        data.insert_perk(PerkDef {
            id: PerkId::new("perk.test.enormous"),
            name: "Enormous".to_string(),
            required_classes: Vec::new(),
            effects: vec![StackedEffect::once(Confidence::Verified(
                Effect::DerivedBonus(DerivedStatId::new(well_known::PDR), fx(500)),
            ))],
        });
        let mut loadout = naked_rogue();
        loadout.perks = vec![PerkId::new("perk.test.enormous")];
        let resolved = resolve(&loadout, &data).unwrap();
        assert_eq!(*resolved.stat(well_known::PDR).unwrap().value(), fx(60));
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
    fn confidence_survives_the_rating_model() {
        let resolved = resolve(&naked_rogue(), &test_dataset()).unwrap();
        // Wiki-graded base attributes and curves make every output unverified.
        assert_eq!(
            resolved.stat(well_known::ACTION_SPEED).unwrap().level(),
            ConfidenceLevel::Unverified
        );
        // No armour at all is a certain fact, not a guess.
        assert_eq!(
            resolved.stat(well_known::ARMOR_RATING).unwrap().level(),
            ConfidenceLevel::Verified
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
