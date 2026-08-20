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
use crate::ids::{ClassId, DerivedStatId, ItemId, PerkId, SkillId};
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
    /// A derived stat rolled onto this copy — the game prints these as
    /// `+11 Additional Armor Rating`, and the word *Additional* is what
    /// separates them from the value printed on the item itself.
    ///
    /// Lands in stage 7's *other* bucket, outside any Item Armor Rating
    /// Bonus, for the same reason: it is not armour the item carries, it is
    /// armour rolled onto this one.
    Derived(DerivedStatId, Fixed),
}

/// Where a piece is worn.
///
/// The pipeline does not need this to add stats up — a piece is a piece —
/// so it is not here for arithmetic. It is here so a loadout that cannot
/// exist can be rejected, and so the open question about whether a ring's
/// armour rating counts as *item* armour is expressible when it is
/// answered (ADR-005 amendment: gear attributes).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Slot {
    /// Helmets and caps.
    Head,
    /// Body armour.
    Chest,
    /// Trousers and leggings.
    Legs,
    /// Gloves and gauntlets.
    Hands,
    /// Boots.
    Feet,
    /// Back slot: cloaks.
    Back,
    /// Neck slot.
    Necklace,
    /// Either ring slot.
    Ring,
    /// A held weapon, which carries stats like anything else.
    Weapon,
}

impl Slot {
    /// The name used in loadout files and diffs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Slot::Head => "head",
            Slot::Chest => "chest",
            Slot::Legs => "legs",
            Slot::Hands => "hands",
            Slot::Feet => "feet",
            Slot::Back => "back",
            Slot::Necklace => "necklace",
            Slot::Ring => "ring",
            Slot::Weapon => "weapon",
        }
    }

    /// How many of this slot a character may wear at once.
    #[must_use]
    pub fn capacity(self) -> usize {
        match self {
            Slot::Ring | Slot::Weapon => 2,
            _ => 1,
        }
    }

    /// Every slot, for validation and for iteration in a fixed order.
    pub const ALL: [Slot; 9] = [
        Slot::Head,
        Slot::Chest,
        Slot::Legs,
        Slot::Hands,
        Slot::Feet,
        Slot::Back,
        Slot::Necklace,
        Slot::Ring,
        Slot::Weapon,
    ];
}

/// One equipped piece: where it is worn, which item it is, and the rolls on
/// this copy.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GearPiece {
    /// Where this piece is worn.
    pub slot: Slot,
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
    /// Every equipped piece, in any order.
    pub gear: Vec<GearPiece>,
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
