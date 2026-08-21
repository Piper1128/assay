//! Diffs the two committed dataset versions.
//!
//! Unlike `two_versions.rs`, which manufactures a change to exercise the
//! engine, this asserts a change that Ironmace actually shipped: the
//! Patch 6.12 notes for Hotfix 123 read *"Reduced the Additional Move Speed
//! per stack of Sprint from 15 to 13"*.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use assay_core::confidence::Confidence;
use assay_core::fixed::Fixed;
use assay_core::schema::DatasetSource;
use assay_core::{ClassId, Loadout, PartyBuffs, SkillId, Weapons};
use assay_diff::{Change, dataset_diff, impact_diff};

const HF122: &str = "0.17.149.9316";
const HF123: &str = "0.17.150.9384";

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn fighter_with_sprint() -> Loadout {
    Loadout {
        name: "fighter-sprint".to_string(),
        class: ClassId::new("class.fighter"),
        perks: vec![],
        skills: vec![SkillId::new("skill.fighter.sprint")],
        gear: vec![],
        weapons: Weapons::default(),
        stacks: BTreeMap::new(),
        party: PartyBuffs::default(),
    }
}

#[test]
fn the_hotfix_123_sprint_nerf_shows_up_as_a_data_change() {
    let before = assay_data::load(&data_root(), HF122).expect("hotfix-122 loads");
    let after = assay_data::load(&data_root(), HF123).expect("hotfix-123 loads");
    let changes = dataset_diff(&before, &after);

    assert!(
        changes.iter().any(|c| matches!(
            c,
            Change::Modified { id, from, to, .. }
                if id == "skill.fighter.sprint"
                    && from.contains("15")
                    && to.contains("13")
        )),
        "{changes:#?}"
    );
}

#[test]
fn the_nerf_reaches_a_build_that_runs_sprint() {
    // The question the tool exists to answer: did this patch move my numbers?
    let before = assay_data::load(&data_root(), HF122).expect("hotfix-122 loads");
    let after = assay_data::load(&data_root(), HF123).expect("hotfix-123 loads");
    let impacts = impact_diff(&before, &after, &[fighter_with_sprint()]);

    // Sprint carries three stacks, so the per-stack nerf costs 3 x 2 = 6
    // move speed at full stacks - not the 2 a single stack would suggest.
    let moved: Vec<_> = impacts[0].stats.iter().filter(|s| s.changed()).collect();
    assert_eq!(moved.len(), 1, "{:#?}", impacts[0].stats);
    assert_eq!(moved[0].id, "derived.move_speed");
    assert_eq!(moved[0].from.to_string(), "345");
    assert_eq!(moved[0].to.to_string(), "339");
    assert_eq!(moved[0].delta().to_string(), "-6");
    assert!(impacts[0].error.is_none());
}

#[test]
fn a_build_the_patch_did_not_touch_reports_unchanged() {
    // Just as important: the diff must stay quiet about what did not move,
    // or a reader learns to ignore it.
    let before = assay_data::load(&data_root(), HF122).expect("hotfix-122 loads");
    let after = assay_data::load(&data_root(), HF123).expect("hotfix-123 loads");
    let naked_rogue = Loadout {
        name: "naked-rogue".to_string(),
        class: ClassId::new("class.rogue"),
        perks: vec![],
        skills: vec![],
        gear: vec![],
        weapons: Weapons::default(),
        stacks: BTreeMap::new(),
        party: PartyBuffs::default(),
    };
    let impacts = impact_diff(&before, &after, &[naked_rogue]);
    assert!(impacts[0].stats.iter().all(|s| !s.changed()));
}

#[test]
fn the_weapon_nerfs_show_up_too() {
    // Regression: weapon profiles were added to the schema without being
    // added to the diff's field extractor, so the first real weapon patch
    // reported nothing at all. Every documented weapon change is asserted
    // here so a future field cannot go blind the same way.
    let before = assay_data::load(&data_root(), HF122).expect("hotfix-122 loads");
    let after = assay_data::load(&data_root(), HF123).expect("hotfix-123 loads");
    let changes = dataset_diff(&before, &after);

    let modified = |id: &str, field: &str, from: &str, to: &str| {
        changes.iter().any(|c| {
            matches!(
                c,
                Change::Modified { id: cid, field: cf, from: cfrom, to: cto }
                    if cid == id && cf == field && cfrom == from && cto == to
            )
        })
    };

    // Patch:6.12 Hotfix 123: "Flanged Mace/Morning Star weapon damage -1,
    // armor penetration from 15% to 10%", "War Hammer -1 damage",
    // "Club +1 damage".
    assert!(
        modified("item.flanged_mace", "weapon.base_damage", "32", "31"),
        "{changes:#?}"
    );
    assert!(
        modified("item.flanged_mace", "weapon.armor_pen", "15", "10"),
        "{changes:#?}"
    );
    assert!(
        modified("item.morning_star", "weapon.base_damage", "32", "31"),
        "{changes:#?}"
    );
    assert!(
        modified("item.morning_star", "weapon.armor_pen", "15", "10"),
        "{changes:#?}"
    );
    assert!(
        modified("item.war_hammer", "weapon.base_damage", "33", "32"),
        "{changes:#?}"
    );
    assert!(
        modified("item.club", "weapon.base_damage", "28", "29"),
        "{changes:#?}"
    );
}

#[test]
fn the_versions_are_chained_through_their_manifests() {
    let after = assay_data::load(&data_root(), HF123).expect("hotfix-123 loads");
    assert_eq!(after.manifest.previous.as_deref(), Some(HF122));
}

#[test]
fn a_patched_skill_shows_up_in_the_diff() {
    // The whole reason a skill's numbers moved into the dataset. While
    // Sneak Attack's scaling lived in a hand-written situation file, a
    // patch note changing it was invisible here — and `assay diff` exists
    // to answer exactly "what did this patch do to my build".
    let a = assay_data::load(&data_root(), "0.17.149.9316").expect("hf122 loads");
    let mut b = assay_data::load(&data_root(), "0.17.150.9384").expect("hf123 loads");

    // Nerf it by hand, the way a patch would.
    let id = SkillId::new("skill.rogue.sneak_attack");
    let mut nerfed = b.entities.skill(&id).expect("the skill is there").clone();
    let mut strike = nerfed.strike.clone().expect("it attacks");
    strike.flat_bonus = Some(Confidence::Verified(Fixed::from_int(12)));
    nerfed.strike = Some(strike);
    b.entities.insert_skill(nerfed);

    let changes = dataset_diff(&a, &b);
    let found = changes.iter().any(|c| match c {
        Change::Modified {
            id,
            field,
            from,
            to,
        } => {
            id.contains("sneak_attack")
                && field == "strike.flat_bonus"
                && from == "15"
                && to == "12"
        }
        _ => false,
    });
    assert!(found, "the nerf is invisible: {changes:?}");
}
