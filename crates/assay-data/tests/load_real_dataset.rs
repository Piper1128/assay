//! Loads the committed Hotfix 123 dataset and resolves against it.
//!
//! This is the end-to-end proof that the pieces line up: real JSON on disk →
//! loader → core resolver → the numbers the game's own character sheet shows.
//! The unit tests in `assay-core` assert the same values against hand-built
//! datasets; this asserts them against the file a human actually maintains.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use assay_core::derived::well_known;
use assay_core::loadout::{Loadout, PartyBuffs, Weapons};
use assay_core::{ClassId, ConfidenceLevel, Fixed, resolve};

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

const BUILD: &str = "0.17.150.9384";

fn naked_rogue() -> Loadout {
    Loadout {
        name: "naked-rogue".to_string(),
        class: ClassId::new("class.rogue"),
        perks: vec![],
        skills: vec![],
        armor: vec![],
        weapons: Weapons::default(),
        party: PartyBuffs::default(),
    }
}

#[test]
fn manifest_describes_the_build_it_lives_in() {
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    assert_eq!(dataset.manifest.build, BUILD);
    assert_eq!(dataset.manifest.label, "hotfix-123");
    assert!(
        dataset.manifest.sources.iter().all(|s| s.reviewed),
        "unreviewed data must never reach the core (ADR-003)"
    );
}

#[test]
fn naked_rogue_matches_the_games_character_sheet() {
    // Published for the Rogue at Patch 6.12 / Hotfix 123. Every one of these
    // falls out of the wiki's own curves through the rating model (ADR-012);
    // action speed in particular is unreachable from Agility alone.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let resolved = resolve(&naked_rogue(), &dataset.entities).expect("resolves");

    let expect = |id: &str, value: &str| {
        let got = *resolved
            .stat(id)
            .unwrap_or_else(|| panic!("{id} missing"))
            .value();
        let want: Fixed = value.parse().expect("test literal parses");
        assert_eq!(got, want, "{id}");
    };
    expect(well_known::PHYSICAL_POWER_BONUS, "-14");
    expect(well_known::ACTION_SPEED, "7.8125");
    expect(well_known::HEALTH, "108.5");
    expect(well_known::MOVE_SPEED, "306");
}

#[test]
fn wiki_sourced_values_stay_unverified() {
    // Two wiki pages agreeing is not independent verification (ADR-007), and
    // the tool must keep saying so rather than presenting these as fact.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let resolved = resolve(&naked_rogue(), &dataset.entities).expect("resolves");
    assert_eq!(
        resolved.stat(well_known::ACTION_SPEED).unwrap().level(),
        ConfidenceLevel::Unverified
    );
    // Wearing nothing is a certain fact, not a guess.
    assert_eq!(
        resolved.stat(well_known::ARMOR_RATING).unwrap().level(),
        ConfidenceLevel::Verified
    );
}

#[test]
fn every_class_resolves_in_every_committed_version() {
    // ADR-010 rev 2 §7 cross-version test: a fixture must resolve in every
    // available dataset version, or fail with an explicit, expected error.
    // It catches schema drift before a user meets it.
    for build in assay_data::versions(&data_root()).expect("versions listed") {
        let dataset = assay_data::load(&data_root(), &build)
            .unwrap_or_else(|e| panic!("{build} failed to load: {e}"));
        resolve(&naked_rogue(), &dataset.entities)
            .unwrap_or_else(|e| panic!("{build}: naked rogue failed to resolve: {e}"));
    }
}
