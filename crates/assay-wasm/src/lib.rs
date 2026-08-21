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
use assay_core::exchange::{DamageType, Exchange, ExchangeContext, Strike};
use assay_core::fixed::Fixed;
use assay_core::ids::{AbilityId, ClassId, DerivedStatId, ItemId, PerkId, SkillId};
use assay_core::loadout::{GearPiece, Loadout, PartyBuffs, Roll, Slot, Weapons};
use assay_core::resolve::{Resolved, resolve};
use assay_core::schema::{AttributeKind, DatasetSource};
use assay_core::stats::Rarity;
use assay_data::submission::{ItemObservation, Method, Submission};
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
                    // Everything an item card prints, so a tooltip can be
                    // the card rather than a summary of it. The grade travels
                    // with each line: a tooltip that showed a number without
                    // saying how well known it is would undo the whole point
                    // of grading them.
                    items.push(json!({
                        "id": id,
                        "name": def.name,
                        "rarity": def.rarity.map(Rarity::as_str),
                        "slot": def.slot.map(Slot::as_str),
                        "requiredClasses": def.required_classes.iter()
                            .map(|c| c.as_str()).collect::<Vec<_>>(),
                        "grants": grants,
                        "attributes": printed,
                        "attributesGrade": def.attributes.as_ref()
                            .map(|a| level(a.level())),
                        "moveSpeedAdd": def.move_speed_add.as_ref().map(graded),
                        "weapon": def.weapon.as_ref().map(|w| json!({
                            "baseDamage": graded(&w.base_damage),
                            "armorPen": graded(&w.armor_pen),
                            "swingTime": w.swing_time.as_ref().map(graded),
                            "combo": w.combo.iter().map(|hit| json!({
                                "kind": hit.kind.as_str(),
                                "scaling": graded(&hit.scaling),
                            })).collect::<Vec<_>>(),
                        })),
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
                    // Whether it attacks, so the page can offer the ones
                    // that do without knowing any of their numbers. A skill
                    // added to the dataset then appears in the UI with no
                    // code change, the way a derived stat does (ADR-012).
                    skills.push(json!({
                        "id": id,
                        "name": def.name,
                        "attacks": def.strike.is_some(),
                    }));
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

/// Resolves two loadouts and states the difference between them.
///
/// The subtraction happens here rather than in the page. A delta computed in
/// JavaScript would be a `parseFloat` away from the exact error class this
/// project exists to prevent — and it would be a second implementation of
/// arithmetic the core already does exactly, which is what the mirror exists
/// to catch. So the page renders differences; it never computes one.
///
/// A stat present in only one build is reported as appearing or vanishing
/// rather than as a difference from zero, because those are different facts.
#[wasm_bindgen]
#[must_use]
pub fn compare_loadouts(a_json: &str, b_json: &str) -> String {
    let a: Value = match serde_json::from_str(&resolve_loadout(a_json)) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }).to_string(),
    };
    let b: Value = match serde_json::from_str(&resolve_loadout(b_json)) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }).to_string(),
    };
    if a["ok"] != json!(true) {
        return json!({ "ok": false, "error": a["error"], "side": "a" }).to_string();
    }
    if b["ok"] != json!(true) {
        return json!({ "ok": false, "error": b["error"], "side": "b" }).to_string();
    }

    // The grade travels with the comparison too. A difference column with no
    // provenance is the one place this tool could quietly become the thing
    // it exists against: two numbers, a delta, and no way to see that half
    // of it came off a wiki.
    let read = |side: &Value| -> BTreeMap<String, (String, String, String)> {
        side["derived"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        Some((
                            row["id"].as_str()?.to_string(),
                            (
                                row["label"].as_str()?.to_string(),
                                row["value"].as_str()?.to_string(),
                                row["confidence"].as_str()?.to_string(),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (left, right) = (read(&a), read(&b));

    let mut ids: Vec<&String> = left.keys().chain(right.keys()).collect();
    ids.sort_unstable();
    ids.dedup();

    let mut deltas = Vec::new();
    for id in ids {
        // The weaker of the two grades, by the same minimum rule the core
        // propagates by: a comparison is only as trustworthy as its worse half.
        let weaker = |a: &str, b: &str| -> String {
            let rank = |g: &str| match g {
                "verified" => 2,
                "unverified" => 1,
                _ => 0,
            };
            if rank(a) <= rank(b) {
                a.to_string()
            } else {
                b.to_string()
            }
        };
        let entry = match (left.get(id), right.get(id)) {
            (Some((label, from, ga)), Some((_, to, gb))) => {
                let (from_v, to_v) = match (from.parse::<Fixed>(), to.parse::<Fixed>()) {
                    (Ok(f), Ok(t)) => (f, t),
                    _ => continue,
                };
                let change = to_v - from_v;
                json!({
                    "id": id,
                    "label": label,
                    "from": from,
                    "to": to,
                    // `{:+}` so a delta always carries its sign: an unsigned
                    // "3" in a difference column is ambiguous in the one
                    // place ambiguity costs the most.
                    "delta": format!("{change:+}"),
                    "same": change == Fixed::ZERO,
                    "confidence": weaker(ga, gb),
                })
            }
            (Some((label, from, ga)), None) => json!({
                "id": id, "label": label, "from": from, "to": Value::Null,
                "gone": true, "confidence": ga,
            }),
            (None, Some((label, to, gb))) => json!({
                "id": id, "label": label, "from": Value::Null, "to": to,
                "new": true, "confidence": gb,
            }),
            (None, None) => continue,
        };
        deltas.push(entry);
    }
    json!({ "ok": true, "a": a, "b": b, "delta": deltas }).to_string()
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

/// Writes a submission from a card the page read.
///
/// The format is written here rather than in JavaScript so there is one
/// writer and one reader of it, both in Rust. A page that hand-rolled the
/// JSON would be a second implementation of the contract, free to drift from
/// the one `assay submit` parses — and the drift would show up as a
/// contributor's work being rejected, which is the worst place to find it.
#[wasm_bindgen]
#[must_use]
pub fn submission_json(card_json: &str, observer: &str, observed_at: &str, method: &str) -> String {
    let node: Value = match serde_json::from_str(card_json) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }).to_string(),
    };
    let method = match method {
        "screenshot-ocr" => Method::ScreenshotOcr,
        "screenshot-typed" => Method::ScreenshotTyped,
        "in-game" => Method::InGame,
        "documented" => Method::Documented,
        other => {
            return json!({ "ok": false, "error": format!("unknown method: {other}") }).to_string();
        }
    };

    let text = |key: &str| node.get(key).and_then(Value::as_str).unwrap_or_default();
    let mut grants: BTreeMap<String, String> = BTreeMap::new();
    let mut attributes: BTreeMap<String, i32> = BTreeMap::new();
    let mut move_speed_add = None;
    if let Some(rows) = node.get("lines").and_then(Value::as_array) {
        for row in rows {
            let what = row.get("what").and_then(Value::as_str).unwrap_or_default();
            let amount = row
                .get("amount")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(attr) = what.strip_prefix("attr:") {
                if let Ok(points) = amount.parse::<i32>() {
                    *attributes.entry(attr.to_string()).or_default() += points;
                }
            } else if let Some(stat) = what.strip_prefix("stat:") {
                grants.insert(stat.to_string(), amount.to_string());
            } else if what.starts_with("move:") {
                move_speed_add = Some(amount.to_string());
            }
        }
    }

    let observation = ItemObservation {
        id: text("id").to_string(),
        rarity: Some(text("rarity"))
            .filter(|t| !t.is_empty())
            .map(str::to_string),
        required_classes: node
            .get("requiredClasses")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        name: text("name").to_string(),
        slot: node
            .get("slot")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        grants,
        attributes,
        move_speed_add,
    };
    let submission = Submission {
        submission: assay_data::submission::FORMAT,
        observer: observer.to_string(),
        observed_at: observed_at.to_string(),
        build: BUILD.to_string(),
        method,
        note: node
            .get("note")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        items: vec![observation],
        unrecognised: node
            .get("unrecognised")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    };
    match submission.encode() {
        Ok(text) => json!({ "ok": true, "text": text }).to_string(),
        Err(e) => json!({ "ok": false, "error": e.to_string() }).to_string(),
    }
}

/// One attack, with all nine steps.
///
/// The situation arrives as its own object for the same reason the CLI takes
/// its own file: it is a fact about the attack, not about either character.
/// Omitted fields fall back to the weapon swinging, which is not the same as
/// zero and is usually what was meant.
#[wasm_bindgen]
#[must_use]
pub fn exchange(attacker_json: &str, defender_json: &str, situation_json: &str) -> String {
    let data = match dataset() {
        Ok(data) => data,
        Err(e) => return json!({ "ok": false, "error": e }).to_string(),
    };
    let read = |text: &str, side: &str| -> Result<Loadout, String> {
        let node: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        parse_loadout(&node).map_err(|e| format!("{side}: {e}"))
    };
    let (attacker_loadout, defender_loadout) = match (
        read(attacker_json, "attacker"),
        read(defender_json, "defender"),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return json!({ "ok": false, "error": e }).to_string(),
    };

    let resolved = |loadout: &Loadout, side: &str| -> Result<Resolved, String> {
        resolve(loadout, &data.entities).map_err(|e| format!("{side}: {e}"))
    };
    let (attacker, defender) = match (
        resolved(&attacker_loadout, "attacker"),
        resolved(&defender_loadout, "defender"),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return json!({ "ok": false, "error": e }).to_string(),
    };

    // A weapon is required. A "basic attack" with nothing in hand has no
    // base damage, and inventing one would be the tool guessing.
    let Some(weapon_id) = &attacker_loadout.weapons.main_hand else {
        return json!({
            "ok": false,
            "error": "the attacker has no weapon, so there is nothing to swing",
        })
        .to_string();
    };
    let Some(profile) = data
        .entities
        .item(weapon_id)
        .and_then(|i| i.weapon.as_ref())
    else {
        return json!({
            "ok": false,
            "error": format!("{} is not a weapon in this dataset", weapon_id.as_str()),
        })
        .to_string();
    };

    let situation: Value = match serde_json::from_str(situation_json) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }).to_string(),
    };
    let (strike, context, named) =
        match build_situation(&situation, weapon_id, profile, &data.entities) {
            Ok(parts) => parts,
            Err(e) => return json!({ "ok": false, "error": e }).to_string(),
        };

    match Exchange::new(&attacker, &defender, &strike, &context, &data.entities).damage() {
        Ok(out) => json!({
            "ok": true,
            "damage": graded(&out.damage.clone().map(|d| d.value())),
            "effectivePdr": graded(&out.effective_pdr.clone().map(|p| p.value())),
            "weapon": data.entities.item(weapon_id).map(|i| i.name.clone()),
            "skill": named,
            "hitsToKill": out.hits_to_kill,
            "timeToKill": out.time_to_kill.as_ref().map(graded),
            // What the weapon actually does, when it does more than swing
            // the same blow forever.
            "chainToKill": out.chain_to_kill,
            "chainTimeToKill": out.chain_time_to_kill.as_ref().map(graded),
            "steps": out
                .trace
                .iter()
                .map(|n| json!({ "step": n.stage, "label": n.label, "detail": n.detail }))
                .collect::<Vec<_>>(),
        })
        .to_string(),
        Err(e) => json!({ "ok": false, "error": e.to_string() }).to_string(),
    }
}

fn build_situation(
    node: &Value,
    weapon_id: &ItemId,
    weapon: &assay_core::schema::WeaponProfile,
    data: &impl DatasetSource,
) -> Result<(Strike, ExchangeContext, Option<String>), String> {
    let mut basic = Strike::basic_swing(weapon_id, weapon);

    // Three layers, same as the CLI: the weapon is what you hold, the skill
    // is what the game says it does, an explicit field is what you asked.
    // The page used to carry Sneak Attack's numbers in JavaScript, which put
    // a fact about the game one more layer away from `assay diff`.
    let mut named = None;
    if let Some(id) = node
        .get("skill")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        let skill = data
            .skill(&SkillId::new(id))
            .ok_or_else(|| format!("no such skill: {id}"))?;
        let profile = skill
            .strike
            .as_ref()
            .ok_or_else(|| format!("{} is not an attack", skill.name))?;
        if let Some(v) = &profile.base {
            basic.base = v.clone().map(assay_core::stats::Damage::new);
        }
        if let Some(v) = &profile.scaling {
            basic.scaling = v.clone().map(assay_core::stats::ScalingCoefficient::new);
        }
        if let Some(v) = &profile.flat_bonus {
            basic.flat_bonus = v.clone().map(assay_core::stats::Damage::new);
        }
        if let Some(v) = &profile.penetration {
            basic.penetration = v.clone().map(assay_core::stats::ArmorPen::new);
        }
        if let Some(v) = &profile.true_damage {
            basic.true_damage = v.clone().map(assay_core::stats::TrueDamage::new);
        }
        if profile.damage_type.as_deref() == Some("magic") {
            basic.damage_type = DamageType::Magic;
        }
        named = Some(skill.name.clone());
    }
    let neutral = ExchangeContext::default();
    let fixed = |section: &str, key: &str| -> Result<Option<Fixed>, String> {
        let Some(raw) = node.get(section).and_then(|s| s.get(key)) else {
            return Ok(None);
        };
        let text = raw
            .as_str()
            .ok_or_else(|| format!("{key} must be a decimal string, not a number"))?;
        if text.trim().is_empty() {
            return Ok(None);
        }
        text.parse::<Fixed>()
            .map(Some)
            .map_err(|e| format!("{key} {text:?}: {e:?}"))
    };

    let strike = Strike {
        // The page hands over whatever the fields say; overriding the blow
        // itself means the chain is no longer the question being asked.
        pinned: fixed("strike", "scaling")?.is_some() || fixed("strike", "base")?.is_some(),
        tags: basic.tags.clone(),
        weapon: basic.weapon.clone(),
        damage_type: match node.get("type").and_then(Value::as_str) {
            Some("magic") => DamageType::Magic,
            Some("physical") => DamageType::Physical,
            None => basic.damage_type,
            Some(other) => return Err(format!("unknown damage type: {other}")),
        },
        base: fixed("strike", "base")?.map_or(basic.base, |v| {
            Confidence::Verified(assay_core::stats::Damage::new(v))
        }),
        scaling: fixed("strike", "scaling")?.map_or(basic.scaling, |v| {
            Confidence::Verified(assay_core::stats::ScalingCoefficient::new(v))
        }),
        flat_bonus: fixed("strike", "flatBonus")?.map_or(basic.flat_bonus, |v| {
            Confidence::Verified(assay_core::stats::Damage::new(v))
        }),
        penetration: fixed("strike", "penetration")?.map_or(basic.penetration, |v| {
            Confidence::Verified(assay_core::stats::ArmorPen::new(v))
        }),
        true_damage: fixed("strike", "trueDamage")?.map_or(basic.true_damage, |v| {
            Confidence::Verified(assay_core::stats::TrueDamage::new(v))
        }),
    };

    let mut mods: BTreeMap<AbilityId, Confidence<Fixed>> = BTreeMap::new();
    if let Some(map) = node
        .get("context")
        .and_then(|c| c.get("itemArmorBonusMods"))
        .and_then(Value::as_object)
    {
        for (ability, raw) in map {
            let text = raw
                .as_str()
                .ok_or_else(|| format!("{ability} must be a decimal string"))?;
            let value = text
                .parse::<Fixed>()
                .map_err(|e| format!("{ability} {text:?}: {e:?}"))?;
            mods.insert(AbilityId::new(ability), Confidence::Verified(value));
        }
    }

    let context = ExchangeContext {
        power_bonus_adjust: fixed("context", "powerBonusAdjust")?
            .map_or(neutral.power_bonus_adjust, Confidence::Verified),
        pdr_mod: fixed("context", "pdrMod")?.map_or(neutral.pdr_mod, |v| {
            Confidence::Verified(assay_core::stats::PdrMod::new(v))
        }),
        hit_location_bonus: fixed("context", "hitLocationBonus")?
            .map_or(neutral.hit_location_bonus, Confidence::Verified),
        item_armor_bonus_mods: mods,
    };
    Ok((strike, context, named))
}
