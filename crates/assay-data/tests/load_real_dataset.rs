//! Loads the committed Hotfix 123 dataset and resolves against it.
//!
//! This is the end-to-end proof that the pieces line up: real JSON on disk →
//! loader → core resolver → the numbers the game's own character sheet shows.
//! The unit tests in `assay-core` assert the same values against hand-built
//! datasets; this asserts them against the file a human actually maintains.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use assay_core::derived::well_known;
use assay_core::loadout::{ArmorPiece, Loadout, PartyBuffs, Roll, Weapons};
use assay_core::{ClassId, ConfidenceLevel, Fixed, ItemId, PerkId, resolve};

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
        stacks: BTreeMap::new(),
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
fn the_armor_curve_hits_its_known_anchors() {
    // The curve is transcribed from the wiki's conversion table, whose rows
    // are internally consistent and independently reproduce the -22% the
    // page states for a character wearing nothing. Both anchors are pinned
    // here so a future edit that breaks them fails loudly.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");

    let naked = resolve(&naked_rogue(), &dataset.entities).expect("resolves");
    assert_eq!(
        *naked.stat(well_known::PDR).unwrap().value(),
        "-22".parse::<Fixed>().unwrap(),
        "a character with no armour sits at -22% PDR"
    );

    // Armour rating 36 lands inside the 20..75 segment, at 0.15% per point:
    // 4.2 + 0.15 x 16 = 6.6.
    let geared = Loadout {
        name: "armored".to_string(),
        class: ClassId::new("class.rogue"),
        perks: vec![],
        skills: vec![],
        armor: vec![assay_core::ArmorPiece {
            id: assay_core::ItemId::new("item.dark_leather_leggings"),
            rolls: vec![],
        }],
        weapons: Default::default(),
        stacks: BTreeMap::new(),
        party: PartyBuffs::default(),
    };
    let resolved = resolve(&geared, &dataset.entities).expect("resolves");
    assert_eq!(
        *resolved.stat(well_known::ARMOR_RATING).unwrap().value(),
        "36".parse::<Fixed>().unwrap()
    );
    assert_eq!(
        *resolved.stat(well_known::PDR).unwrap().value(),
        "6.6".parse::<Fixed>().unwrap()
    );
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

#[test]
fn defense_mastery_multiplies_worn_armour_but_not_enchantments() {
    // The perk reads "gain an additional 15% Item Armor Rating Bonus from
    // equipped armor, and raise your Physical Damage Reduction cap to 75%".
    // We modelled the cap for a long time and the multiplier not at all, so
    // the perk changed a ceiling nothing could reach and nothing else
    // (ADR-005 amendment: item armor bonus).
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let leggings = ItemId::new("item.dark_leather_leggings");

    let build = |perks: Vec<PerkId>, rolls: Vec<Roll>| {
        let loadout = Loadout {
            name: "fighter".to_string(),
            class: ClassId::new("class.fighter"),
            perks,
            skills: vec![],
            armor: vec![ArmorPiece {
                id: leggings.clone(),
                rolls,
            }],
            weapons: Weapons::default(),
            stacks: BTreeMap::new(),
            party: PartyBuffs::default(),
        };
        *resolve(&loadout, &dataset.entities)
            .expect("fighter resolves")
            .stat(well_known::ARMOR_RATING)
            .expect("armour rating is defined")
            .value()
    };

    let mastery = vec![PerkId::new("perk.fighter.defense_mastery")];
    assert_eq!(build(vec![], vec![]), Fixed::from_micro(36_000_000));
    // 36 × 1.15, exactly — no rounding to argue about.
    assert_eq!(
        build(mastery.clone(), vec![]),
        Fixed::from_micro(41_400_000)
    );
    // With 10 enchanted on: 41.4 + 10. Were the enchantment inside the
    // multiplier's base it would be 52.9 instead.
    assert_eq!(
        build(mastery, vec![Roll::ArmorRating(Fixed::from_int(10))]),
        Fixed::from_micro(51_400_000)
    );
}
