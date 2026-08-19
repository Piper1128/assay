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
//! [[armor]]
//! id = "item.dark_leather_leggings"
//! attributes = { dexterity = 4 }
//! move_speed_add = "2"
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
//! **Weapons, rarity and roll levels are not accepted yet.** The ADR sketches
//! `[weapons]` and `rarity = "epic"`/`roll = "max"`, but nothing in the
//! resolver consumes them: per-rarity modifier ranges are the dataset arc's
//! subject. Accepting a field and ignoring it would be a silent lie, so they
//! are rejected with an error that says why. Explicit rolls already work,
//! which is what those fields would have been shorthand for.

use std::collections::BTreeMap;
use std::fmt;

use assay_core::fixed::Fixed;
use assay_core::ids::{ClassId, ItemId, PerkId, SkillId};
use assay_core::loadout::{ArmorPiece, Loadout, PartyBuffs, Roll};
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

    let mut armor = Vec::new();
    for piece in &dto.armor {
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
        armor.push(ArmorPiece {
            id: ItemId::new(&piece.id),
            rolls,
        });
    }

    Ok(Loadout {
        name: dto.name,
        class: ClassId::new(&dto.class),
        perks: dto.perks.iter().map(PerkId::new).collect(),
        skills: dto.skills.iter().map(SkillId::new).collect(),
        armor,
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
    armor: Vec<ArmorDto>,
    #[serde(default)]
    party: PartyDto,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArmorDto {
    id: String,
    /// Whole attribute points rolled on this copy.
    #[serde(default)]
    attributes: BTreeMap<String, i32>,
    /// Flat move speed rolled on this copy, as an exact decimal string.
    #[serde(default)]
    move_speed_add: Option<String>,
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

[[armor]]
id = "item.dark_leather_leggings"
attributes = { dexterity = 4 }
move_speed_add = "2"

[party]
skills = ["skill.fighter.fortified_ground"]
"#;
        let loadout = parse(text).unwrap();
        assert_eq!(loadout.name, "rogue-duo-buffed");
        assert_eq!(loadout.perks.len(), 1);
        assert_eq!(loadout.armor.len(), 1);
        assert_eq!(loadout.armor[0].rolls.len(), 2);
        assert_eq!(loadout.party.skills.len(), 1);
    }

    #[test]
    fn a_minimal_loadout_needs_only_name_and_class() {
        let loadout = parse("name = \"naked\"\nclass = \"class.rogue\"\n").unwrap();
        assert!(loadout.perks.is_empty());
        assert!(loadout.armor.is_empty());
    }

    #[test]
    fn unmodelled_fields_are_rejected_rather_than_ignored() {
        // ADR-009 sketches these; nothing consumes them yet, and silently
        // accepting them would let a user believe rarity was applied.
        let with_weapons = "name = \"x\"\nclass = \"class.rogue\"\n[weapons]\nmain_hand = \"item.rondel_dagger\"\n";
        assert!(matches!(parse(with_weapons), Err(LoadoutError::Syntax(_))));

        let with_rarity = "name = \"x\"\nclass = \"class.rogue\"\n[[armor]]\nid = \"item.x\"\nrarity = \"epic\"\n";
        assert!(matches!(parse(with_rarity), Err(LoadoutError::Syntax(_))));
    }

    #[test]
    fn move_speed_is_an_exact_decimal_string() {
        let ok = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n[[armor]]\nid = \"item.x\"\nmove_speed_add = \"2.5\"\n",
        )
        .unwrap();
        assert_eq!(ok.armor[0].rolls.len(), 1);

        // Over-precision is refused, not rounded away.
        let too_precise = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n[[armor]]\nid = \"item.x\"\nmove_speed_add = \"2.1234567\"\n",
        );
        assert!(matches!(too_precise, Err(LoadoutError::Invalid(_))));
    }

    #[test]
    fn unknown_attribute_names_are_named_in_the_error() {
        let bad = parse(
            "name = \"x\"\nclass = \"class.rogue\"\n[[armor]]\nid = \"item.x\"\nattributes = { luck = 3 }\n",
        );
        match bad {
            Err(LoadoutError::Invalid(msg)) => assert!(msg.contains("luck"), "{msg}"),
            other => panic!("expected a named error, got {other:?}"),
        }
    }
}
