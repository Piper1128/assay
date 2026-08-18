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
use crate::fixed::Fixed;
use crate::ids::{ClassId, CurveId, ItemId, PerkId, SkillId};
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

/// Which curve derives which stat for a class (ADR-005 stage 4). Curves are
/// referenced per class: whether classes share curves is a property of the
/// dataset, not of the code.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DerivedCurves {
    /// Strength → Physical Power Bonus (percent points).
    pub strength_to_physical_power: CurveId,
    /// Agility → Action Speed (percent points).
    pub agility_to_action_speed: CurveId,
    /// Agility → Move Speed (absolute).
    pub agility_to_move_speed: CurveId,
    /// Vigor → Health (absolute).
    pub vigor_to_health: CurveId,
    /// Armor Rating → PDR (percent points, capped at ADR-005 stage 7).
    pub armor_to_pdr: CurveId,
}

/// A playable class: base attributes and the curves that derive stats from
/// them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClassDef {
    /// Stable identity (`class.rogue`).
    pub id: ClassId,
    /// Display name; may change between patches without changing identity.
    pub name: String,
    /// Base attribute block (ADR-005 stage 1).
    pub base_attributes: Confidence<AttributeBlock>,
    /// The PDR cap before any cap-raising perk (60%).
    pub pdr_cap: Confidence<Fixed>,
    /// Curve references for derived stats.
    pub curves: DerivedCurves,
}

/// Fixed stats granted by wearing an item. Per-rarity modifier ranges are
/// the dataset arc's subject; explicit loadout rolls come in on top.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ItemDef {
    /// Stable identity (`item.dark_leather_leggings`).
    pub id: ItemId,
    /// Display name.
    pub name: String,
    /// Armor rating contributed to the defensive chain, if any.
    pub armor_rating: Option<Confidence<Fixed>>,
    /// Flat move speed contribution; negative for armor penalties
    /// (ADR-005 stage 5).
    pub move_speed_add: Option<Confidence<Fixed>>,
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
    /// Raises the PDR cap (Defense Mastery: 60% → 75%).
    RaisePdrCap(Fixed),
    /// Flat move speed (ADR-005 stage 5).
    MoveSpeedAdd(Fixed),
    /// Percentage move speed bonus (ADR-005 stage 6).
    MoveSpeedBonus(Fixed),
}

/// A perk: passive, always on when slotted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PerkDef {
    /// Stable identity (`perk.rogue.jokester`).
    pub id: PerkId,
    /// Display name.
    pub name: String,
    /// Effects, each with its own confidence grade (per-field grading,
    /// ADR-003 review).
    pub effects: Vec<Confidence<Effect>>,
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
    /// Stat effects while active, each with its own confidence grade.
    pub effects: Vec<Confidence<Effect>>,
}

/// Everything the core needs from a dataset, as a trait the `std` crates
/// implement (ADR-000 rev 2). The core does not know that files exist.
pub trait DatasetSource {
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
    classes: alloc::collections::BTreeMap<ClassId, ClassDef>,
    items: alloc::collections::BTreeMap<ItemId, ItemDef>,
    perks: alloc::collections::BTreeMap<PerkId, PerkDef>,
    skills: alloc::collections::BTreeMap<SkillId, SkillDef>,
    curves: alloc::collections::BTreeMap<CurveId, Confidence<Curve>>,
}

impl InMemoryDataset {
    /// An empty dataset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        let mut data = InMemoryDataset::new();
        data.insert_perk(PerkDef {
            id: PerkId::new("perk.rogue.jokester"),
            name: "Jokester".to_string(),
            effects: vec![Confidence::Unverified(Effect::AllAttributes(2))],
        });
        assert!(data.perk(&PerkId::new("perk.rogue.jokester")).is_some());
        assert!(data.perk(&PerkId::new("perk.rogue.creep")).is_none());
    }
}
