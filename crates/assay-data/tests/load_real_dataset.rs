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
use assay_core::loadout::{GearPiece, Loadout, PartyBuffs, Roll, Slot, Weapons};
use assay_core::schema::DatasetSource;
use assay_core::{
    AttributeKind, ClassId, ConfidenceLevel, DerivedStatId, Fixed, ItemId, PerkId, resolve,
};
use assay_data::DatasetText;

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
        gear: vec![],
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
        gear: vec![assay_core::GearPiece {
            slot: Slot::Legs,
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
            gear: vec![GearPiece {
                slot: Slot::Legs,
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
        build(
            mastery,
            vec![Roll::Derived(
                DerivedStatId::new(well_known::ARMOR_RATING),
                Fixed::from_int(10)
            )]
        ),
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
    loadout.gear = vec![GearPiece {
        slot: Slot::Legs,
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

#[test]
fn three_item_cards_reach_the_block_through_every_route_they_have() {
    // Read off three cards in game, in three slots. One loadout exercises
    // everything the gear amendment added, and each assertion below fails
    // for a different reason if one route is wrong.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let mut loadout = naked_rogue();
    loadout.gear = vec![
        GearPiece {
            slot: Slot::Head,
            id: ItemId::new("item.leather_cap"),
            rolls: vec![Roll::Derived(
                DerivedStatId::new(well_known::ARMOR_RATING),
                Fixed::from_int(11),
            )],
        },
        GearPiece {
            slot: Slot::Legs,
            id: ItemId::new("item.loose_trousers"),
            rolls: vec![
                Roll::Attribute(AttributeKind::Strength, 2),
                Roll::Derived(
                    DerivedStatId::new("derived.magic_resistance"),
                    Fixed::from_int(9),
                ),
            ],
        },
        GearPiece {
            slot: Slot::Necklace,
            id: ItemId::new("item.phoenix_choker"),
            rolls: vec![],
        },
    ];
    let r = resolve(&loadout, &dataset.entities).expect("resolves");
    let block = r.attributes.value();

    // Printed on the items: Vigor 2 on the cap, Agility 4 on the trousers.
    assert_eq!(block.get(AttributeKind::Vigor).points(), 8, "6 + 2 printed");
    assert_eq!(
        block.get(AttributeKind::Agility).points(),
        28,
        "24 + 4 printed"
    );
    // Rolled on this copy of the trousers.
    assert_eq!(
        block.get(AttributeKind::Strength).points(),
        12,
        "10 + 2 rolled"
    );

    let stat = |id: &str| *r.stat(id).unwrap_or_else(|| panic!("{id} missing")).value();
    // 33 printed + 25 printed + 11 rolled. The rolled 11 is outside any
    // Item Armor Rating Bonus, which is what the game calls it: Additional.
    assert_eq!(stat(well_known::ARMOR_RATING), Fixed::from_int(69));
    // A necklace grants stats, and they are not attributes. Magical Power
    // is 11 rather than the choker's 1: Will feeds it the way Strength
    // feeds Physical Power, so the choker adds to the 10 the Rogue already
    // had. Before the magic chain existed this read 1, which was the
    // choker's contribution going nowhere.
    assert_eq!(stat("derived.magical_power"), Fixed::from_int(11));
    assert_eq!(stat("derived.magic_penetration"), Fixed::from_int(1));
    // A cap grants a defensive stat nothing computes.
    assert_eq!(
        stat("derived.headshot_damage_reduction"),
        Fixed::from_int(14)
    );
    // The one that is easy to get wrong: gear ADDS to Magic Resistance
    // rather than replacing it. Will 10 gives 15; the trousers roll 9.
    // Replacing would give 9, and the chain below it would read -1.5%.
    assert_eq!(stat("derived.magic_resistance"), Fixed::from_int(24));
    assert_eq!(
        stat("derived.magical_damage_reduction"),
        Fixed::from_micro(5_400_000)
    );
}

#[test]
fn the_move_speed_cap_binds_before_armour_takes_its_cut() {
    // Confirmed in game: Agility gives base move speed, and armour reduces
    // it flat afterwards. So the 330 cap applies to the base, not to the
    // result, and the order is worth a test because the two readings differ
    // only for a build fast enough to reach the cap — which is exactly the
    // build that would notice.
    //
    // Agility 75 puts the curve at 336, over the cap. A cap and a pair of
    // trousers cost 3 and 2. Capping first gives 330 - 5 = 325; folding the
    // penalty into the rating first would give min(331, 330) = 330, and the
    // difference is a whole point of move speed on every fast build.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let mut loadout = naked_rogue();
    loadout.gear = vec![
        GearPiece {
            slot: Slot::Head,
            id: ItemId::new("item.leather_cap"),
            rolls: vec![],
        },
        GearPiece {
            slot: Slot::Legs,
            id: ItemId::new("item.loose_trousers"),
            rolls: vec![Roll::Attribute(AttributeKind::Agility, 47)],
        },
    ];
    let resolved = resolve(&loadout, &dataset.entities).expect("resolves");
    assert_eq!(
        resolved
            .attributes
            .value()
            .get(AttributeKind::Agility)
            .points(),
        75
    );
    // Stage 4 caps at 330; the breakdown still shows what the curve wanted.
    let parts = resolved
        .breakdown
        .get(&DerivedStatId::new(well_known::MOVE_SPEED))
        .expect("move speed breakdown");
    assert_eq!(*parts.from_rating.value(), Fixed::from_int(336));
    // Stage 5 then takes the armour's cut off the capped value.
    assert_eq!(
        *resolved.stat(well_known::MOVE_SPEED).unwrap().value(),
        Fixed::from_int(325)
    );
}

#[test]
fn the_whole_character_sheet_comes_back() {
    // Every stat the game printed for this Rogue, in one place. Each curve
    // was transcribed from a wiki that has contradicted itself three times
    // in this project, so each one had to reproduce its line here before it
    // was written — one anchor per stat, from a source that is not the wiki.
    //
    // This is the test that would notice a curve drifting. A single stat can
    // be wrong quietly; the whole sheet cannot.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let resolved = resolve(&naked_rogue(), &dataset.entities).expect("resolves");

    for (id, printed) in [
        ("derived.physical_power", "10"),
        ("derived.physical_power_bonus", "-11"),
        ("derived.action_speed", "7.5"),
        ("derived.health", "109"),
        // The sheet shows 305 and also 101.8%; 305.4/300 is exactly that, so
        // the integer is the rounding and the fraction is the answer.
        ("derived.move_speed", "305.4"),
        ("derived.pdr", "-22"),
        ("derived.magic_resistance", "15"),
        ("derived.magical_damage_reduction", "1.5"),
        ("derived.memory_capacity", "4"),
        ("derived.spell_casting_speed", "-15"),
        ("derived.cooldown_reduction_bonus", "15"),
        ("derived.persuasiveness", "20"),
        ("derived.buff_duration_bonus", "-11"),
        ("derived.magical_interaction_speed", "-25"),
        ("derived.manual_dexterity", "15"),
        ("derived.equip_speed", "25"),
        ("derived.health_recovery_bonus", "-27"),
    ] {
        let got = resolved
            .stat(id)
            .unwrap_or_else(|| panic!("{id} is not in the dataset"))
            .value();
        let want: Fixed = printed.parse().expect("test literal parses");
        assert_eq!(*got, want, "{id}");
    }
}

#[test]
fn a_stat_with_a_field_of_its_own_cannot_hide_in_grants() {
    // The Arming Sword sat in the dataset with `derived.weapon_damage` among
    // its grants. It loaded, it resolved, it contributed every stat it had —
    // and it could not be swung, because nothing reads weapon damage there.
    // A value that lands somewhere and does nothing is the failure this
    // project is built against, so the loader refuses the spelling now.
    let text = DatasetText {
        manifest: r#"{"build":"x","label":"t","released":"2026-01-01","sources":[]}"#.to_string(),
        classes: r#"{"classes":[]}"#.to_string(),
        curves: r#"{"curves":[]}"#.to_string(),
        items: r#"{"items":[{"id":"item.x","name":"X","grants":{
            "derived.weapon_damage":{"confidence":"verified","micro":32000000}}}]}"#
            .to_string(),
        perks: r#"{"perks":[]}"#.to_string(),
        skills: r#"{"skills":[]}"#.to_string(),
    };
    let refused = assay_data::decode(&text, "x");
    let message = match refused {
        Err(e) => e.to_string(),
        Ok(_) => panic!("a grant with a field of its own was accepted"),
    };
    // And it says where the value belongs, because the person who wrote it
    // there was not being careless — the schema has two plausible homes.
    assert!(message.contains("weapon.base_damage"), "{message}");
}

#[test]
fn the_arming_sword_can_actually_be_swung() {
    // The regression the rule above exists for: it is in the dataset as a
    // weapon, not as an item that merely mentions weapon damage.
    let dataset = assay_data::load(&data_root(), BUILD).expect("dataset loads");
    let sword = dataset
        .entities
        .item(&ItemId::new("item.arming_sword"))
        .expect("the sword is in the dataset");
    let weapon = sword.weapon.as_ref().expect("and it is a weapon");
    assert_eq!(*weapon.base_damage.value(), Fixed::from_int(32));
}
