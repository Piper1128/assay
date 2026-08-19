//! Diff engine for Assay (ADR-008).
//!
//! Two levels, one machine:
//!
//! **Level 1 — dataset diff.** What changed in the *data*: entities added,
//! removed, renamed, or fields modified. Renames are explicit
//! (`renamed_from`, set by a human under review), never inferred — heuristic
//! rename detection is a source of quietly wrong diffs, and this project is
//! designed against exactly that.
//!
//! **Level 2 — impact diff.** What changed for *my builds*: resolve a corpus
//! of loadouts against both versions and report the stat deltas. This is the
//! output that answers the question actually asked every Thursday, and the
//! reason a loadout must never pin a dataset version (ADR-009).
//!
//! The same engine renders the scraper's proposal diff (ADR-003): one piece
//! of machinery, two uses. A scraper break then shows up as a loud, obvious
//! diff rather than as silent corruption.
//!
//! Fields are compared as rendered strings rather than by walking typed
//! structures. That is deliberate: every value already has an exact,
//! lossless rendering (integers and `Fixed`, never floats), the field set
//! grows with the schema for free, and a new field cannot silently escape
//! the diff because nobody remembered to add a match arm.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use assay_core::confidence::{Confidence, ConfidenceLevel};
use assay_core::derived::RatingInput;
use assay_core::fixed::Fixed;
use assay_core::schema::{AttributeKind, DatasetSource, Effect};
use assay_core::{ClassId, CurveId, ItemId, Loadout, PerkId, SkillId, resolve};
use assay_data::{Dataset, EntityKind};

/// One change to one entity (ADR-008 level 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The entity is new in the later version.
    Added {
        /// Entity id.
        id: String,
        /// Which table it lives in.
        kind: EntityKind,
    },
    /// The entity is gone from the later version.
    Removed {
        /// Entity id.
        id: String,
        /// Which table it lived in.
        kind: EntityKind,
    },
    /// The entity kept its identity under a new id, stated explicitly.
    Renamed {
        /// Id in the earlier version.
        from_id: String,
        /// Id in the later version.
        to_id: String,
    },
    /// A field changed value. Absence is rendered as `—`, which is distinct
    /// from a zero.
    Modified {
        /// Entity id, in the later version.
        id: String,
        /// Field name.
        field: String,
        /// Rendering in the earlier version.
        from: String,
        /// Rendering in the later version.
        to: String,
    },
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Change::Added { id, kind } => write!(f, "+ {id} ({kind:?})"),
            Change::Removed { id, kind } => write!(f, "- {id} ({kind:?})"),
            Change::Renamed { from_id, to_id } => write!(f, "~ {from_id} -> {to_id}"),
            Change::Modified {
                id,
                field,
                from,
                to,
            } => write!(f, "  {id}.{field}: {from} -> {to}"),
        }
    }
}

/// One loadout's stat deltas between two versions (ADR-008 level 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impact {
    /// The loadout's name.
    pub name: String,
    /// Per derived stat: the value before, after, and whether it moved.
    pub stats: Vec<StatDelta>,
    /// Set when the loadout stops resolving in one of the versions — itself
    /// useful information (ADR-009).
    pub error: Option<String>,
}

/// How one derived stat moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatDelta {
    /// Derived stat id.
    pub id: String,
    /// Value in the earlier version.
    pub from: Fixed,
    /// Value in the later version.
    pub to: Fixed,
}

impl StatDelta {
    /// True when the value moved at all.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.from != self.to
    }

    /// The signed difference.
    #[must_use]
    pub fn delta(&self) -> Fixed {
        self.to - self.from
    }
}

/// Compares two dataset versions structurally (ADR-008 level 1).
///
/// Changes come back in a stable order: by entity id, then field name.
#[must_use]
pub fn dataset_diff(before: &Dataset, after: &Dataset) -> Vec<Change> {
    let mut changes = Vec::new();

    // Explicit renames first: they tell us which old id an entity continues,
    // so it is not reported as an add plus a remove.
    let renamed_from: BTreeMap<&str, &str> = after
        .renames
        .iter()
        .map(|(new_id, old_id)| (old_id.as_str(), new_id.as_str()))
        .collect();

    for (id, kind) in &after.ids {
        if !before.ids.contains_key(id) && !after.renames.contains_key(id) {
            changes.push(Change::Added {
                id: id.clone(),
                kind: *kind,
            });
        }
    }
    for (id, kind) in &before.ids {
        if let Some(new_id) = renamed_from.get(id.as_str()) {
            changes.push(Change::Renamed {
                from_id: id.clone(),
                to_id: (*new_id).to_string(),
            });
        } else if !after.ids.contains_key(id) {
            changes.push(Change::Removed {
                id: id.clone(),
                kind: *kind,
            });
        }
    }

    // Field-level comparison for everything present in both, following
    // renames so a renamed entity's fields are still compared.
    let before_fields = fields(before);
    let after_fields = fields(after);
    for (id, after_map) in &after_fields {
        let old_id = after.renames.get(id).map_or(id.as_str(), String::as_str);
        let Some(before_map) = before_fields.get(old_id) else {
            continue;
        };
        let names: BTreeSet<&String> = before_map.keys().chain(after_map.keys()).collect();
        for name in names {
            let from = before_map.get(name).map_or("—", String::as_str);
            let to = after_map.get(name).map_or("—", String::as_str);
            if from != to {
                changes.push(Change::Modified {
                    id: id.clone(),
                    field: name.clone(),
                    from: from.to_string(),
                    to: to.to_string(),
                });
            }
        }
    }

    changes.sort_by_key(|c| match c {
        Change::Added { id, .. } | Change::Removed { id, .. } => (id.clone(), String::new()),
        Change::Renamed { from_id, .. } => (from_id.clone(), String::new()),
        Change::Modified { id, field, .. } => (id.clone(), field.clone()),
    });
    changes
}

/// Resolves each loadout against both versions and reports the deltas
/// (ADR-008 level 2).
#[must_use]
pub fn impact_diff(before: &Dataset, after: &Dataset, loadouts: &[Loadout]) -> Vec<Impact> {
    loadouts
        .iter()
        .map(|loadout| {
            let old = resolve(loadout, &before.entities);
            let new = resolve(loadout, &after.entities);
            match (old, new) {
                (Ok(old), Ok(new)) => {
                    let ids: BTreeSet<&assay_core::DerivedStatId> =
                        old.derived.keys().chain(new.derived.keys()).collect();
                    let stats = ids
                        .into_iter()
                        .filter_map(|id| {
                            // A stat present in only one version is reported
                            // through the dataset diff, not as a fake delta.
                            let from = old.derived.get(id)?;
                            let to = new.derived.get(id)?;
                            Some(StatDelta {
                                id: id.as_str().to_string(),
                                from: *from.value(),
                                to: *to.value(),
                            })
                        })
                        .collect();
                    Impact {
                        name: loadout.name.clone(),
                        stats,
                        error: None,
                    }
                }
                (Err(e), _) | (_, Err(e)) => Impact {
                    name: loadout.name.clone(),
                    stats: Vec::new(),
                    error: Some(e.to_string()),
                },
            }
        })
        .collect()
}

/// Renders every entity's fields as `id -> {field -> rendering}`.
///
/// The renderings carry confidence, so a value that stayed numerically equal
/// but was downgraded from verified to unverified shows up as a change —
/// which it is.
fn fields(dataset: &Dataset) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let entities = &dataset.entities;

    for (id, kind) in &dataset.ids {
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        match kind {
            EntityKind::Class => {
                if let Some(def) = entities.class(&ClassId::new(id)) {
                    map.insert("name".into(), def.name.clone());
                    let block = def.base_attributes.value();
                    for kind in AttributeKind::ALL {
                        map.insert(
                            format!("base.{}", kind.as_str()),
                            block.get(kind).points().to_string(),
                        );
                    }
                    map.insert(
                        "base.confidence".into(),
                        level(def.base_attributes.level()).into(),
                    );
                    for stat in &def.derived {
                        let prefix = stat.id.as_str();
                        map.insert(format!("{prefix}.curve"), stat.curve.as_str().to_string());
                        map.insert(format!("{prefix}.offset"), stat.offset.to_string());
                        map.insert(format!("{prefix}.floor"), optional(stat.floor));
                        map.insert(format!("{prefix}.cap"), optional(stat.cap));
                        for (input, weight) in &stat.weights {
                            let name = match input {
                                RatingInput::Attribute(kind) => kind.as_str().to_string(),
                                RatingInput::Derived(id) => id.as_str().to_string(),
                            };
                            map.insert(format!("{prefix}.weight.{name}"), weight.to_string());
                        }
                    }
                }
            }
            EntityKind::Curve => {
                if let Some(curve) = entities.curve(&CurveId::new(id)) {
                    map.insert("confidence".into(), level(curve.level()).into());
                    map.insert("points".into(), format!("{:?}", curve.value()));
                }
            }
            EntityKind::Item => {
                if let Some(def) = entities.item(&ItemId::new(id)) {
                    map.insert("name".into(), def.name.clone());
                    insert_graded(&mut map, "armor_rating", def.armor_rating.as_ref());
                    insert_graded(&mut map, "move_speed_add", def.move_speed_add.as_ref());
                }
            }
            EntityKind::Perk => {
                if let Some(def) = entities.perk(&PerkId::new(id)) {
                    map.insert("name".into(), def.name.clone());
                    insert_effects(&mut map, &def.effects);
                }
            }
            EntityKind::Skill => {
                if let Some(def) = entities.skill(&SkillId::new(id)) {
                    map.insert("name".into(), def.name.clone());
                    insert_effects(&mut map, &def.effects);
                }
            }
        }
        out.insert(id.clone(), map);
    }
    out
}

fn level(level: ConfidenceLevel) -> &'static str {
    match level {
        ConfidenceLevel::Verified => "verified",
        ConfidenceLevel::Unverified => "unverified",
        ConfidenceLevel::Unknown => "unknown",
    }
}

fn optional(value: Option<Fixed>) -> String {
    value.map_or_else(|| "—".to_string(), |v| v.to_string())
}

fn insert_graded(
    map: &mut BTreeMap<String, String>,
    name: &str,
    value: Option<&Confidence<Fixed>>,
) {
    if let Some(value) = value {
        map.insert(name.to_string(), value.value().to_string());
        map.insert(format!("{name}.confidence"), level(value.level()).into());
    }
}

fn insert_effects(map: &mut BTreeMap<String, String>, effects: &[Confidence<Effect>]) {
    for (index, effect) in effects.iter().enumerate() {
        let rendered = match effect.value() {
            Effect::AllAttributes(points) => format!("all_attributes {points:+}"),
            Effect::Attribute(kind, points) => format!("attribute {} {points:+}", kind.as_str()),
            Effect::RaiseCap(id, value) => format!("raise_cap {} {value}", id.as_str()),
            Effect::MoveSpeedAdd(value) => format!("move_speed_add {value}"),
            Effect::MoveSpeedBonus(value) => format!("move_speed_bonus {value}"),
        };
        map.insert(format!("effect.{index}"), rendered);
        map.insert(
            format!("effect.{index}.confidence"),
            level(effect.level()).into(),
        );
    }
}
