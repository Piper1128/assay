//! Dataset I/O for Assay (ADR-003, ADR-004).
//!
//! Knows the filesystem; knows nothing about computation. Loads hand-approved,
//! versioned JSON from `data/<build>/` and hands owned structures into
//! `assay-core`. The trust boundary is one-way: this crate must never depend
//! on `assay-scrape` (enforced by the dependency-direction gate in CI).
//!
//! ## Why the file format is described here and not in the core
//!
//! ADR-000 rev 2 sketched putting `serde` derives on the core's own types.
//! This crate uses **separate DTO structs** instead, and the deviation is
//! deliberate:
//!
//! - The graded-value shape depends on its payload — `{"confidence":…,
//!   "micro":…}` for a number but `{"confidence":…,"points":{…}}` for an
//!   attribute block. One derive cannot express both.
//! - `#[serde(deny_unknown_fields)]` on a DTO *is* ADR-004's "no unknown
//!   fields" validation. Getting a schema gate for free is worth a layer.
//! - It keeps `serde` out of `assay-core` entirely, so the `no_std` purity
//!   gate has one less dependency to survive.
//!
//! The file format is a contract with the humans who edit the dataset
//! (ADR-003's fallback guarantee); the domain types are a contract with the
//! resolver. Letting them drift apart on purpose is cheaper than coupling
//! them.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use assay_core::confidence::Confidence;
use assay_core::curve::Curve;
use assay_core::derived::{DerivedStatDef, RatingInput};
use assay_core::fixed::Fixed;
use assay_core::ids::{ClassId, CurveId, DerivedStatId, ItemId, PerkId, SkillId};
use assay_core::schema::{
    AttributeBlock, AttributeKind, ClassDef, Effect, InMemoryDataset, ItemDef, PerkDef, SkillDef,
};
use serde::Deserialize;

/// Provenance for one dataset file (ADR-004): where it came from, when, and
/// whether a human has approved it. `reviewed: false` never reaches the core.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    /// File this record describes.
    pub file: String,
    /// Where the values came from (`wiki:spellsandguns`, `patch-notes`, …).
    pub origin: String,
    /// When the source was read.
    pub scraped: String,
    /// Whether a human approved it (ADR-003's review gate).
    pub reviewed: bool,
}

/// A dataset version's manifest (ADR-004).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The game's build id — the version key.
    pub build: String,
    /// Human-friendly alias (`hotfix-123`).
    pub label: String,
    /// Release date of the build.
    pub released: String,
    /// Predecessor build, if any.
    pub previous: Option<String>,
    /// Per-file provenance.
    pub sources: Vec<SourceRecord>,
}

/// Why a dataset could not be loaded.
#[derive(Debug)]
pub enum LoadError {
    /// The directory or a required file is missing or unreadable.
    Io(PathBuf, std::io::Error),
    /// A file is not valid JSON, or violates the schema (unknown field,
    /// missing mandatory field, bad enum value).
    Schema(PathBuf, serde_json::Error),
    /// A cross-file reference does not resolve, or a value is out of range.
    Invalid(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(path, e) => write!(f, "{}: {e}", path.display()),
            LoadError::Schema(path, e) => write!(f, "{}: {e}", path.display()),
            LoadError::Invalid(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// A loaded dataset version: the manifest plus everything the resolver needs.
#[derive(Debug)]
pub struct Dataset {
    /// The version's manifest.
    pub manifest: Manifest,
    /// Entities, ready for `assay_core::resolve`.
    pub entities: InMemoryDataset,
    /// Explicit renames, `new_id -> old_id` (ADR-008). Set by a human under
    /// review; the diff never guesses that two ids are the same thing.
    pub renames: BTreeMap<String, String>,
    /// Every entity id this version defines, so a diff can spot additions
    /// and removals without reaching back into the files.
    pub ids: BTreeMap<String, EntityKind>,
}

/// Which table an id lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityKind {
    /// A playable class.
    Class,
    /// A curve definition.
    Curve,
    /// An item.
    Item,
    /// A perk.
    Perk,
    /// A skill.
    Skill,
}

/// Loads the dataset for one build from `root/<build>/`.
pub fn load(root: &Path, build: &str) -> Result<Dataset, LoadError> {
    let dir = root.join(build);
    let manifest: Manifest = read_json(&dir.join("manifest.json"))?;
    if manifest.build != build {
        return Err(LoadError::Invalid(format!(
            "manifest build {} does not match directory {build}",
            manifest.build
        )));
    }
    for source in &manifest.sources {
        if !source.reviewed {
            return Err(LoadError::Invalid(format!(
                "{} is not reviewed; unreviewed data must not reach the core (ADR-003)",
                source.file
            )));
        }
    }

    let classes: ClassFile = read_json(&dir.join("classes.json"))?;
    let curves: CurveFile = read_json(&dir.join("curves.json"))?;
    let items: ItemFile = read_json(&dir.join("items.json"))?;
    let perks: PerkFile = read_json(&dir.join("perks.json"))?;
    let skills: SkillFile = read_json(&dir.join("skills.json"))?;

    let mut entities = InMemoryDataset::new();
    let mut curve_ids: Vec<String> = Vec::new();
    let mut renames: BTreeMap<String, String> = BTreeMap::new();
    let mut ids: BTreeMap<String, EntityKind> = BTreeMap::new();

    for dto in curves.curves {
        let points: Vec<(Fixed, Fixed)> = dto
            .points
            .iter()
            .map(|p| (Fixed::from_micro(p[0]), Fixed::from_micro(p[1])))
            .collect();
        let curve = Curve::linear(points)
            .map_err(|e| LoadError::Invalid(format!("curve {}: {e}", dto.id)))?;
        curve_ids.push(dto.id.clone());
        ids.insert(dto.id.clone(), EntityKind::Curve);
        if let Some(from) = &dto.renamed_from {
            renames.insert(dto.id.clone(), from.clone());
        }
        entities.insert_curve(
            CurveId::new(&dto.id),
            dto.confidence.wrap_with_note(curve, dto.note.as_deref())?,
        );
    }

    for dto in classes.classes {
        let mut derived = Vec::new();
        for def in &dto.derived {
            if !curve_ids.contains(&def.curve) {
                return Err(LoadError::Invalid(format!(
                    "{} references curve {}, which this version does not define",
                    def.id, def.curve
                )));
            }
            let mut weights: BTreeMap<RatingInput, Fixed> = BTreeMap::new();
            for w in &def.weights {
                let input = match w.kind {
                    WeightKind::Attribute => RatingInput::Attribute(attribute_kind(&w.reference)?),
                    WeightKind::Derived => RatingInput::Derived(DerivedStatId::new(&w.reference)),
                };
                if weights.insert(input, Fixed::from_micro(w.weight)).is_some() {
                    return Err(LoadError::Invalid(format!(
                        "{} weights {} twice",
                        def.id, w.reference
                    )));
                }
            }
            if weights.is_empty() {
                return Err(LoadError::Invalid(format!("{} has no weights", def.id)));
            }
            derived.push(DerivedStatDef {
                id: DerivedStatId::new(&def.id),
                weights,
                curve: CurveId::new(&def.curve),
                offset: Fixed::from_micro(def.offset.unwrap_or(0)),
                floor: def.floor.map(Fixed::from_micro),
                cap: def.cap.map(Fixed::from_micro),
            });
        }
        ids.insert(dto.id.clone(), EntityKind::Class);
        if let Some(from) = &dto.renamed_from {
            renames.insert(dto.id.clone(), from.clone());
        }
        let block = attribute_block(&dto.base_attributes.points)?;
        entities.insert_class(ClassDef {
            id: ClassId::new(&dto.id),
            name: dto.name,
            base_attributes: dto
                .base_attributes
                .confidence
                .wrap_with_note(block, dto.base_attributes.note.as_deref())?,
            derived,
        });
    }

    for dto in items.items {
        ids.insert(dto.id.clone(), EntityKind::Item);
        if let Some(from) = &dto.renamed_from {
            renames.insert(dto.id.clone(), from.clone());
        }
        entities.insert_item(ItemDef {
            id: ItemId::new(&dto.id),
            name: dto.name,
            armor_rating: dto.armor_rating.map(GradedMicro::into_fixed).transpose()?,
            move_speed_add: dto
                .move_speed_add
                .map(GradedMicro::into_fixed)
                .transpose()?,
        });
    }

    for dto in perks.perks {
        ids.insert(dto.id.clone(), EntityKind::Perk);
        if let Some(from) = &dto.renamed_from {
            renames.insert(dto.id.clone(), from.clone());
        }
        entities.insert_perk(PerkDef {
            id: PerkId::new(&dto.id),
            name: dto.name,
            effects: effects(&dto.effects)?,
        });
    }

    for dto in skills.skills {
        ids.insert(dto.id.clone(), EntityKind::Skill);
        if let Some(from) = &dto.renamed_from {
            renames.insert(dto.id.clone(), from.clone());
        }
        entities.insert_skill(SkillDef {
            id: SkillId::new(&dto.id),
            name: dto.name,
            effects: effects(&dto.effects)?,
        });
    }

    for (new_id, old_id) in &renames {
        if ids.contains_key(old_id) {
            return Err(LoadError::Invalid(format!(
                "{new_id} claims to be renamed from {old_id}, but {old_id} still exists in this version"
            )));
        }
    }

    Ok(Dataset {
        manifest,
        entities,
        renames,
        ids,
    })
}

/// Lists the build ids available under `root`, sorted.
pub fn versions(root: &Path) -> Result<Vec<String>, LoadError> {
    let mut out = Vec::new();
    let entries = fs::read_dir(root).map_err(|e| LoadError::Io(root.to_path_buf(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| LoadError::Io(root.to_path_buf(), e))?;
        if entry.path().join("manifest.json").is_file()
            && let Some(name) = entry.file_name().to_str()
        {
            out.push(name.to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, LoadError> {
    let raw = fs::read_to_string(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))?;
    serde_json::from_str(&raw).map_err(|e| LoadError::Schema(path.to_path_buf(), e))
}

fn attribute_kind(name: &str) -> Result<AttributeKind, LoadError> {
    AttributeKind::ALL
        .into_iter()
        .find(|k| k.as_str() == name)
        .ok_or_else(|| LoadError::Invalid(format!("unknown attribute: {name}")))
}

fn attribute_block(points: &BTreeMap<String, i32>) -> Result<AttributeBlock, LoadError> {
    let mut block = AttributeBlock::default();
    for (name, value) in points {
        block.add(attribute_kind(name)?, *value);
    }
    Ok(block)
}

fn effects(dtos: &[EffectDto]) -> Result<Vec<Confidence<Effect>>, LoadError> {
    dtos.iter()
        .map(|dto| {
            let effect = match dto.kind {
                EffectKind::AllAttributes => Effect::AllAttributes(require_points(dto)?),
                EffectKind::Attribute => Effect::Attribute(
                    attribute_kind(dto.attribute.as_deref().ok_or_else(|| {
                        LoadError::Invalid("attribute effect needs `attribute`".into())
                    })?)?,
                    require_points(dto)?,
                ),
                EffectKind::RaiseCap => Effect::RaiseCap(
                    DerivedStatId::new(dto.target.as_deref().ok_or_else(|| {
                        LoadError::Invalid("raise_cap effect needs `target`".into())
                    })?),
                    Fixed::from_micro(require_micro(dto)?),
                ),
                EffectKind::MoveSpeedAdd => {
                    Effect::MoveSpeedAdd(Fixed::from_micro(require_micro(dto)?))
                }
                EffectKind::MoveSpeedBonus => {
                    Effect::MoveSpeedBonus(Fixed::from_micro(require_micro(dto)?))
                }
            };
            dto.confidence.wrap_with_note(effect, dto.note.as_deref())
        })
        .collect()
}

fn require_points(dto: &EffectDto) -> Result<i32, LoadError> {
    dto.points
        .ok_or_else(|| LoadError::Invalid(format!("{:?} effect needs `points`", dto.kind)))
}

fn require_micro(dto: &EffectDto) -> Result<i64, LoadError> {
    dto.micro
        .ok_or_else(|| LoadError::Invalid(format!("{:?} effect needs `micro`", dto.kind)))
}

// ── File-format DTOs ────────────────────────────────────────────────────────
// `deny_unknown_fields` everywhere: an unrecognised key is a dataset error,
// not something to skip silently (ADR-004 schema validation).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfidenceDto {
    Verified,
    Unverified,
    Unknown,
}

impl ConfidenceDto {
    /// Wraps a value in its grade. `unknown` without a note is a schema
    /// error: ADR-007 requires an assumption to say why, and dropping the
    /// note on the floor would defeat the whole grade.
    fn wrap_with_note<T>(self, value: T, note: Option<&str>) -> Result<Confidence<T>, LoadError> {
        match (self, note) {
            (ConfidenceDto::Verified, _) => Ok(Confidence::Verified(value)),
            (ConfidenceDto::Unverified, _) => Ok(Confidence::Unverified(value)),
            (ConfidenceDto::Unknown, Some(note)) => Ok(Confidence::Unknown {
                assumed: value,
                note: note.to_string(),
            }),
            (ConfidenceDto::Unknown, None) => Err(LoadError::Invalid(
                "an `unknown` value must carry a `note` saying what was assumed (ADR-007)".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GradedMicro {
    confidence: ConfidenceDto,
    micro: i64,
    #[serde(default)]
    note: Option<String>,
}

impl GradedMicro {
    fn into_fixed(self) -> Result<Confidence<Fixed>, LoadError> {
        self.confidence
            .wrap_with_note(Fixed::from_micro(self.micro), self.note.as_deref())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GradedPoints {
    confidence: ConfidenceDto,
    points: BTreeMap<String, i32>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WeightKind {
    Attribute,
    Derived,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WeightDto {
    kind: WeightKind,
    #[serde(rename = "ref")]
    reference: String,
    weight: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DerivedDto {
    id: String,
    weights: Vec<WeightDto>,
    curve: String,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    floor: Option<i64>,
    #[serde(default)]
    cap: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassDto {
    id: String,
    /// Explicit rename (ADR-008): the id this entity had in the previous
    /// version. Never inferred.
    #[serde(default)]
    renamed_from: Option<String>,
    name: String,
    base_attributes: GradedPoints,
    derived: Vec<DerivedDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassFile {
    classes: Vec<ClassDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurveDto {
    id: String,
    /// Explicit rename (ADR-008): the id this entity had in the previous
    /// version. Never inferred.
    #[serde(default)]
    renamed_from: Option<String>,
    confidence: ConfidenceDto,
    points: Vec<[i64; 2]>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CurveFile {
    curves: Vec<CurveDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemDto {
    id: String,
    /// Explicit rename (ADR-008): the id this entity had in the previous
    /// version. Never inferred.
    #[serde(default)]
    renamed_from: Option<String>,
    name: String,
    #[serde(default)]
    armor_rating: Option<GradedMicro>,
    #[serde(default)]
    move_speed_add: Option<GradedMicro>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemFile {
    items: Vec<ItemDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EffectKind {
    AllAttributes,
    Attribute,
    RaiseCap,
    MoveSpeedAdd,
    MoveSpeedBonus,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectDto {
    confidence: ConfidenceDto,
    kind: EffectKind,
    #[serde(default)]
    points: Option<i32>,
    #[serde(default)]
    micro: Option<i64>,
    #[serde(default)]
    attribute: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PerkDto {
    id: String,
    /// Explicit rename (ADR-008): the id this entity had in the previous
    /// version. Never inferred.
    #[serde(default)]
    renamed_from: Option<String>,
    name: String,
    effects: Vec<EffectDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PerkFile {
    perks: Vec<PerkDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillDto {
    id: String,
    /// Explicit rename (ADR-008): the id this entity had in the previous
    /// version. Never inferred.
    #[serde(default)]
    renamed_from: Option<String>,
    name: String,
    effects: Vec<EffectDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillFile {
    skills: Vec<SkillDto>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn data_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn unknown_fields_are_rejected() {
        // ADR-004: an unrecognised key is a dataset error, not a shrug.
        let bad =
            r#"{"curves":[{"id":"c","confidence":"verified","points":[[0,0],[1,1]],"typo":1}]}"#;
        let err = serde_json::from_str::<CurveFile>(bad).unwrap_err();
        assert!(err.to_string().contains("typo"), "{err}");
    }

    #[test]
    fn unknown_confidence_grade_is_rejected() {
        let bad = r#"{"curves":[{"id":"c","confidence":"probably","points":[[0,0],[1,1]]}]}"#;
        assert!(serde_json::from_str::<CurveFile>(bad).is_err());
    }

    #[test]
    fn unknown_without_a_note_is_rejected() {
        // ADR-007: an assumption must say what was assumed.
        let dto = GradedMicro {
            confidence: ConfidenceDto::Unknown,
            micro: 1,
            note: None,
        };
        assert!(dto.into_fixed().is_err());
    }

    #[test]
    fn versions_lists_the_committed_builds() {
        let versions = versions(&data_root()).unwrap();
        assert!(
            versions.contains(&"0.17.150.9384".to_string()),
            "{versions:?}"
        );
    }

    #[test]
    fn missing_build_is_an_io_error_naming_the_path() {
        let err = load(&data_root(), "0.0.0.0").unwrap_err();
        assert!(matches!(err, LoadError::Io(_, _)), "{err}");
        assert!(err.to_string().contains("0.0.0.0"));
    }
}
