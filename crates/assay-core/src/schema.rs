//! Schema types for versioned datasets (ADR-000 rev 2, ADR-004).
//!
//! These live in the core — not in `assay-data` — so the `no_std` gate is
//! also the dependency gate: `assay-data` does I/O and hands owned structures
//! in; the core defines what those structures are. Everything the core needs
//! from the outside world passes through [`DatasetSource`]. The core does not
//! know that files exist.
//!
//! Slice scope, stated honestly: definitions carry the fields the v1 vertical
//! slice resolves (fixed stats, attribute grants, a small effect vocabulary).
//! Per-rarity modifier ranges and roll-count modelling (ADR-004) land with
//! the real dataset arc; the loadout's explicit rolls already flow through.

use alloc::string::String;
use alloc::vec::Vec;

use crate::confidence::Confidence;
use crate::curve::Curve;
use crate::derived::DerivedStatDef;
use crate::fixed::Fixed;
use crate::ids::{ClassId, CurveId, DerivedStatId, ItemId, PerkId, SkillId};
use crate::stats::Attribute;

/// The game's seven character attributes.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum AttributeKind {
    /// Physical damage scaling.
    Strength,
    /// Health.
    Vigor,
    /// Action speed and move speed.
    Agility,
    /// Interaction and equip speed.
    Dexterity,
    /// Buff duration and magical resistance scaling.
    Will,
    /// Spell capacity and casting.
    Knowledge,
    /// Interaction range and loot.
    Resourcefulness,
}

impl AttributeKind {
    /// All seven kinds in canonical (declaration) order.
    pub const ALL: [AttributeKind; 7] = [
        AttributeKind::Strength,
        AttributeKind::Vigor,
        AttributeKind::Agility,
        AttributeKind::Dexterity,
        AttributeKind::Will,
        AttributeKind::Knowledge,
        AttributeKind::Resourcefulness,
    ];

    /// Stable lowercase name, used in canonical encodings and datasets.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            AttributeKind::Strength => "strength",
            AttributeKind::Vigor => "vigor",
            AttributeKind::Agility => "agility",
            AttributeKind::Dexterity => "dexterity",
            AttributeKind::Will => "will",
            AttributeKind::Knowledge => "knowledge",
            AttributeKind::Resourcefulness => "resourcefulness",
        }
    }
}

/// One value per attribute. The block is the unit the pipeline sums in
/// stages 1–3 (ADR-005) before any curve lookup.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct AttributeBlock {
    /// Physical damage scaling.
    pub strength: Attribute,
    /// Health.
    pub vigor: Attribute,
    /// Action speed and move speed.
    pub agility: Attribute,
    /// Interaction and equip speed.
    pub dexterity: Attribute,
    /// Buff duration and magical resistance scaling.
    pub will: Attribute,
    /// Spell capacity and casting.
    pub knowledge: Attribute,
    /// Interaction range and loot.
    pub resourcefulness: Attribute,
}

impl AttributeBlock {
    /// Reads one attribute by kind.
    #[must_use]
    pub const fn get(&self, kind: AttributeKind) -> Attribute {
        match kind {
            AttributeKind::Strength => self.strength,
            AttributeKind::Vigor => self.vigor,
            AttributeKind::Agility => self.agility,
            AttributeKind::Dexterity => self.dexterity,
            AttributeKind::Will => self.will,
            AttributeKind::Knowledge => self.knowledge,
            AttributeKind::Resourcefulness => self.resourcefulness,
        }
    }

    /// Adds whole points to one attribute.
    pub fn add(&mut self, kind: AttributeKind, points: i32) {
        let slot = match kind {
            AttributeKind::Strength => &mut self.strength,
            AttributeKind::Vigor => &mut self.vigor,
            AttributeKind::Agility => &mut self.agility,
            AttributeKind::Dexterity => &mut self.dexterity,
            AttributeKind::Will => &mut self.will,
            AttributeKind::Knowledge => &mut self.knowledge,
            AttributeKind::Resourcefulness => &mut self.resourcefulness,
        };
        *slot = *slot + Attribute::new(points);
    }

    /// Adds whole points to every attribute (Fortified Ground +3, Jokester
    /// +2 — ADR-005 stage 3).
    pub fn add_all(&mut self, points: i32) {
        for kind in AttributeKind::ALL {
            self.add(kind, points);
        }
    }
}

/// A playable class: base attributes and the derived stats computed from
/// them (ADR-012). Definitions are referenced per class: whether classes
/// share them is a property of the dataset, not of the code.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClassDef {
    /// Stable identity (`class.rogue`).
    pub id: ClassId,
    /// Display name; may change between patches without changing identity.
    pub name: String,
    /// Base attribute block (ADR-005 stage 1).
    pub base_attributes: Confidence<AttributeBlock>,
    /// Derived-stat definitions, evaluated in dependency order at ADR-005
    /// stage 4. Caps live on the definitions, so a cap-raising perk targets
    /// one by id rather than through a dedicated field.
    pub derived: Vec<DerivedStatDef>,
}

/// What an item contributes when it is wielded rather than worn (ADR-006
/// step 1: "base damage from weapon/skill"). Values are the Rarity I base;
/// per-rarity modifier ranges remain the dataset arc's subject (ADR-004).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WeaponProfile {
    /// Physical base weapon damage (ADR-006 step 1).
    pub base_damage: Confidence<Fixed>,
    /// Armor penetration in percentage points (ADR-006 step 5).
    pub armor_pen: Confidence<Fixed>,
}

/// Fixed stats granted by an item. Per-rarity modifier ranges are the
/// dataset arc's subject; explicit loadout rolls come in on top.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ItemDef {
    /// Stable identity (`item.dark_leather_leggings`).
    pub id: ItemId,
    /// Display name.
    pub name: String,
    /// Armor rating contributed to the defensive chain, if any.
    pub armor_rating: Option<Confidence<Fixed>>,
    /// Flat move speed contribution; negative for armour and heavy weapon
    /// penalties alike (ADR-005 stage 5).
    pub move_speed_add: Option<Confidence<Fixed>>,
    /// Present when the item is a weapon.
    pub weapon: Option<WeaponProfile>,
}

/// One effect of a perk or skill, in the vocabulary the slice resolves.
/// Extending this enum is a dataset schema change (ADR-004 validation).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    /// Whole points to every attribute (Jokester +2, Fortified Ground +3).
    /// Applied at ADR-005 stage 3, strictly before curve lookups.
    AllAttributes(i32),
    /// Whole points to one attribute.
    Attribute(AttributeKind, i32),
    /// Raises a derived stat's cap (Defense Mastery: PDR 60% → 75%).
    /// Generalised in ADR-012: the cap belongs to the derived-stat
    /// definition, so the effect names which one it lifts.
    RaiseCap(DerivedStatId, Fixed),
    /// Flat move speed (ADR-005 stage 5).
    MoveSpeedAdd(Fixed),
    /// Percentage move speed bonus (ADR-005 stage 6).
    MoveSpeedBonus(Fixed),
}

/// One effect together with whether it stacks.
///
/// A stacking effect's value is **per stack** — Sprint grants 13 move speed
/// *each*, up to three — so the resolved contribution depends on how many
/// stacks are active. That is a property of the situation, not of the
/// loadout, so the loadout states it and an unstated count is an assumption
/// (ADR-007).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StackedEffect {
    /// The effect, with its own confidence grade (per-field grading,
    /// ADR-003 review).
    pub effect: Confidence<Effect>,
    /// Present when the effect stacks, carrying the maximum stack count.
    /// `None` means it applies once.
    pub max_stacks: Option<u32>,
}

impl StackedEffect {
    /// A plain, non-stacking effect.
    #[must_use]
    pub fn once(effect: Confidence<Effect>) -> Self {
        StackedEffect {
            effect,
            max_stacks: None,
        }
    }
}

/// A perk: passive, always on when slotted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PerkDef {
    /// Stable identity (`perk.rogue.jokester`).
    pub id: PerkId,
    /// Display name.
    pub name: String,
    /// Effects granted while slotted.
    pub effects: Vec<StackedEffect>,
}

/// A skill, as far as stat resolution cares: its passive/aura stat effects.
/// Activation timing, cooldowns and damage land with the exchange model
/// (ADR-006).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SkillDef {
    /// Stable identity (`skill.fighter.fortified_ground`).
    pub id: SkillId,
    /// Display name.
    pub name: String,
    /// Stat effects while active.
    pub effects: Vec<StackedEffect>,
}

impl Effect {
    /// This effect at `stacks` stacks. A stacking effect's value is per
    /// stack, so the magnitude multiplies; `RaiseCap` is returned unchanged
    /// because a raised ceiling does not stack (the loader rejects a dataset
    /// that says otherwise).
    #[must_use]
    pub fn scaled(&self, stacks: u32) -> Effect {
        let count = i64::from(stacks);
        match self {
            Effect::AllAttributes(points) => {
                Effect::AllAttributes(points.saturating_mul(stacks.cast_signed()))
            }
            Effect::Attribute(kind, points) => {
                Effect::Attribute(*kind, points.saturating_mul(stacks.cast_signed()))
            }
            Effect::RaiseCap(id, value) => Effect::RaiseCap(id.clone(), *value),
            Effect::MoveSpeedAdd(value) => Effect::MoveSpeedAdd(*value * Fixed::from_int(count)),
            Effect::MoveSpeedBonus(value) => {
                Effect::MoveSpeedBonus(*value * Fixed::from_int(count))
            }
        }
    }

    /// Whether stacking this effect is meaningful. A cap raise is a ceiling,
    /// not a quantity.
    #[must_use]
    pub fn can_stack(&self) -> bool {
        !matches!(self, Effect::RaiseCap(_, _))
    }
}

/// Everything the core needs from a dataset, as a trait the `std` crates
/// implement (ADR-000 rev 2). The core does not know that files exist.
pub trait DatasetSource {
    /// Which game build this dataset describes. A dataset that cannot name
    /// its version cannot be checked against a `Resolved` that was computed
    /// from another one (ADR-006 amendment: penetration re-sampling).
    fn build(&self) -> &str;

    /// Looks up a class definition.
    fn class(&self, id: &ClassId) -> Option<&ClassDef>;
    /// Looks up an item definition.
    fn item(&self, id: &ItemId) -> Option<&ItemDef>;
    /// Looks up a perk definition.
    fn perk(&self, id: &PerkId) -> Option<&PerkDef>;
    /// Looks up a skill definition.
    fn skill(&self, id: &SkillId) -> Option<&SkillDef>;
    /// Looks up a curve with the grade its dataset review assigned.
    fn curve(&self, id: &CurveId) -> Option<&Confidence<Curve>>;
}

/// In-memory [`DatasetSource`] over `BTreeMap`s — the reference
/// implementation used by tests and fixtures. `assay-data` produces one of
/// these from a versioned dataset directory.
#[derive(Default, Debug, Clone)]
pub struct InMemoryDataset {
    build: String,
    classes: alloc::collections::BTreeMap<ClassId, ClassDef>,
    items: alloc::collections::BTreeMap<ItemId, ItemDef>,
    perks: alloc::collections::BTreeMap<PerkId, PerkDef>,
    skills: alloc::collections::BTreeMap<SkillId, SkillDef>,
    curves: alloc::collections::BTreeMap<CurveId, Confidence<Curve>>,
}

impl InMemoryDataset {
    /// An empty dataset for the named build.
    #[must_use]
    pub fn new(build: impl Into<String>) -> Self {
        InMemoryDataset {
            build: build.into(),
            ..Self::default()
        }
    }

    /// Inserts a class definition, replacing any previous one with the id.
    pub fn insert_class(&mut self, def: ClassDef) {
        self.classes.insert(def.id.clone(), def);
    }

    /// Inserts an item definition.
    pub fn insert_item(&mut self, def: ItemDef) {
        self.items.insert(def.id.clone(), def);
    }

    /// Inserts a perk definition.
    pub fn insert_perk(&mut self, def: PerkDef) {
        self.perks.insert(def.id.clone(), def);
    }

    /// Inserts a skill definition.
    pub fn insert_skill(&mut self, def: SkillDef) {
        self.skills.insert(def.id.clone(), def);
    }

    /// Inserts a curve with its review-assigned grade.
    pub fn insert_curve(&mut self, id: CurveId, curve: Confidence<Curve>) {
        self.curves.insert(id, curve);
    }
}

impl DatasetSource for InMemoryDataset {
    fn build(&self) -> &str {
        &self.build
    }

    fn class(&self, id: &ClassId) -> Option<&ClassDef> {
        self.classes.get(id)
    }

    fn item(&self, id: &ItemId) -> Option<&ItemDef> {
        self.items.get(id)
    }

    fn perk(&self, id: &PerkId) -> Option<&PerkDef> {
        self.perks.get(id)
    }

    fn skill(&self, id: &SkillId) -> Option<&SkillDef> {
        self.skills.get(id)
    }

    fn curve(&self, id: &CurveId) -> Option<&Confidence<Curve>> {
        self.curves.get(id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::string::ToString;
    use alloc::vec;

    use super::*;

    #[test]
    fn attribute_block_sums_before_curves() {
        // Rogue base + Jokester +2 + Fortified Ground +3 (ADR-005 stage 3).
        let mut block = AttributeBlock::default();
        block.add(AttributeKind::Strength, 9);
        block.add(AttributeKind::Agility, 25);
        block.add_all(2);
        block.add_all(3);
        assert_eq!(block.get(AttributeKind::Strength).points(), 14);
        assert_eq!(block.get(AttributeKind::Agility).points(), 30);
        assert_eq!(block.get(AttributeKind::Vigor).points(), 5);
    }

    #[test]
    fn dataset_lookups_are_by_id() {
        let mut data = InMemoryDataset::new("test.build");
        data.insert_perk(PerkDef {
            id: PerkId::new("perk.rogue.jokester"),
            name: "Jokester".to_string(),
            effects: vec![StackedEffect::once(Confidence::Unverified(
                Effect::AllAttributes(2),
            ))],
        });
        assert!(data.perk(&PerkId::new("perk.rogue.jokester")).is_some());
        assert!(data.perk(&PerkId::new("perk.rogue.creep")).is_none());
    }
}
