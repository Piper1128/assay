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
use assay_core::{AttributeKind, ClassId, ConfidenceLevel, Fixed, ItemId, PerkId, resolve};

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
    // Read off the in-game character sheet for a naked Rogue, not off a
    // wiki page. Every one falls out of the wiki's curves through the rating
    // model (ADR-012) -- action speed in particular is unreachable from
    // Agility alone -- but the base attributes had to be corrected first:
    // the wiki-sourced block said Strength 9 and Agility 25, and the game
    // says 10 and 24. Both sum to 105, so the error was invisible in the
    // total and visible in four derived stats at once.
    //
    // Move speed is the interesting one. The sheet prints 305, which looks
    // like a near miss against 305.4 -- but it also prints 101.8%, and
    // 305.4/300 is exactly 101.8%. The percentage carries the precision the
    // integer rounds away, so the two readings of the same sheet agree and
    // the fractional answer is the right one.
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
    expect(well_known::PHYSICAL_POWER_BONUS, "-11");
    expect(well_known::ACTION_SPEED, "7.5");
    expect(well_known::HEALTH, "109");
    expect(well_known::MOVE_SPEED, "305.4");
    expect(well_known::PDR, "-22");
    // The magic chain, from the same sheet: Will 10 -> 15 -> 1.5%.
    expect("derived.magic_resistance", "15");
    expect("derived.magical_damage_reduction", "1.5");
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

#[test]
fn the_magic_resistance_chain_runs_will_through_to_reduction() {
    // Two curves in series, and the second reads the first: Will feeds
    // Magic Resistance, Magic Resistance feeds Magical Damage Reduction.
    // ADR-012 allows a derived stat to be another one's input, and the
    // topological order is what makes the chain resolve at all.
    //
    // Rogue, Will 10: between the 5 and 15 breakpoints at +3/point, so
    // MR 15. On the reduction curve that is between 8 and 18 at 0.5/point:
    // -2 + 7 x 0.5 = 1.5.
    //
    // Fighter, Will 15: MR 30, which sits between 18 and 33 at 0.4/point:
    // 3 + 12 x 0.4 = 7.8. Two classes off the same pair of curves, the
    // same cross-check that settled the rating model.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let chain = |class: &str| {
        let mut loadout = naked_rogue();
        loadout.class = ClassId::new(class);
        let resolved = resolve(&loadout, &dataset.entities).expect("resolves");
        (
            *resolved
                .stat("derived.magic_resistance")
                .expect("MR")
                .value(),
            *resolved
                .stat("derived.magical_damage_reduction")
                .expect("MDR")
                .value(),
        )
    };
    assert_eq!(
        chain("class.rogue"),
        (Fixed::from_int(15), Fixed::from_micro(1_500_000))
    );
    assert_eq!(
        chain("class.fighter"),
        (Fixed::from_int(30), Fixed::from_micro(7_800_000))
    );
}

#[test]
fn magical_damage_reduction_cannot_reach_its_own_cap() {
    // A canary, not a rule. The wiki caps MDR at 65%, but Will alone tops
    // out at Magic Resistance 209 (Will 100), which the curve turns into
    // 47.8% — so the cap never binds and its value has never mattered.
    //
    // That matters because the wiki made exactly this claim about Physical
    // Damage Reduction, also 65%, and it was measured wrong in game (60
    // base, 75 with Defense Mastery).
    //
    // The premise is narrower than it looks, and the wiki misled us here
    // too. Gear *does* grant Magic Resistance -- an Epic pair of Loose
    // Trousers rolls +9 of it -- so Will is not the only source and the cap
    // is reachable in play. The model does not have gear-sourced Magic
    // Resistance yet, which is the only reason this holds; when it lands,
    // this test fails, and that failure is the signal that the 65% has
    // started to matter and has never been verified.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let mut loadout = naked_rogue();
    loadout.class = ClassId::new("class.fighter");
    loadout.armor = vec![ArmorPiece {
        // Will 15 + 85 = 100, the top of the documented conversion.
        id: ItemId::new("item.dark_leather_leggings"),
        rolls: vec![Roll::Attribute(AttributeKind::Will, 85)],
    }];
    let resolved = resolve(&loadout, &dataset.entities).expect("resolves");
    assert_eq!(
        *resolved
            .stat("derived.magic_resistance")
            .expect("MR")
            .value(),
        Fixed::from_int(209),
        "Will 100 is the end of the conversion table"
    );
    let mdr = *resolved
        .stat("derived.magical_damage_reduction")
        .expect("MDR")
        .value();
    assert_eq!(mdr, Fixed::from_micro(47_800_000));
    assert!(
        mdr < Fixed::from_int(65),
        "if the cap has started to bind, read this test's comment"
    );
}
