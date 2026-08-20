//! Browser bindings: the same resolver, in a page.
//!
//! `assay-core` is `no_std + alloc` with no floats and no hash maps, which is
//! exactly what compiles to `wasm32-unknown-unknown` unchanged. So the UI runs
//! *this* resolver rather than a second one written in JavaScript — a second
//! implementation is what the Python mirror exists to catch, and shipping one
//! on purpose would be the same mistake with a nicer excuse.
//!
//! Every number crosses into JavaScript as a **decimal string**, never as a
//! JSON number. Floats are banned throughout this project because `0.1 + 0.2`
//! is the error class it exists to prevent, and a `f64` on the other side of
//! the boundary would reintroduce it at the last possible moment.
//!
//! The dataset is compiled in. A page that had to fetch its data would not
//! work from `file://`, and working from `file://` is the point: the tool has
//! to be there between raids without a server, an install, or a network.

use assay_core::confidence::{Confidence, ConfidenceLevel};
use assay_core::derived::StatBreakdown;
use assay_core::fixed::Fixed;
use assay_core::ids::{ClassId, DerivedStatId, ItemId, PerkId, SkillId};
use assay_core::loadout::{GearPiece, Loadout, PartyBuffs, Roll, Slot, Weapons};
use assay_core::resolve::{Resolved, resolve};
use assay_core::schema::{AttributeKind, DatasetSource};
use assay_data::{Dataset, DatasetText, decode};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use wasm_bindgen::prelude::wasm_bindgen;

/// The build the page ships with.
const BUILD: &str = "0.17.150.9384";

macro_rules! embedded {
    ($name:literal) => {
        include_str!(concat!("../../../data/", "0.17.150.9384", "/", $name))
    };
}

fn dataset() -> Result<Dataset, String> {
    let text = DatasetText {
        manifest: embedded!("manifest.json").to_string(),
        classes: embedded!("classes.json").to_string(),
        curves: embedded!("curves.json").to_string(),
        items: embedded!("items.json").to_string(),
        perks: embedded!("perks.json").to_string(),
        skills: embedded!("skills.json").to_string(),
    };
    decode(&text, BUILD).map_err(|e| e.to_string())
}

fn level(level: ConfidenceLevel) -> &'static str {
    match level {
        ConfidenceLevel::Verified => "verified",
        ConfidenceLevel::Unverified => "unverified",
        ConfidenceLevel::Unknown => "unknown",
    }
}

/// A graded number, as a string and a grade. Never a JSON number.
fn graded(value: &Confidence<Fixed>) -> Value {
    json!({ "value": value.value().to_string(), "confidence": level(value.level()) })
}

/// Everything the page needs to offer choices: what classes exist, what items
/// exist and where they are worn, and which abilities can be taken.
#[wasm_bindgen]
#[must_use]
pub fn catalog() -> String {
    let data = match dataset() {
        Ok(data) => data,
        Err(e) => return json!({ "ok": false, "error": e }).to_string(),
    };
    let mut classes = Vec::new();
    let mut items = Vec::new();
    let mut perks = Vec::new();
    let mut skills = Vec::new();

    for (id, kind) in &data.ids {
        match kind {
            assay_data::EntityKind::Class => {
                if let Some(def) = data.entities.class(&ClassId::new(id)) {
                    classes.push(json!({ "id": id, "name": def.name }));
                }
            }
            assay_data::EntityKind::Item => {
                if let Some(def) = data.entities.item(&ItemId::new(id)) {
                    let mut grants = Map::new();
                    for (stat, value) in &def.grants {
                        grants.insert(stat.as_str().to_string(), graded(value));
                    }
                    let mut printed = Map::new();
                    if let Some(block) = &def.attributes {
                        for (kind, points) in block.value() {
                            printed.insert(kind.as_str().to_string(), json!(points));
                        }
                    }
                    items.push(json!({
                        "id": id,
                        "name": def.name,
                        "slot": def.slot.map(Slot::as_str),
                        "grants": grants,
                        "attributes": printed,
                        "isWeapon": def.weapon.is_some(),
                    }));
                }
            }
            assay_data::EntityKind::Perk => {
                if let Some(def) = data.entities.perk(&PerkId::new(id)) {
                    perks.push(json!({ "id": id, "name": def.name }));
                }
            }
            assay_data::EntityKind::Skill => {
                if let Some(def) = data.entities.skill(&SkillId::new(id)) {
                    skills.push(json!({ "id": id, "name": def.name }));
                }
            }
            assay_data::EntityKind::Curve => {}
        }
    }

    json!({
        "ok": true,
        "build": data.manifest.build,
        "label": data.manifest.label,
        "slots": Slot::ALL.map(|s| json!({ "id": s.as_str(), "capacity": s.capacity() })),
        "attributes": AttributeKind::ALL.map(|k| k.as_str()),
        "classes": classes,
        "items": items,
        "perks": perks,
        "skills": skills,
    })
    .to_string()
}

fn parse_loadout(node: &Value) -> Result<Loadout, String> {
    let s = |key: &str| -> Result<String, String> {
        node.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("loadout needs a {key}"))
    };
    let ids = |key: &str| -> Vec<String> {
        node.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let fixed = |v: &Value, what: &str| -> Result<Fixed, String> {
        v.as_str()
            .ok_or_else(|| format!("{what} must be a decimal string, not a number"))?
            .parse()
            .map_err(|e| format!("{what}: {e:?}"))
    };

    let mut gear = Vec::new();
    for piece in node
        .get("gear")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let slot_name = piece
            .get("slot")
            .and_then(Value::as_str)
            .ok_or("a gear piece needs a slot")?;
        let slot = Slot::ALL
            .into_iter()
            .find(|s| s.as_str() == slot_name)
            .ok_or_else(|| format!("unknown slot: {slot_name}"))?;
        let mut rolls = Vec::new();
        if let Some(map) = piece.get("attributes").and_then(Value::as_object) {
            for (name, points) in map {
                let kind = AttributeKind::ALL
                    .into_iter()
                    .find(|k| k.as_str() == name)
                    .ok_or_else(|| format!("unknown attribute: {name}"))?;
                let points = points
                    .as_i64()
                    .ok_or_else(|| format!("{name} must be whole points"))?;
                rolls.push(Roll::Attribute(
                    kind,
                    i32::try_from(points).map_err(|_| format!("{name} is out of range"))?,
                ));
            }
        }
        if let Some(map) = piece.get("additional").and_then(Value::as_object) {
            for (stat, value) in map {
                rolls.push(Roll::Derived(DerivedStatId::new(stat), fixed(value, stat)?));
            }
        }
        if let Some(add) = piece.get("moveSpeedAdd") {
            rolls.push(Roll::MoveSpeedAdd(fixed(add, "moveSpeedAdd")?));
        }
        gear.push(GearPiece {
            slot,
            id: ItemId::new(piece.get("id").and_then(Value::as_str).unwrap_or_default()),
            rolls,
        });
    }

    let mut stacks = BTreeMap::new();
    if let Some(map) = node.get("stacks").and_then(Value::as_object) {
        for (id, count) in map {
            if let Some(n) = count.as_u64() {
                stacks.insert(id.clone(), u32::try_from(n).unwrap_or(u32::MAX));
            }
        }
    }

    Ok(Loadout {
        name: s("name").unwrap_or_else(|_| "loadout".to_string()),
        class: ClassId::new(&s("class")?),
        perks: ids("perks").iter().map(PerkId::new).collect(),
        skills: ids("skills").iter().map(SkillId::new).collect(),
        gear,
        weapons: Weapons {
            main_hand: node
                .get("weapons")
                .and_then(|w| w.get("mainHand"))
                .and_then(Value::as_str)
                .map(ItemId::new),
        },
        stacks,
        party: PartyBuffs {
            perks: node
                .get("party")
                .map(|p| {
                    p.get("perks")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(PerkId::new)
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default(),
            skills: node
                .get("party")
                .map(|p| {
                    p.get("skills")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(SkillId::new)
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default(),
        },
    })
}

fn render(resolved: &Resolved, name: &str) -> Value {
    let mut derived = Vec::new();
    for (id, value) in &resolved.derived {
        let mut entry = json!({
            "id": id.as_str(),
            "label": id.as_str().strip_prefix("derived.").unwrap_or(id.as_str()),
            "value": value.value().to_string(),
            "confidence": level(value.level()),
        });
        if let Some(parts) = resolved.breakdown.get(id) {
            entry["breakdown"] = breakdown(parts, value);
        }
        if let Some(note) = value.note() {
            entry["note"] = json!(note);
        }
        derived.push(entry);
    }

    let mut points = Map::new();
    for kind in AttributeKind::ALL {
        points.insert(
            kind.as_str().to_string(),
            json!(resolved.attributes.value().get(kind).points()),
        );
    }

    json!({
        "ok": true,
        "name": name,
        "build": resolved.build,
        "attributes": {
            "points": points,
            "confidence": level(resolved.attributes.level()),
        },
        "derived": derived,
        "trace": resolved
            .trace
            .iter()
            .map(|n| json!({ "stage": n.stage, "label": n.label, "detail": n.detail }))
            .collect::<Vec<_>>(),
        "armor": {
            "item": graded(&resolved.armor.item),
            "bonus": graded(&resolved.armor.bonus),
            "other": graded(&resolved.armor.other),
        },
    })
}

/// The character sheet's own decomposition, so the page can print a number
/// beside the game's and say whether they agree *for the same reason*.
fn breakdown(parts: &StatBreakdown, total: &Confidence<Fixed>) -> Value {
    let sum = *parts.from_rating.value() + *parts.from_bonuses.value();
    json!({
        "rating": graded(&parts.rating),
        "fromRating": graded(&parts.from_rating),
        "fromBonuses": graded(&parts.from_bonuses),
        // Stages 5 and 6 still adjust move speed after this, and a clamp may
        // have bound. Saying which would be guessing; saying that something
        // did is not.
        "movedLater": sum != *total.value(),
        "beforeMove": sum.to_string(),
    })
}

/// Resolves one loadout, given as JSON. Returns JSON either way: a failure is
/// a result the page can show, not an exception it has to catch.
#[wasm_bindgen]
#[must_use]
pub fn resolve_loadout(loadout_json: &str) -> String {
    let data = match dataset() {
        Ok(data) => data,
        Err(e) => return json!({ "ok": false, "error": e }).to_string(),
    };
    let node: Value = match serde_json::from_str(loadout_json) {
        Ok(node) => node,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }).to_string(),
    };
    let loadout = match parse_loadout(&node) {
        Ok(loadout) => loadout,
        Err(e) => return json!({ "ok": false, "error": e }).to_string(),
    };
    match resolve(&loadout, &data.entities) {
        Ok(resolved) => render(&resolved, &loadout.name).to_string(),
        Err(e) => json!({ "ok": false, "error": e.to_string() }).to_string(),
    }
}
