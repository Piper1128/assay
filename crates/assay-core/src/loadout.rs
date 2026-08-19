//! Loadouts as the core sees them (ADR-009 semantics).
//!
//! A loadout references entity ids only and never pins a dataset version —
//! the version is supplied at resolve time, which is the whole precondition
//! for the impact diff (ADR-008 level 2). The TOML file format and its
//! parsing live with `assay-cli`; this is the resolved-input shape.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fixed::Fixed;
use crate::ids::{ClassId, ItemId, PerkId, SkillId};
use crate::schema::AttributeKind;

/// An explicit gear roll chosen in the loadout. Rolls are part of the
/// *question* ("this exact pair of leggings"), not a wiki claim — so they
/// enter the pipeline as `Verified` facts, while the item's own base fields
/// keep their dataset grades.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Roll {
    /// Whole attribute points rolled on the piece (`dexterity = 4`).
    Attribute(AttributeKind, i32),
    /// Flat move speed rolled on the piece (`move_speed = 2`).
    MoveSpeedAdd(Fixed),
    /// Armour rating rolled on the piece (`armor_rating = 5`). This is an
    /// enchantment, so it lands in stage 7's *other* bucket and no Item
    /// Armor Rating Bonus multiplies it.
    ArmorRating(Fixed),
}

/// One equipped piece: an item id plus the explicit rolls on this copy.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArmorPiece {
    /// Which item definition this piece instantiates.
    pub id: ItemId,
    /// Explicit rolls on this copy; explicit values win over any
    /// convenience roll level (ADR-009).
    pub rolls: Vec<Roll>,
}

/// Buffs granted by party members whose auras cover this character
/// (ADR-005 stage 3: Fortified Ground from the Fighter, Jokester from the
/// Rogue). Own perks and skills apply through their own lists; these are the
/// *external* sources.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct PartyBuffs {
    /// Party members' skills affecting this character.
    pub skills: Vec<SkillId>,
    /// Party members' perks affecting this character.
    pub perks: Vec<PerkId>,
}

/// Wielded weapons. Only the main hand is modelled: dual-wield changes how
/// attacks alternate, which is a mechanic the exchange model does not have
/// yet, and pretending otherwise would produce confident wrong numbers.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Weapons {
    /// The weapon an attack comes from.
    pub main_hand: Option<ItemId>,
}

/// A complete loadout: the input to [`crate::resolve::resolve`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Loadout {
    /// Human-readable name (`rogue-lethal-artillery`).
    pub name: String,
    /// The class being resolved.
    pub class: ClassId,
    /// Slotted perks, in declaration order (applied in this order).
    pub perks: Vec<PerkId>,
    /// Slotted skills, in declaration order.
    pub skills: Vec<SkillId>,
    /// Equipped armor pieces.
    pub armor: Vec<ArmorPiece>,
    /// Wielded weapons.
    pub weapons: Weapons,
    /// Active stacks per stacking source, keyed by perk or skill id.
    ///
    /// A stacking effect only has a value once you say how many stacks are
    /// up, and that is a fact about the moment rather than about the build.
    /// An unstated count resolves at the maximum and is graded `Unknown`,
    /// so the assumption travels with the number (ADR-007).
    pub stacks: BTreeMap<String, u32>,
    /// External party buffs.
    pub party: PartyBuffs,
}
