//! The situation file: what makes one attack different from another.
//!
//! ADR-006's nine steps were built, tested and mirrored, and until now the
//! only attack anyone could ask about was an unmodified swing with every
//! situational input at zero. Sneak Attack — the case the source analysis is
//! written around, whose 0% scaling is exactly what makes it immune to the
//! Hide-exit penalty — was unreachable. So were back attacks, headshots,
//! Lethal Mark and Weakpoint.
//!
//! A situation is a fact about **this attack**, not about either character.
//! That is why it is a third file rather than a section of a loadout: two
//! identical builds differ only in whether one of them is behind the other,
//! and putting that in a loadout would make it a property of the build.
//!
//! Every value is an exact decimal string. Floats are banned project-wide,
//! and a TOML float here would reintroduce the error class at the one place
//! a person types numbers by hand.

use std::collections::BTreeMap;
use std::fmt;

use assay_core::AbilityId;
use assay_core::confidence::Confidence;
use assay_core::exchange::{DamageType, ExchangeContext, Strike};
use assay_core::fixed::Fixed;
use assay_core::ids::{ItemId, SkillId};
use assay_core::schema::{DatasetSource, WeaponProfile};
use assay_core::stats::{ArmorPen, Damage, PdrMod, ScalingCoefficient, TrueDamage};
use serde::Deserialize;

/// Why a situation file could not be read.
#[derive(Debug)]
pub(crate) enum SituationError {
    /// Not valid TOML, or a field the format does not define.
    Syntax(toml::de::Error),
    /// Structurally valid but semantically wrong.
    Invalid(String),
}

impl fmt::Display for SituationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SituationError::Syntax(e) => write!(f, "{e}"),
            SituationError::Invalid(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SituationError {}

/// A parsed situation: the strike, and the circumstances around it.
pub(crate) struct Situation {
    pub(crate) strike: Strike,
    pub(crate) context: ExchangeContext,
    /// What to call it in the readout.
    pub(crate) name: String,
}

/// Reads a situation, using the weapon for whatever the file does not say.
///
/// The weapon supplies base damage and armour penetration because that is
/// where they come from; a skill overrides them by saying so. Omitting a
/// field means "as the weapon swings", not "zero" — those are different
/// statements and only one of them is usually meant.
pub(crate) fn parse(
    text: &str,
    weapon_id: &ItemId,
    weapon: &WeaponProfile,
    data: &impl DatasetSource,
) -> Result<Situation, SituationError> {
    let dto: SituationDto = toml::from_str(text).map_err(SituationError::Syntax)?;
    let mut basic = Strike::basic_swing(weapon_id, weapon);

    // Which swing in the chain, if the weapon has one. A weapon is a
    // sequence — an Arming Sword runs 100/105/110 — so "what does it hit
    // for" has as many answers as the chain is long, and asking for the
    // third is a different question from asking for the first.
    if let Some(n) = dto.combo_hit {
        if n == 0 {
            return Err(SituationError::Invalid(
                "combo_hit counts from 1: the first swing is 1, not 0".into(),
            ));
        }
        let index = n - 1; // probe: combo-counts-from-one
        let hit = weapon.combo.get(index).ok_or_else(|| {
            SituationError::Invalid(if weapon.combo.is_empty() {
                "this weapon has no chain recorded, so there is no swing to pick".to_string()
            } else {
                format!(
                    "this weapon's chain is {} swings long, so there is no swing {n}",
                    weapon.combo.len()
                )
            })
        })?;
        basic.scaling = hit.scaling.clone().map(ScalingCoefficient::new);
    }

    // A named skill fills in what it knows, over the weapon and under the
    // file. Three layers, and the order is the point: the weapon is what
    // you are holding, the skill is what the game says it does, and an
    // explicit field is what you are asking about — the most specific
    // statement wins, and the question is always the most specific.
    let mut named = None;
    if let Some(id) = &dto.skill {
        let skill = data
            .skill(&SkillId::new(id))
            .ok_or_else(|| SituationError::Invalid(format!("no such skill: {id}")))?;
        let profile = skill.strike.as_ref().ok_or_else(|| {
            SituationError::Invalid(format!(
                "{id} is not an attack: it has no strike profile, so there is \
                 nothing for a situation to take from it"
            ))
        })?;
        if let Some(v) = &profile.base {
            basic.base = v.clone().map(Damage::new);
        }
        if let Some(v) = &profile.scaling {
            basic.scaling = v.clone().map(ScalingCoefficient::new);
        }
        if let Some(v) = &profile.flat_bonus {
            basic.flat_bonus = v.clone().map(Damage::new);
        }
        if let Some(v) = &profile.penetration {
            basic.penetration = v.clone().map(ArmorPen::new);
        }
        if let Some(v) = &profile.true_damage {
            basic.true_damage = v.clone().map(TrueDamage::new);
        }
        if profile.damage_type.as_deref() == Some("magic") {
            basic.damage_type = DamageType::Magic;
        }
        named = Some(skill.name.clone());
    }

    let fixed = |raw: &Option<String>, what: &str| -> Result<Option<Fixed>, SituationError> {
        raw.as_deref()
            .map(|s| {
                s.parse::<Fixed>()
                    .map_err(|e| SituationError::Invalid(format!("{what} {s:?}: {e:?}")))
            })
            .transpose()
    };

    let strike = Strike {
        weapon: basic.weapon.clone(),
        damage_type: match dto.strike.damage_type.as_deref() {
            Some("magic") => DamageType::Magic,
            Some("physical") => DamageType::Physical,
            None => basic.damage_type,
            Some(other) => {
                return Err(SituationError::Invalid(format!(
                    "unknown damage type: {other}. It is `physical` or `magic`; true damage is a field, not a type, because bypassing reduction is what it means."
                )));
            }
        },
        base: match fixed(&dto.strike.base, "base")? {
            Some(v) => Confidence::Verified(Damage::new(v)),
            None => basic.base,
        },
        scaling: match fixed(&dto.strike.scaling, "scaling")? {
            Some(v) => Confidence::Verified(ScalingCoefficient::new(v)),
            None => basic.scaling,
        },
        flat_bonus: match fixed(&dto.strike.flat_bonus, "flat_bonus")? {
            Some(v) => Confidence::Verified(Damage::new(v)),
            None => basic.flat_bonus,
        },
        penetration: match fixed(&dto.strike.penetration, "penetration")? {
            Some(v) => Confidence::Verified(ArmorPen::new(v)),
            None => basic.penetration,
        },
        true_damage: match fixed(&dto.strike.true_damage, "true_damage")? {
            Some(v) => Confidence::Verified(TrueDamage::new(v)),
            None => basic.true_damage,
        },
    };

    let neutral = ExchangeContext::default();
    let mut mods: BTreeMap<AbilityId, Confidence<Fixed>> = BTreeMap::new();
    for (ability, raw) in &dto.context.item_armor_bonus_mods {
        let value = raw
            .parse::<Fixed>()
            .map_err(|e| SituationError::Invalid(format!("{ability}: {raw:?}: {e:?}")))?;
        mods.insert(AbilityId::new(ability), Confidence::Verified(value));
    }

    let context = ExchangeContext {
        power_bonus_adjust: match fixed(&dto.context.power_bonus_adjust, "power_bonus_adjust")? {
            Some(v) => Confidence::Verified(v),
            None => neutral.power_bonus_adjust,
        },
        pdr_mod: match fixed(&dto.context.pdr_mod, "pdr_mod")? {
            Some(v) => Confidence::Verified(PdrMod::new(v)),
            None => neutral.pdr_mod,
        },
        hit_location_bonus: match fixed(&dto.context.hit_location_bonus, "hit_location_bonus")? {
            Some(v) => Confidence::Verified(v),
            None => neutral.hit_location_bonus,
        },
        item_armor_bonus_mods: mods,
    };

    Ok(Situation {
        strike,
        context,
        name: dto
            .name
            .or(named)
            .unwrap_or_else(|| "situation".to_string()),
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SituationDto {
    #[serde(default)]
    name: Option<String>,
    /// Which swing in the weapon's chain, counting from 1. A skill named
    /// below overrides it: a skill replaces the normal swing rather than
    /// joining the chain.
    #[serde(default)]
    combo_hit: Option<usize>,
    /// A skill to take the attack's numbers from, so they live in the
    /// dataset where a patch can change them and `assay diff` can see it.
    /// Anything written below still wins: the file is the question.
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    strike: StrikeDto,
    #[serde(default)]
    context: ContextDto,
}

/// The attack itself. Every field is optional and falls back to the weapon.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrikeDto {
    /// `physical` or `magic`. Chooses which stats steps 3 and 5 through 7
    /// read; it does not add a step or change their order.
    #[serde(default, rename = "type")]
    damage_type: Option<String>,
    /// Base damage, if the skill has its own rather than the weapon's.
    #[serde(default)]
    base: Option<String>,
    /// Scaling coefficient in percent. Sneak Attack is `"0"`, and that zero
    /// is the whole mechanic rather than a missing value — which is why it
    /// has to be written to mean it.
    #[serde(default)]
    scaling: Option<String>,
    /// Flat Buff Weapon Damage.
    #[serde(default)]
    flat_bonus: Option<String>,
    /// Penetration, if the skill differs from the weapon. Which defence it
    /// reduces follows from the damage type.
    #[serde(default)]
    penetration: Option<String>,
    /// True damage, which lands after the whole reduction chain.
    #[serde(default)]
    true_damage: Option<String>,
}

/// The circumstances: where the attacker stands, what the defender is under.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextDto {
    /// Percentage-point adjustment to Physical Power Bonus for this strike:
    /// a back attack is `"30"`, leaving Hide is `"-30"`.
    #[serde(default)]
    power_bonus_adjust: Option<String>,
    /// Multiplicative PDR Mod on the defender: Lethal Mark is `"-30"`.
    #[serde(default)]
    pdr_mod: Option<String>,
    /// Hit location, in percentage points over 100. A headshot is the
    /// attacker's Headshot Damage Bonus.
    #[serde(default)]
    hit_location_bonus: Option<String>,
    /// Item Armor Rating Bonus imposed on the defender, keyed by the ability
    /// that imposed it. Keyed because the same ability never stacks across
    /// the people carrying it.
    #[serde(default)]
    item_armor_bonus_mods: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// A dataset with one attacking skill, so the naming path has
    /// something to name.
    fn data() -> assay_core::schema::InMemoryDataset {
        let mut data = assay_core::schema::InMemoryDataset::new("test.build");
        data.insert_skill(assay_core::schema::SkillDef {
            id: SkillId::new("skill.rogue.sneak_attack"),
            name: "Sneak Attack".to_string(),
            effects: alloc_vec(),
            strike: Some(assay_core::schema::StrikeProfile {
                scaling: Some(Confidence::Verified(Fixed::ZERO)),
                flat_bonus: Some(Confidence::Verified(Fixed::from_int(15))),
                true_damage: Some(Confidence::Verified(Fixed::from_int(1))),
                ..Default::default()
            }),
        });
        data.insert_skill(assay_core::schema::SkillDef {
            id: SkillId::new("skill.fighter.sprint"),
            name: "Sprint".to_string(),
            effects: alloc_vec(),
            strike: None,
        });
        data
    }

    fn alloc_vec() -> Vec<assay_core::schema::StackedEffect> {
        Vec::new()
    }

    fn weapon() -> WeaponProfile {
        WeaponProfile {
            base_damage: Confidence::Verified(Fixed::from_int(32)),
            armor_pen: Confidence::Verified(Fixed::from_int(15)),
            combo: Vec::new(),
            swing_time: None,
        }
    }

    /// The Arming Sword's chain, as the weapon page prints it.
    fn chained() -> WeaponProfile {
        WeaponProfile {
            combo: [("slash", 100), ("slash", 105), ("pierce", 110)]
                .into_iter()
                .map(|(kind, pct)| assay_core::schema::ComboHit {
                    kind: kind.to_string(),
                    scaling: Confidence::Unverified(Fixed::from_int(pct)),
                })
                .collect(),
            ..weapon()
        }
    }

    #[test]
    fn the_third_swing_hits_harder_than_the_first() {
        // The reason the chain is worth recording at all: "what does this
        // weapon hit for" has three answers, and the tool was giving the
        // smallest of them to every question.
        let d = data();
        let id = ItemId::new("item.test");
        let first = parse("combo_hit = 1", &id, &chained(), &d).unwrap();
        let third = parse("combo_hit = 3", &id, &chained(), &d).unwrap();
        assert_eq!(first.strike.scaling.value().value(), Fixed::from_int(100));
        assert_eq!(third.strike.scaling.value().value(), Fixed::from_int(110));
        // And the grade travels with it: nobody has measured these, so an
        // answer built on one cannot come back verified.
        assert!(matches!(third.strike.scaling, Confidence::Unverified(_)));
    }

    #[test]
    fn a_skill_replaces_the_swing_rather_than_joining_the_chain() {
        // Order matters here and the wrong order is plausible: a skill is
        // not the fourth hit of a combo, it is a different attack. If the
        // chain won, Sneak Attack's 0% scaling would silently become 110%
        // and the flat bonus would start multiplying.
        let s = parse(
            "combo_hit = 3
skill = \"skill.rogue.sneak_attack\"",
            &ItemId::new("item.test"),
            &chained(),
            &data(),
        )
        .unwrap();
        assert_eq!(s.strike.scaling.value().value(), Fixed::ZERO);
    }

    #[test]
    fn asking_for_a_swing_the_weapon_does_not_have_says_so() {
        let d = data();
        let id = ItemId::new("item.test");
        // Off the end of a known chain.
        let err = parse("combo_hit = 4", &id, &chained(), &d).err().unwrap();
        assert!(format!("{err}").contains("3 swings long"), "{err}");
        // Counting from zero: an easy mistake, and silently reading swing
        // one would answer a question the file did not ask.
        let err = parse("combo_hit = 0", &id, &chained(), &d).err().unwrap();
        assert!(format!("{err}").contains("counts from 1"), "{err}");
        // No chain recorded at all is a different problem and gets a
        // different sentence — one says "look it up", the other "count".
        let err = parse("combo_hit = 1", &id, &weapon(), &d).err().unwrap();
        assert!(format!("{err}").contains("no chain recorded"), "{err}");
    }

    #[test]
    fn an_empty_situation_is_the_weapon_swinging() {
        // Omitting a field means "as the weapon does", not "zero". A file
        // that said nothing and produced a zero-damage attack would be
        // answering a question nobody asked.
        let s = parse("", &ItemId::new("item.test"), &weapon(), &data()).unwrap();
        assert_eq!(s.strike.base.value().value(), Fixed::from_int(32));
        assert_eq!(s.strike.penetration.value().value(), Fixed::from_int(15));
        assert_eq!(s.strike.scaling.value().value(), Fixed::from_int(100));
    }

    #[test]
    fn sneak_attack_can_finally_be_asked_about() {
        // The case the whole source analysis is built on: 0% scaling means
        // the -30% Hide-exit penalty multiplies nothing, so the attack is
        // immune to it. Until this file existed there was no way to say it.
        let s = parse(
            "name = \"sneak-attack\"\n\
             [strike]\nscaling = \"0\"\nflat_bonus = \"15\"\ntrue_damage = \"1\"\n\
             [context]\npower_bonus_adjust = \"-30\"\n",
            &ItemId::new("item.test"),
            &weapon(),
            &data(),
        )
        .unwrap();
        assert_eq!(s.strike.scaling.value().value(), Fixed::ZERO);
        assert_eq!(s.strike.flat_bonus.value().value(), Fixed::from_int(15));
        assert_eq!(*s.context.power_bonus_adjust.value(), Fixed::from_int(-30));
        // and the weapon still supplies what the skill did not override
        assert_eq!(s.strike.base.value().value(), Fixed::from_int(32));
    }

    #[test]
    fn a_debuff_is_keyed_by_the_ability_that_imposed_it() {
        let s = parse(
            "[context.item_armor_bonus_mods]\n\
             \"skill.rogue.weakpoint_attack\" = \"-30\"\n",
            &ItemId::new("item.test"),
            &weapon(),
            &data(),
        )
        .unwrap();
        assert_eq!(s.context.item_armor_bonus_mods.len(), 1);
        assert_eq!(
            *s.context.item_armor_bonus_mods[&AbilityId::new("skill.rogue.weakpoint_attack")]
                .value(),
            Fixed::from_int(-30)
        );
    }

    #[test]
    fn a_float_is_refused_the_way_it_is_everywhere_else() {
        // TOML would happily give us 2.5 as an f64. The strings are not
        // pedantry: they are the ban holding at the one place a person
        // types a number.
        assert!(matches!(
            parse(
                "[strike]\nbase = 2.5\n",
                &ItemId::new("item.test"),
                &weapon(),
                &data()
            ),
            Err(SituationError::Syntax(_))
        ));
        assert!(matches!(
            parse(
                "[strike]\nbase = \"2.1234567\"\n",
                &ItemId::new("item.test"),
                &weapon(),
                &data()
            ),
            Err(SituationError::Invalid(_))
        ));
    }

    #[test]
    fn an_unmodelled_field_is_rejected_rather_than_ignored() {
        assert!(matches!(
            parse(
                "[context]\nlucky = \"yes\"\n",
                &ItemId::new("item.test"),
                &weapon(),
                &data()
            ),
            Err(SituationError::Syntax(_))
        ));
    }
}
