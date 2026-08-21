//! Who saw what (ADR-013).
//!
//! The dataset says what is true. This says why anyone thinks so.
//!
//! It lives beside the dataset rather than inside it because an item file is
//! read by a person deciding whether a number looks right, and burying four
//! lines of provenance under every value would cost that far more than it
//! buys.
//!
//! Nothing here promotes anything. It assembles evidence and reports what
//! that evidence would support; a person edits the grade and merges. ADR-003
//! does not move.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::LoadError;
use crate::submission::Method;

/// The file, inside a build's directory beside `items.json`.
pub const FILE: &str = "attestations.json";

/// One person, having seen one field, once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attestation {
    /// The handle that told the submissions apart.
    pub observer: String,
    /// The date the observer wrote, unparsed — the format has no clock, and
    /// a string a person wrote is what a person can check.
    pub at: String,
    /// How they saw it. This is what decides whether it corroborates.
    pub method: Method,
}

impl Attestation {
    /// Whether this reading can count toward corroborating another.
    ///
    /// `documented` cannot. A hundred people reading one wiki page is one
    /// source, and a grade that rose because a number was popular would
    /// launder repetition into verification — which is worse than no grade,
    /// because it would look like one.
    #[must_use]
    pub fn is_independent(&self) -> bool {
        let corroborates = self.method != Method::Documented; // probe: wiki-never-corroborates
        corroborates
    }
}

/// Every attestation, by item and then by the field it is about.
///
/// A field here is a *graded group* — the object that carries a
/// `confidence`, such as `grants.derived.armor_rating` — not the leaf inside
/// it. The grade applies to the group, so the evidence has to be about the
/// same thing the grade is about, or a promotion would be reasoning from
/// attestations of a different field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, transparent)]
pub struct Ledger {
    /// item id → field → who saw it.
    pub items: BTreeMap<String, BTreeMap<String, Vec<Attestation>>>,
}

impl Ledger {
    /// Reads the ledger for a build, or an empty one if it has none yet.
    ///
    /// A missing file is not an error: every build starts without evidence,
    /// and the dataset that exists today was authored by review rather than
    /// by submission.
    ///
    /// # Errors
    /// If the file exists and cannot be read or parsed.
    pub fn load(dir: &Path) -> Result<Self, LoadError> {
        let path = dir.join(FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| LoadError::Invalid(format!("{}: {e}", path.display())))?;
        serde_json::from_str(&text)
            .map_err(|e| LoadError::Invalid(format!("{}: {e}", path.display())))
    }

    /// Writes the ledger back.
    ///
    /// # Errors
    /// If the file cannot be written.
    pub fn save(&self, dir: &Path) -> Result<(), LoadError> {
        let path = dir.join(FILE);
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| LoadError::Invalid(e.to_string()))?
            + "\n";
        std::fs::write(&path, text)
            .map_err(|e| LoadError::Invalid(format!("{}: {e}", path.display())))
    }

    /// Records that someone saw a field.
    ///
    /// A second reading from the same observer replaces their first rather
    /// than adding to it. That is the mechanism the `observer` field was
    /// introduced for: one person reading a card twice is one witness, and
    /// counting them twice is how a single source becomes a majority.
    ///
    /// Returns whether this changed anything, so a caller can tell a new
    /// piece of evidence from a repeat.
    pub fn record(&mut self, item: &str, field: &str, attestation: Attestation) -> bool {
        let entries = self
            .items
            .entry(item.to_string())
            .or_default()
            .entry(field.to_string())
            .or_default();
        match entries
            .iter_mut()
            .find(|a| a.observer == attestation.observer)
        {
            Some(existing) if *existing == attestation => false,
            Some(existing) => {
                *existing = attestation;
                true
            }
            None => {
                entries.push(attestation);
                // Sorted, so the file does not reshuffle between runs and a
                // diff shows the reading that arrived rather than the order
                // it happened to land in.
                entries.sort_by(|a, b| a.observer.cmp(&b.observer));
                true
            }
        }
    }

    /// How many independent observers have seen this field.
    ///
    /// Distinct observers, none of them `documented`. Both halves matter and
    /// each is a different way to fool the count: the same person twice, or
    /// several people quoting one page.
    #[must_use]
    pub fn independent(&self, item: &str, field: &str) -> usize {
        self.items
            .get(item)
            .and_then(|fields| fields.get(field))
            .map(|list| {
                let mut seen: Vec<&str> = list
                    .iter()
                    .filter(|a| a.is_independent())
                    .map(|a| a.observer.as_str())
                    .collect();
                seen.sort_unstable();
                seen.dedup();
                seen.len()
            })
            .unwrap_or(0)
    }

    /// Whether the evidence would support raising this field to `verified`.
    ///
    /// The one rule ADR-013 locks, and the only one. It says *would*: nothing
    /// here changes a grade.
    #[must_use]
    pub fn supports_verified(&self, item: &str, field: &str) -> bool {
        self.independent(item, field) >= 2
    }

    /// Every field of an item that has any evidence, in order.
    #[must_use]
    pub fn fields(&self, item: &str) -> Vec<&str> {
        self.items
            .get(item)
            .map(|fields| fields.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn saw(observer: &str, method: Method) -> Attestation {
        Attestation {
            observer: observer.to_string(),
            at: "2026-08-21".to_string(),
            method,
        }
    }

    #[test]
    fn two_people_are_two_and_one_person_twice_is_one() {
        // The whole reason `observer` is a required field, finally consuming
        // it: one person reading a card twice is one witness.
        let mut led = Ledger::default();
        assert!(led.record(
            "item.cap",
            "grants.derived.armor_rating",
            saw("piper", Method::InGame)
        ));
        assert!(!led.record(
            "item.cap",
            "grants.derived.armor_rating",
            saw("piper", Method::InGame)
        ));
        assert_eq!(
            led.independent("item.cap", "grants.derived.armor_rating"),
            1
        );
        assert!(!led.supports_verified("item.cap", "grants.derived.armor_rating"));

        led.record(
            "item.cap",
            "grants.derived.armor_rating",
            saw("friend", Method::ScreenshotOcr),
        );
        assert_eq!(
            led.independent("item.cap", "grants.derived.armor_rating"),
            2
        );
        assert!(led.supports_verified("item.cap", "grants.derived.armor_rating"));
    }

    #[test]
    fn a_wiki_never_corroborates_however_many_people_quote_it() {
        // The failure this rule exists to prevent. A hundred readers of one
        // page is one source, and a grade that rose because a number was
        // popular would be worse than no grade at all.
        let mut led = Ledger::default();
        for who in ["a", "b", "c", "d"] {
            led.record(
                "item.cap",
                "grants.derived.armor_rating",
                saw(who, Method::Documented),
            );
        }
        assert_eq!(
            led.independent("item.cap", "grants.derived.armor_rating"),
            0
        );
        assert!(!led.supports_verified("item.cap", "grants.derived.armor_rating"));

        // One person who actually looked, plus all of that, is still one.
        led.record(
            "item.cap",
            "grants.derived.armor_rating",
            saw("piper", Method::InGame),
        );
        assert_eq!(
            led.independent("item.cap", "grants.derived.armor_rating"),
            1
        );
        assert!(!led.supports_verified("item.cap", "grants.derived.armor_rating"));
    }

    #[test]
    fn changing_your_mind_replaces_your_reading() {
        let mut led = Ledger::default();
        led.record("item.cap", "name", saw("piper", Method::ScreenshotOcr));
        assert!(led.record("item.cap", "name", saw("piper", Method::InGame)));
        let list = &led.items["item.cap"]["name"];
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].method, Method::InGame);
    }

    #[test]
    fn evidence_is_about_one_field_and_does_not_spread() {
        // Two people agreeing about the name says nothing about the armour.
        let mut led = Ledger::default();
        led.record("item.cap", "name", saw("piper", Method::InGame));
        led.record("item.cap", "name", saw("friend", Method::InGame));
        assert!(led.supports_verified("item.cap", "name"));
        assert!(!led.supports_verified("item.cap", "grants.derived.armor_rating"));
        assert!(!led.supports_verified("item.other", "name"));
    }

    #[test]
    fn a_build_with_no_ledger_yet_is_empty_rather_than_an_error() {
        let dir = std::env::temp_dir().join("assay-ledger-absent");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(FILE));
        assert_eq!(Ledger::load(&dir).unwrap(), Ledger::default());
    }

    #[test]
    fn it_round_trips() {
        let dir = std::env::temp_dir().join("assay-ledger-round-trip");
        std::fs::create_dir_all(&dir).unwrap();
        let mut led = Ledger::default();
        led.record(
            "item.cap",
            "grants.derived.armor_rating",
            saw("piper", Method::InGame),
        );
        led.record(
            "item.cap",
            "grants.derived.armor_rating",
            saw("friend", Method::ScreenshotTyped),
        );
        led.save(&dir).unwrap();
        assert_eq!(Ledger::load(&dir).unwrap(), led);
    }
}
