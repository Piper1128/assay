//! Replays the slice vector against the Rust implementation.
//!
//! `fixtures/slice/duo_slice.json` carries expected canonical statblocks
//! computed by the independent Python mirror (ADR-010 rev 2 §3). Rust must
//! agree **byte for byte** — one banker's rounding difference anywhere in
//! the pipeline shows up here. CI additionally re-runs the mirror
//! (`python3 mirror/gen_slice_vector.py --check`), so mirror, vector file
//! and this replay are pinned to each other from both sides.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use assay_core::exchange::{Exchange, ExchangeContext, Strike};
use assay_core::stats::{ArmorPen, Damage, PdrMod, ScalingCoefficient, TrueDamage};
use assay_core::{
    ArmorPiece, AttributeBlock, AttributeKind, ClassDef, ClassId, Confidence, Curve, CurveId,
    DerivedStatDef, DerivedStatId, Effect, Fixed, InMemoryDataset, ItemDef, ItemId, Loadout,
    PartyBuffs, PerkDef, PerkId, RatingInput, Resolved, Roll, SkillDef, SkillId, Weapons,
    canonical_exchange, canonical_statblock, resolve,
};
use serde_json::Value;
use std::collections::BTreeMap;

fn vector() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/slice/duo_slice.json");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e} — run: python mirror/gen_slice_vector.py",
            path.display()
        )
    });
    serde_json::from_str(&raw).expect("vector file is not valid JSON")
}

fn graded<T>(node: &Value, value: T) -> Confidence<T> {
    match node["confidence"].as_str().expect("confidence key") {
        "verified" => Confidence::Verified(value),
        "unverified" => Confidence::Unverified(value),
        "unknown" => Confidence::Unknown {
            assumed: value,
            note: node["note"]
                .as_str()
                .expect("unknown carries a note")
                .to_string(),
        },
        other => panic!("bad confidence grade: {other}"),
    }
}

fn graded_micro(node: &Value) -> Confidence<Fixed> {
    graded(
        node,
        Fixed::from_micro(node["micro"].as_i64().expect("micro int")),
    )
}

fn attribute_kind(name: &str) -> AttributeKind {
    match name {
        "strength" => AttributeKind::Strength,
        "vigor" => AttributeKind::Vigor,
        "agility" => AttributeKind::Agility,
        "dexterity" => AttributeKind::Dexterity,
        "will" => AttributeKind::Will,
        "knowledge" => AttributeKind::Knowledge,
        "resourcefulness" => AttributeKind::Resourcefulness,
        other => panic!("unknown attribute: {other}"),
    }
}

fn attribute_block(node: &Value) -> AttributeBlock {
    let mut block = AttributeBlock::default();
    for (name, points) in node.as_object().expect("points object") {
        block.add(
            attribute_kind(name),
            i32::try_from(points.as_i64().expect("points int")).expect("points fit i32"),
        );
    }
    block
}

fn effect(node: &Value) -> Confidence<Effect> {
    let payload = match node["kind"].as_str().expect("effect kind") {
        "all_attributes" => Effect::AllAttributes(
            i32::try_from(node["points"].as_i64().expect("points")).expect("points fit i32"),
        ),
        "attribute" => Effect::Attribute(
            attribute_kind(node["attribute"].as_str().expect("attribute name")),
            i32::try_from(node["points"].as_i64().expect("points")).expect("points fit i32"),
        ),
        "raise_cap" => Effect::RaiseCap(
            DerivedStatId::new(node["target"].as_str().expect("raise_cap target")),
            Fixed::from_micro(node["micro"].as_i64().expect("micro")),
        ),
        "move_speed_add" => {
            Effect::MoveSpeedAdd(Fixed::from_micro(node["micro"].as_i64().expect("micro")))
        }
        "move_speed_bonus" => {
            Effect::MoveSpeedBonus(Fixed::from_micro(node["micro"].as_i64().expect("micro")))
        }
        other => panic!("unknown effect kind: {other}"),
    };
    graded(node, payload)
}

fn dataset(node: &Value) -> InMemoryDataset {
    let mut data = InMemoryDataset::new();
    for class in node["classes"].as_array().expect("classes") {
        let derived = class["derived"]
            .as_array()
            .expect("derived defs")
            .iter()
            .map(|def| {
                let weights: BTreeMap<RatingInput, Fixed> = def["weights"]
                    .as_array()
                    .expect("weights")
                    .iter()
                    .map(|w| {
                        let reference = w["ref"].as_str().expect("weight ref");
                        let input = match w["kind"].as_str().expect("weight kind") {
                            "attribute" => RatingInput::Attribute(attribute_kind(reference)),
                            "derived" => RatingInput::Derived(DerivedStatId::new(reference)),
                            other => panic!("unknown weight kind: {other}"),
                        };
                        (
                            input,
                            Fixed::from_micro(w["weight"].as_i64().expect("weight")),
                        )
                    })
                    .collect();
                let optional_fixed = |key: &str| {
                    def.get(key)
                        .and_then(serde_json::Value::as_i64)
                        .map(Fixed::from_micro)
                };
                DerivedStatDef {
                    id: DerivedStatId::new(def["id"].as_str().expect("derived id")),
                    weights,
                    curve: CurveId::new(def["curve"].as_str().expect("curve id")),
                    offset: optional_fixed("offset").unwrap_or(Fixed::ZERO),
                    floor: optional_fixed("floor"),
                    cap: optional_fixed("cap"),
                }
            })
            .collect();
        data.insert_class(ClassDef {
            id: ClassId::new(class["id"].as_str().expect("class id")),
            name: class["name"].as_str().expect("class name").to_string(),
            base_attributes: graded(
                &class["base_attributes"],
                attribute_block(&class["base_attributes"]["points"]),
            ),
            derived,
        });
    }
    for curve in node["curves"].as_array().expect("curves") {
        let points = curve["points"]
            .as_array()
            .expect("points")
            .iter()
            .map(|pair| {
                let xy = pair.as_array().expect("point pair");
                (
                    Fixed::from_micro(xy[0].as_i64().expect("x micro")),
                    Fixed::from_micro(xy[1].as_i64().expect("y micro")),
                )
            })
            .collect();
        data.insert_curve(
            CurveId::new(curve["id"].as_str().expect("curve id")),
            graded(curve, Curve::linear(points).expect("valid curve")),
        );
    }
    for item in node["items"].as_array().expect("items") {
        let optional = |key: &str| {
            let field = &item[key];
            if field.is_null() {
                None
            } else {
                Some(graded_micro(field))
            }
        };
        data.insert_item(ItemDef {
            id: ItemId::new(item["id"].as_str().expect("item id")),
            name: item["name"].as_str().expect("item name").to_string(),
            armor_rating: optional("armor_rating"),
            move_speed_add: optional("move_speed_add"),
            weapon: None,
        });
    }
    for perk in node["perks"].as_array().expect("perks") {
        data.insert_perk(PerkDef {
            id: PerkId::new(perk["id"].as_str().expect("perk id")),
            name: perk["name"].as_str().expect("perk name").to_string(),
            effects: perk["effects"]
                .as_array()
                .expect("effects")
                .iter()
                .map(effect)
                .collect(),
        });
    }
    for skill in node["skills"].as_array().expect("skills") {
        data.insert_skill(SkillDef {
            id: SkillId::new(skill["id"].as_str().expect("skill id")),
            name: skill["name"].as_str().expect("skill name").to_string(),
            effects: skill["effects"]
                .as_array()
                .expect("effects")
                .iter()
                .map(effect)
                .collect(),
        });
    }
    data
}

fn loadout(node: &Value) -> Loadout {
    let ids = |key: &str| {
        node[key]
            .as_array()
            .expect("id list")
            .iter()
            .map(|v| v.as_str().expect("id").to_string())
            .collect::<Vec<_>>()
    };
    let armor = node["armor"]
        .as_array()
        .expect("armor")
        .iter()
        .map(|piece| ArmorPiece {
            id: ItemId::new(piece["id"].as_str().expect("item id")),
            rolls: piece["rolls"]
                .as_array()
                .expect("rolls")
                .iter()
                .map(|roll| match roll["kind"].as_str().expect("roll kind") {
                    "attribute" => Roll::Attribute(
                        attribute_kind(roll["attribute"].as_str().expect("attribute")),
                        i32::try_from(roll["points"].as_i64().expect("points"))
                            .expect("points fit i32"),
                    ),
                    "move_speed_add" => Roll::MoveSpeedAdd(Fixed::from_micro(
                        roll["micro"].as_i64().expect("micro"),
                    )),
                    other => panic!("unknown roll kind: {other}"),
                })
                .collect(),
        })
        .collect();
    Loadout {
        name: node["name"].as_str().expect("loadout name").to_string(),
        class: ClassId::new(node["class"].as_str().expect("class id")),
        perks: ids("perks").into_iter().map(PerkId::new).collect(),
        skills: ids("skills").into_iter().map(SkillId::new).collect(),
        armor,
        weapons: Weapons {
            main_hand: node
                .get("weapons")
                .and_then(|w| w.get("main_hand"))
                .and_then(serde_json::Value::as_str)
                .map(ItemId::new),
        },
        party: PartyBuffs {
            perks: node["party"]["perks"]
                .as_array()
                .expect("party perks")
                .iter()
                .map(|v| PerkId::new(v.as_str().expect("id")))
                .collect(),
            skills: node["party"]["skills"]
                .as_array()
                .expect("party skills")
                .iter()
                .map(|v| SkillId::new(v.as_str().expect("id")))
                .collect(),
        },
    }
}

fn strike(node: &Value) -> Strike {
    Strike {
        base: graded_micro(&node["base"]).map(Damage::new),
        scaling: graded_micro(&node["scaling"]).map(ScalingCoefficient::new),
        flat_bonus: graded_micro(&node["flat_bonus"]).map(Damage::new),
        armor_pen: graded_micro(&node["armor_pen"]).map(ArmorPen::new),
        true_damage: graded_micro(&node["true_damage"]).map(TrueDamage::new),
    }
}

fn exchange_context(node: &Value) -> ExchangeContext {
    ExchangeContext {
        power_bonus_adjust: graded_micro(&node["power_bonus_adjust"]),
        pdr_mod: graded_micro(&node["pdr_mod"]).map(PdrMod::new),
        hit_location_bonus: graded_micro(&node["hit_location_bonus"]),
    }
}

#[test]
fn rust_agrees_with_the_mirror_on_every_exchange() {
    let vector = vector();
    let data = dataset(&vector["dataset"]);
    // Resolve every loadout once; exchanges reference them by name, so the
    // exchange cases ride on exactly the resolutions the statblock cases lock.
    let mut resolved_by_name: Vec<(String, Resolved)> = Vec::new();
    for entry in vector["loadouts"].as_array().expect("loadouts") {
        let name = entry["name"].as_str().expect("name").to_string();
        let resolved = resolve(&loadout(entry), &data).expect("loadout resolves");
        resolved_by_name.push((name, resolved));
    }
    let by_name = |wanted: &str| -> &Resolved {
        &resolved_by_name
            .iter()
            .find(|(name, _)| name == wanted)
            .unwrap_or_else(|| panic!("exchange references unknown loadout: {wanted}"))
            .1
    };

    let exchanges = vector["exchanges"].as_array().expect("exchanges");
    assert!(!exchanges.is_empty(), "vector has no exchanges");
    for case in exchanges {
        let name = case["name"].as_str().expect("name");
        let s = strike(&case["strike"]);
        let context = exchange_context(&case["context"]);
        let outcome = Exchange::new(
            by_name(case["attacker"].as_str().expect("attacker")),
            by_name(case["defender"].as_str().expect("defender")),
            &s,
            &context,
        )
        .damage()
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let expected = case["expected_canonical"]
            .as_str()
            .expect("expected_canonical");
        assert_eq!(
            canonical_exchange(&outcome),
            expected,
            "{name}: Rust and mirror disagree on the exchange"
        );
    }
}

#[test]
fn rust_agrees_with_the_mirror_on_every_loadout() {
    let vector = vector();
    let data = dataset(&vector["dataset"]);
    let loadouts = vector["loadouts"].as_array().expect("loadouts");
    assert!(!loadouts.is_empty(), "vector has no loadouts");
    for entry in loadouts {
        let name = entry["name"].as_str().expect("name");
        let resolved = resolve(&loadout(entry), &data)
            .unwrap_or_else(|e| panic!("{name}: resolve failed: {e}"));
        let canon = canonical_statblock(&resolved);
        let expected = entry["expected_canonical"]
            .as_str()
            .expect("expected_canonical");
        assert_eq!(canon, expected, "{name}: Rust and mirror disagree");
    }
}

#[test]
fn resolution_is_deterministic_in_process() {
    // ADR-001 rev 2 §5, in-process half: same input → byte-identical
    // canonical output on repeated runs. The fresh-process and second
    // platform halves run in CI once the CLI exposes resolution.
    let vector = vector();
    let data = dataset(&vector["dataset"]);
    let entry = &vector["loadouts"].as_array().expect("loadouts")[0];
    let first = canonical_statblock(&resolve(&loadout(entry), &data).expect("resolves"));
    let second = canonical_statblock(&resolve(&loadout(entry), &data).expect("resolves"));
    assert_eq!(first, second);
}
