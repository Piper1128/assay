//! The loadout file format (ADR-009).
//!
//! TOML, human-writable, and **version-agnostic**: a loadout names entity ids
//! only and never pins a dataset version. That is the whole precondition for
//! the impact diff (ADR-008 level 2) — the same file must resolve against two
//! versions to say what a patch did to it.
//!
//! ```toml
//! name  = "rogue-lethal-artillery"
//! class = "class.rogue"
//!
//! perks  = ["perk.rogue.jokester"]
//! skills = []
//!
//! [[gear]]
//! slot = "legs"
//! id = "item.dark_leather_leggings"
//! attributes = { dexterity = 4 }
//! move_speed_add = "2"
//! additional = { "derived.armor_rating" = "5" }
//!
//! [weapons]
//! main_hand = "item.flanged_mace"
//!
//! [stacks]
//! "skill.fighter.sprint" = 3
//!
//! [party]
//! skills = ["skill.fighter.fortified_ground"]
//! ```
//!
//! ## Two deviations from ADR-009's sketch, both deliberate
//!
//! **Party buffs are id lists, not booleans.** The ADR sketched
//! `fortified_ground = true`, which hardcodes one skill per TOML key: it
//! cannot express a buff the schema has not heard of, and it cannot say
//! *which* ally granted it. Id lists match how every other reference in the
//! format works.
//!
//! **Only the main hand, and no rarity or roll levels.** `[weapons]` accepts
//! `main_hand` because the exchange model consumes it; `off_hand` is absent
//! because dual-wield changes how attacks alternate and that mechanic does
//! not exist yet. `rarity = "epic"` and `roll = "max"` are still rejected:
//! per-rarity modifier ranges are the dataset arc's subject, and accepting a
//! field that changes nothing would be a silent lie. Explicit rolls already
//! work, which is what those fields would have been shorthand for.

use std::collections::BTreeMap;
use std::fmt;

use assay_core::DerivedStatId;
use assay_core::fixed::Fixed;
use assay_core::ids::{ClassId, ItemId, PerkId, SkillId};
use assay_core::loadout::{GearPiece, Loadout, PartyBuffs, Roll, Slot, Weapons};
use assay_core::schema::AttributeKind;
use serde::Deserialize;

/// Why a loadout file could not be read.
#[derive(Debug)]
pub(crate) enum LoadoutError {
    /// Not valid TOML, or a field the format does not define.
    Syntax(toml::de::Error),
    /// Structurally valid but semantically wrong.
    Invalid(String),
}

impl fmt::Display for LoadoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadoutError::Syntax(e) => write!(f, "{e}"),
            LoadoutError::Invalid(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for LoadoutError {}

/// Parses a loadout file into the core's loadout type.
pub(crate) fn parse(text: &str) -> Result<Loadout, LoadoutError> {
    let dto: LoadoutDto = toml::from_str(text).map_err(LoadoutError::Syntax)?;

    let mut gear = Vec::new();
    let mut worn: BTreeMap<&str, usize> = BTreeMap::new();
    for piece in &dto.gear {
        let slot = slot_of(&piece.slot)
            .ok_or_else(|| LoadoutError::Invalid(format!("unknown slot {:?}", piece.slot)))?;
        let count = worn.entry(slot.as_str()).or_default();
        *count += 1;
        if *count > slot.capacity() {
            return Err(LoadoutError::Invalid(format!(
                "{} pieces in the {} slot, which holds {}",
                count,
                slot.as_str(),
                slot.capacity()
            )));
        }
        let mut rolls = Vec::new();
        for (name, points) in &piece.attributes {
            rolls.push(Roll::Attribute(attribute_kind(name)?, *points));
        }
        if let Some(add) = &piece.move_speed_add {
            // A string, not a TOML float: floats are banned project-wide, and
            // Fixed's parser rejects over-precision instead of truncating it.
            let parsed: Fixed = add.parse().map_err(|e| {
                LoadoutError::Invalid(format!("{}: move_speed_add {add:?}: {e}", piece.id))
            })?;
            rolls.push(Roll::MoveSpeedAdd(parsed));
        }
        // The game writes these `+11 Additional Armor Rating`. They are rolls
        // on this copy, so they sit outside any Item Armor Rating Bonus.
        for (stat, value) in &piece.additional {
            let parsed: Fixed = value.parse().map_err(|e| {
                LoadoutError::Invalid(format!("{}: additional {stat} {value:?}: {e}", piece.id))
            })?;
            rolls.push(Roll::Derived(DerivedStatId::new(stat), parsed));
        }
        gear.push(GearPiece {
            slot,
            id: ItemId::new(&piece.id),
            rolls,
        });
    }

    Ok(Loadout {
        name: dto.name,
        class: ClassId::new(&dto.class),
        perks: dto.perks.iter().map(PerkId::new).collect(),
        skills: dto.skills.iter().map(SkillId::new).collect(),
        gear,
        weapons: Weapons {
            main_hand: dto.weapons.main_hand.as_deref().map(ItemId::new),
        },
        stacks: dto.stacks,
        party: PartyBuffs {
            perks: dto.party.perks.iter().map(PerkId::new).collect(),
            skills: dto.party.skills.iter().map(SkillId::new).collect(),
        },
    })
}

fn attribute_kind(name: &str) -> Result<AttributeKind, LoadoutError> {
    AttributeKind::ALL
        .into_iter()
        .find(|k| k.as_str() == name)
        .ok_or_else(|| LoadoutError::Invalid(format!("unknown attribute: {name}")))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadoutDto {
    name: String,
    class: String,
    #[serde(default)]
    perks: Vec<String>,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    gear: Vec<GearDto>,
    #[serde(default)]
    weapons: WeaponsDto,
    /// Active stacks per stacking source. Omit a source and it resolves at
    /// its maximum, graded as the assumption it is.
    #[serde(default)]
    stacks: BTreeMap<String, u32>,
    #[serde(default)]
    party: PartyDto,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WeaponsDto {
    #[serde(default)]
    main_hand: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GearDto {
    /// Where the piece is worn: head, chest, legs, hands, feet, cape,
    /// necklace, ring or weapon.
    slot: String,
    id: String,
    /// Whole attribute points rolled on this copy.
    #[serde(default)]
    attributes: BTreeMap<String, i32>,
    /// Flat move speed rolled on this copy, as an exact decimal string.
    #[serde(default)]
    move_speed_add: Option<String>,
    /// Derived stats rolled onto this copy, by stat id — the game prints
    /// these as `+11 Additional Armor Rating`. Outside any Item Armor
    /// Rating Bonus, because they are not what the item carries.
    #[serde(default)]
    additional: BTreeMap<String, String>,
}

/// Parses a slot name. Unknown names are rejected rather than defaulted:
/// a typo that silently became a chest piece would change the answer.
fn slot_of(name: &str) -> Option<Slot> {
    Slot::ALL.into_iter().find(|s| s.as_str() == name)
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartyDto {
    #[serde(default)]
    perks: Vec<String>,
    #[serde(default)]
    skills: Vec<String>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_the_documented_example() {
        let text = r#"
name  = "rogue-duo-buffed"
class = "class.rogue"
perks = ["perk.rogue.jokester"]

[[gear]]
slot = "legs"
id = "item.dark_leather_leggings"
attributes = { dexterity = 4 }
move_speed_add = "2"

[party]
skills = ["skill.fighter.fortified_ground"]
"#;
        let loadout = parse(text).unwrap();
        assert_eq!(loadout.name, "rogue-duo-buffed");
        assert_eq!(loadout.perks.len(), 1);
        assert_eq!(loadout.gear.len(), 1);
        assert_eq!(loadout.gear[0].rolls.len(), 2);
        assert_eq!(loadout.party.skills.len(), 1);
    }

    #[test]
    fn a_minimal_loadout_needs_only_name_and_class() {
        let loadout = parse("name = \"naked\"\nclass = \"class.rogue\"\n").unwrap();
        assert!(loadout.perks.is_empty());
        assert!(loadout.gear.is_empty());
    }

    #[test]
    fn unmodelled_fields_are_rejected_rather_than_ignored() {
        // A main hand is accepted now: the exchange model consumes it.
        let main_hand = r#"
name  = "x"
class = "class.rogue"
[weapons]
main_hand = "item.flanged_mace"
"#;
        assert!(parse(main_hand).is_ok());

        // An off hand still changes nothing — dual-wield is not modelled,
        // and accepting the field would let a user believe it was.
        let off_hand = r#"
name  = "x"
class = "class.rogue"
[weapons]
off_hand = "item.x"
"#;
        assert!(matches!(parse(off_hand), Err(LoadoutError::Syntax(_))));

        // Rarity and roll levels likewise: per-rarity ranges are deferred.
        let with_rarity = r#"
name  = "x"
class = "class.rogue"
[[gear]]
slot = \"legs\"
id = \"item.x\"
rarity = "epic"
"#;
        assert!(matches!(parse(with_rarity), Err(LoadoutError::Syntax(_))));
    }

    #[test]
    fn move_speed_is_an_exact_decimal_string() {
        let ok = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n[[gear]]\nslot = \"legs\"\nid = \"item.x\"\nmove_speed_add = \"2.5\"\n",
        )
        .unwrap();
        assert_eq!(ok.gear[0].rolls.len(), 1);

        // Over-precision is refused, not rounded away.
        let too_precise = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n[[gear]]\nslot = \"legs\"\nid = \"item.x\"\nmove_speed_add = \"2.1234567\"\n",
        );
        assert!(matches!(too_precise, Err(LoadoutError::Invalid(_))));
    }

    #[test]
    fn a_slot_cannot_hold_more_than_it_has_room_for() {
        // Two rings are fine, three are not, and one cape is the limit.
        // The pipeline never needed the slot to add stats up; this is the
        // question it exists to answer.
        let two_rings = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n\
             [[gear]]\nslot = \"ring\"\nid = \"item.x\"\n\
             [[gear]]\nslot = \"ring\"\nid = \"item.x\"\n",
        );
        assert!(two_rings.is_ok(), "{two_rings:?}");

        let three_rings = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n\
             [[gear]]\nslot = \"ring\"\nid = \"item.x\"\n\
             [[gear]]\nslot = \"ring\"\nid = \"item.x\"\n\
             [[gear]]\nslot = \"ring\"\nid = \"item.x\"\n",
        );
        assert!(matches!(three_rings, Err(LoadoutError::Invalid(_))));

        let two_capes = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n\
             [[gear]]\nslot = \"cape\"\nid = \"item.x\"\n\
             [[gear]]\nslot = \"cape\"\nid = \"item.x\"\n",
        );
        assert!(matches!(two_capes, Err(LoadoutError::Invalid(_))));
    }

    #[test]
    fn an_unknown_slot_is_refused_rather_than_defaulted() {
        // A typo that silently became a chest piece would change the answer
        // without saying so.
        let bad = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n\
             [[gear]]\nslot = \"backpack\"\nid = \"item.x\"\n",
        );
        match bad {
            Err(LoadoutError::Invalid(msg)) => assert!(msg.contains("backpack"), "{msg}"),
            other => panic!("expected a named error, got {other:?}"),
        }
    }

    #[test]
    fn unknown_attribute_names_are_named_in_the_error() {
        let bad = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n[[gear]]\nslot = \"legs\"\nid = \"item.x\"\nattributes = { luck = 3 }\n",
        );
        match bad {
            Err(LoadoutError::Invalid(msg)) => assert!(msg.contains("luck"), "{msg}"),
            other => panic!("expected a named error, got {other:?}"),
        }
    }
}
