//! Submissions: what someone observed, offered to the dataset.
//!
//! The dataset is reviewed data. ADR-003 keeps the scraper outside the trust
//! boundary and has it produce proposals rather than facts, because a grade
//! that anyone can write is not a grade. Several people filling in a library
//! is the same problem with more hands, so it gets the same answer: a
//! submission is a proposal, and something has to promote it.
//!
//! Three things travel with every value, and they are the reason the format
//! exists rather than being a JSON blob of numbers:
//!
//! - **Who saw it, and when.** Two people agreeing independently is stronger
//!   evidence than one person insisting, and that is not knowable after the
//!   fact if submissions arrive anonymous.
//! - **How they saw it.** A number read by text recognition and the same
//!   number typed off the same screenshot carry different transcription risk.
//!   Recording the method lets review weigh them differently; guessing a
//!   grade here would throw that away.
//! - **What was not understood.** A card line this schema has no home for is
//!   kept, not dropped. Fifty submissions quietly discarding "Demon Damage
//!   Reduction" is fifty pieces of evidence that the schema is missing
//!   something, thrown away one at a time.

use std::collections::BTreeMap;
use std::path::PathBuf;

use assay_core::confidence::Confidence;
use assay_core::fixed::Fixed;
use assay_core::ids::{DerivedStatId, ItemId};
use assay_core::loadout::Slot;
use assay_core::schema::{AttributeBlockDelta, AttributeKind, ItemDef};
use serde::{Deserialize, Serialize};

use crate::LoadError;

/// How a value was observed. Never inferred from the value itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    /// Text recognition ran over a screenshot.
    ScreenshotOcr,
    /// A person read a screenshot and typed what it said.
    ScreenshotTyped,
    /// A person read the game and typed what it said.
    InGame,
    /// Taken from patch notes or a wiki page rather than seen.
    Documented,
}

impl Method {
    /// The grade a submission of this kind is *offered* at. Review may lower
    /// it and only a person may raise it: nothing here promotes itself.
    #[must_use]
    pub fn offered_grade(self) -> &'static str {
        match self {
            // Seeing the game is the only thing that verifies anything, and
            // even then a reviewer decides.
            Method::InGame | Method::ScreenshotTyped => "verified",
            Method::ScreenshotOcr | Method::Documented => "unverified",
        }
    }
}

/// One observed item, in the shape the dataset stores items.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ItemObservation {
    /// Proposed id (`item.leather_cap`).
    pub id: String,
    /// Name as the card prints it, without the rarity — that has its own
    /// field now, because a rarity inside a display name can only be got at
    /// by parsing a string written to be looked at.
    pub name: String,
    /// The rarity read off the card, if it printed one.
    #[serde(default)]
    pub rarity: Option<String>,
    /// Classes the card restricts it to, if any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_classes: Vec<String>,
    /// Where it is worn, as the card's `Slot Type` names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    /// Derived stats printed on it, as exact decimal strings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub grants: BTreeMap<String, String>,
    /// Attributes printed on it, in whole points.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, i32>,
    /// Flat move speed printed on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_speed_add: Option<String>,
}

/// One person's observations, offered for review.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Submission {
    /// Format version. Present so a reader can refuse a shape it predates
    /// rather than misread one.
    pub submission: u32,
    /// Who observed this. A handle is enough; the point is that two
    /// submissions can be told apart, not that anyone is identified.
    pub observer: String,
    /// When, as `YYYY-MM-DD`. Not parsed into a date: the format has no
    /// clock, and a string that a person wrote is what a person can check.
    pub observed_at: String,
    /// Which game build was running.
    pub build: String,
    /// How it was observed.
    pub method: Method,
    /// Anything the observer wants a reviewer to know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The items observed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ItemObservation>,
    /// Lines that were read but had nowhere to go. Kept deliberately: see
    /// this module's header.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unrecognised: Vec<String>,
}

/// The format this build of the tool writes and understands.
pub const FORMAT: u32 = 1;

/// The most a submission may be. Generous for anything a person would send
/// and small enough that a hostile file cannot be read into memory first and
/// rejected second.
pub const MAX_BYTES: usize = 1 << 20;

/// Refuses an id that would be indistinguishable from another one on screen.
///
/// `item.leather_cap` and `item.leather_cap ` are different ids and the same
/// picture. A reviewer reading `new  item.leather_cap` in a terminal has no
/// way to see the trailing space, approves it, and the dataset now holds two
/// items that look identical and disagree. Homoglyphs — a Cyrillic `а` among
/// Latin letters — are the same attack with better camouflage, and fall to
/// the same rule: the alphabet is small and explicit, so anything outside it
/// is named rather than rendered.
fn check_id(id: &str) -> Result<(), LoadError> {
    if id.is_empty() {
        return Err(LoadError::Invalid("an item needs an id".into()));
    }
    let bad: Vec<String> = id
        .chars()
        .filter(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '.' || *c == '_'))
        .map(|c| format!("{c:?} (U+{:04X})", u32::from(c)))
        .collect();
    if !bad.is_empty() {
        return Err(LoadError::Invalid(format!(
            "{id:?}: an id is lowercase ascii, digits, dots and underscores. Refused: {}. Two ids that look alike and are not is how a bad value gets past a reviewer.",
            bad.join(", ")
        )));
    }
    Ok(())
}

impl Submission {
    /// Reads a submission, refusing anything it cannot fully understand.
    ///
    /// # Errors
    /// Malformed JSON, an unknown field, a format version this build
    /// predates, or an empty submission.
    pub fn decode(text: &str) -> Result<Submission, LoadError> {
        let parsed: Submission = serde_json::from_str(text)
            .map_err(|e| LoadError::Schema(PathBuf::from("submission"), e))?;
        if parsed.submission > FORMAT {
            return Err(LoadError::Invalid(format!(
                "submission format {} is newer than this build understands ({FORMAT})",
                parsed.submission
            )));
        }
        if parsed.items.is_empty() && parsed.unrecognised.is_empty() {
            return Err(LoadError::Invalid(
                "a submission with nothing in it is not a submission".into(),
            ));
        }
        for item in &parsed.items {
            check_id(&item.id)?;
        }
        if parsed.observer.trim().is_empty() {
            return Err(LoadError::Invalid(
                "a submission needs an observer: two people agreeing is only \
                 evidence if you can tell them apart"
                    .into(),
            ));
        }
        Ok(parsed)
    }

    /// Writes a submission as canonical-ish JSON: readable, because a person
    /// is going to look at it and send it on.
    ///
    /// # Errors
    /// Only if serialisation itself fails, which it does not for this shape.
    pub fn encode(&self) -> Result<String, LoadError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| LoadError::Schema(PathBuf::from("submission"), e))
            .map(|s| s + "\n")
    }
}

impl ItemObservation {
    /// Turns an observation into the item definition it proposes, at the
    /// grade its method offers.
    ///
    /// # Errors
    /// An unknown attribute or slot name, or a value that is not an exact
    /// decimal — over-precision is refused rather than rounded, here as
    /// everywhere.
    pub fn to_item(&self, method: Method) -> Result<ItemDef, LoadError> {
        let grade = |value: Fixed| match method.offered_grade() {
            "verified" => Confidence::Verified(value),
            _ => Confidence::Unverified(value),
        };
        let parse = |raw: &str, what: &str| -> Result<Fixed, LoadError> {
            raw.parse::<Fixed>()
                .map_err(|e| LoadError::Invalid(format!("{}: {what} {raw:?}: {e:?}", self.id)))
        };

        let mut grants = BTreeMap::new();
        for (stat, raw) in &self.grants {
            grants.insert(DerivedStatId::new(stat), grade(parse(raw, stat)?));
        }

        let mut delta = AttributeBlockDelta::new();
        for (name, points) in &self.attributes {
            let kind = AttributeKind::ALL
                .into_iter()
                .find(|k| k.as_str() == name)
                .ok_or_else(|| LoadError::Invalid(format!("unknown attribute: {name}")))?;
            delta.insert(kind, *points);
        }

        Ok(ItemDef {
            id: ItemId::new(&self.id),
            name: self.name.clone(),
            rarity: self
                .rarity
                .as_deref()
                .map(|text| {
                    assay_core::stats::Rarity::parse(text).ok_or_else(|| {
                        LoadError::Invalid(format!("{}: unknown rarity {text:?}", self.id))
                    })
                })
                .transpose()?,
            required_classes: self
                .required_classes
                .iter()
                .map(assay_core::ids::ClassId::new)
                .collect(),
            slot: self
                .slot
                .as_deref()
                .map(|name| {
                    Slot::ALL
                        .into_iter()
                        .find(|s| s.as_str() == name)
                        .ok_or_else(|| LoadError::Invalid(format!("unknown slot: {name}")))
                })
                .transpose()?,
            attributes: if delta.is_empty() {
                None
            } else {
                Some(match method.offered_grade() {
                    "verified" => Confidence::Verified(delta),
                    _ => Confidence::Unverified(delta),
                })
            },
            grants,
            move_speed_add: self
                .move_speed_add
                .as_deref()
                .map(|raw| parse(raw, "moveSpeedAdd").map(grade))
                .transpose()?,
            weapon: None,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn one() -> &'static str {
        r#"{
          "submission": 1,
          "observer": "piper",
          "observedAt": "2026-08-20",
          "build": "0.17.150.9384",
          "method": "in-game",
          "items": [{
            "id": "item.leather_cap",
            "name": "Leather Cap",
            "rarity": "uncommon",
            "slot": "head",
            "grants": {"derived.armor_rating": "33"},
            "attributes": {"vigor": 2},
            "moveSpeedAdd": "-3"
          }]
        }"#
    }

    #[test]
    fn a_submission_round_trips() {
        let parsed = Submission::decode(one()).unwrap();
        let again = Submission::decode(&parsed.encode().unwrap()).unwrap();
        assert_eq!(again.items.len(), 1);
        assert_eq!(again.items[0].grants["derived.armor_rating"], "33");
    }

    #[test]
    fn the_method_decides_what_grade_is_offered() {
        // Text recognition offers unverified however confident it sounds;
        // only a person looking at the game offers verified, and a reviewer
        // still has to agree.
        let seen = Submission::decode(one()).unwrap();
        let item = seen.items[0].to_item(seen.method).unwrap();
        assert_eq!(
            item.grants[&DerivedStatId::new("derived.armor_rating")].level(),
            assay_core::confidence::ConfidenceLevel::Verified
        );
        let ocr = seen.items[0].to_item(Method::ScreenshotOcr).unwrap();
        assert_eq!(
            ocr.grants[&DerivedStatId::new("derived.armor_rating")].level(),
            assay_core::confidence::ConfidenceLevel::Unverified
        );
    }

    #[test]
    fn an_anonymous_or_empty_submission_is_refused() {
        // Both refusals are about evidence, not tidiness: an unattributed
        // observation cannot corroborate anything, and an empty one claims
        // nothing while looking like it claims something.
        let anon = one().replace(r#""observer": "piper""#, r#""observer": " ""#);
        assert!(Submission::decode(&anon).is_err());
        let empty =
            r#"{"submission":1,"observer":"p","observedAt":"x","build":"y","method":"in-game"}"#;
        assert!(Submission::decode(empty).is_err());
    }

    #[test]
    fn over_precision_is_refused_rather_than_rounded() {
        let too_fine = one().replace(r#""33""#, r#""33.1234567""#);
        let parsed = Submission::decode(&too_fine).unwrap();
        assert!(parsed.items[0].to_item(parsed.method).is_err());
    }

    #[test]
    fn an_id_that_looks_like_another_id_is_refused() {
        // The whole review step is a person reading a list of ids. Two that
        // render identically defeat it without any cleverness.
        for attack in [
            "item.leather_cap ", // trailing space
            " item.leather_cap", // leading space
            "item.leather_сap",  // Cyrillic es
            "item.Leather_Cap",  // case, which the dataset never uses
            "item.leather-cap",  // hyphen, likewise
        ] {
            let body = one().replace(r#""item.leather_cap""#, &format!("{attack:?}"));
            let refused = Submission::decode(&body);
            assert!(refused.is_err(), "accepted {attack:?}");
        }
        // And the legitimate form still passes.
        assert!(Submission::decode(one()).is_ok());
    }

    #[test]
    fn a_newer_format_is_refused_rather_than_misread() {
        let ahead = one().replace(r#""submission": 1"#, r#""submission": 99"#);
        assert!(Submission::decode(&ahead).is_err());
    }
}
