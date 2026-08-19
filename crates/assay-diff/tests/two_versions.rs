//! Diffs two dataset versions written to a temporary directory.
//!
//! The versions are synthetic and stay out of `data/`, which holds real game
//! data only: a diff test needs a *known* change, and manufacturing one in
//! the real dataset would put invented numbers where a reader expects
//! measurements.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use assay_core::{ClassId, Loadout, PartyBuffs, PerkId, Weapons};
use assay_diff::{Change, dataset_diff, impact_diff};

/// A version differing from the baseline only where the arguments say.
struct Version {
    /// Move speed the leggings take away.
    leggings_ms: &'static str,
    /// Extra JSON for the item table.
    extra_item: &'static str,
    /// Whether the Jokester perk exists.
    jokester: bool,
    /// Optional `renamed_from` on the leggings.
    renamed_from: Option<&'static str>,
}

fn write_version(root: &Path, build: &str, v: &Version) {
    let dir = root.join(build);
    fs::create_dir_all(&dir).unwrap();

    let files = [
        "classes.json",
        "curves.json",
        "items.json",
        "perks.json",
        "skills.json",
    ];
    let sources: Vec<String> = files
        .iter()
        .map(|f| {
            format!(
                r#"{{"file":"{f}","origin":"synthetic","scraped":"2026-08-19T00:00:00Z","reviewed":true}}"#
            )
        })
        .collect();
    write(
        &dir,
        "manifest.json",
        &format!(
            r#"{{"build":"{build}","label":"test-{build}","released":"2026-08-13","previous":null,"sources":[{}]}}"#,
            sources.join(",")
        ),
    );

    write(
        &dir,
        "curves.json",
        r#"{"curves":[
          {"id":"curve.ms","confidence":"unverified","points":[[0,0],[100000000,10000000]]}
        ]}"#,
    );
    write(
        &dir,
        "classes.json",
        r#"{"classes":[{"id":"class.rogue","name":"Rogue",
          "base_attributes":{"confidence":"unverified","points":{"agility":25}},
          "derived":[{"id":"derived.move_speed",
            "weights":[{"kind":"attribute","ref":"agility","weight":1000000}],
            "curve":"curve.ms","offset":300000000}]}]}"#,
    );

    let rename = v
        .renamed_from
        .map(|from| format!(r#","renamed_from":"{from}""#))
        .unwrap_or_default();
    write(
        &dir,
        "items.json",
        &format!(
            r#"{{"items":[{{"id":"item.leggings","name":"Leggings"{rename},
              "move_speed_add":{{"confidence":"unverified","micro":{}}}}}{}]}}"#,
            v.leggings_ms, v.extra_item
        ),
    );

    let perks = if v.jokester {
        r#"{"perks":[{"id":"perk.rogue.jokester","name":"Jokester",
          "effects":[{"confidence":"unverified","kind":"all_attributes","points":2}]}]}"#
    } else {
        r#"{"perks":[]}"#
    };
    write(&dir, "perks.json", perks);
    write(&dir, "skills.json", r#"{"skills":[]}"#);
}

fn write(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("assay-diff-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

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
fn reports_adds_removes_and_field_changes() {
    let root = temp_root("basic");
    write_version(
        &root,
        "1.0.0.0",
        &Version {
            leggings_ms: "-5000000",
            extra_item: "",
            jokester: true,
            renamed_from: None,
        },
    );
    write_version(
        &root,
        "1.0.0.1",
        &Version {
            // The leggings' penalty softened, a boot appeared, Jokester left.
            leggings_ms: "-4000000",
            extra_item: r#",{"id":"item.boots","name":"Boots"}"#,
            jokester: false,
            renamed_from: None,
        },
    );

    let before = assay_data::load(&root, "1.0.0.0").unwrap();
    let after = assay_data::load(&root, "1.0.0.1").unwrap();
    let changes = dataset_diff(&before, &after);

    assert!(
        changes
            .iter()
            .any(|c| matches!(c, Change::Added { id, .. } if id == "item.boots")),
        "{changes:#?}"
    );
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, Change::Removed { id, .. } if id == "perk.rogue.jokester")),
        "{changes:#?}"
    );
    assert!(
        changes.iter().any(|c| matches!(
            c,
            Change::Modified { id, field, from, to }
                if id == "item.leggings" && field == "move_speed_add" && from == "-5" && to == "-4"
        )),
        "{changes:#?}"
    );
}

#[test]
fn renames_are_followed_rather_than_reported_as_add_plus_remove() {
    let root = temp_root("rename");
    write_version(
        &root,
        "2.0.0.0",
        &Version {
            leggings_ms: "-5000000",
            extra_item: "",
            jokester: false,
            renamed_from: None,
        },
    );
    // Same entity, new id, stated explicitly.
    let dir = root.join("2.0.0.1");
    write_version(
        &root,
        "2.0.0.1",
        &Version {
            leggings_ms: "-5000000",
            extra_item: "",
            jokester: false,
            renamed_from: None,
        },
    );
    fs::write(
        dir.join("items.json"),
        r#"{"items":[{"id":"item.dark_leggings","name":"Leggings","renamed_from":"item.leggings",
          "move_speed_add":{"confidence":"unverified","micro":-4000000}}]}"#,
    )
    .unwrap();

    let before = assay_data::load(&root, "2.0.0.0").unwrap();
    let after = assay_data::load(&root, "2.0.0.1").unwrap();
    let changes = dataset_diff(&before, &after);

    assert!(
        changes.iter().any(|c| matches!(
            c,
            Change::Renamed { from_id, to_id }
                if from_id == "item.leggings" && to_id == "item.dark_leggings"
        )),
        "{changes:#?}"
    );
    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, Change::Added { .. } | Change::Removed { .. })),
        "a rename must not surface as an add plus a remove: {changes:#?}"
    );
    // Fields are still compared across the rename.
    assert!(
        changes.iter().any(|c| matches!(
            c,
            Change::Modified { id, field, .. }
                if id == "item.dark_leggings" && field == "move_speed_add"
        )),
        "{changes:#?}"
    );
}

#[test]
fn impact_diff_answers_did_my_build_change() {
    let root = temp_root("impact");
    write_version(
        &root,
        "3.0.0.0",
        &Version {
            leggings_ms: "-5000000",
            extra_item: "",
            jokester: true,
            renamed_from: None,
        },
    );
    write_version(
        &root,
        "3.0.0.1",
        &Version {
            leggings_ms: "-5000000",
            extra_item: "",
            jokester: true,
            renamed_from: None,
        },
    );
    // Only the class's move-speed baseline moved: 300 -> 295.
    let classes = fs::read_to_string(root.join("3.0.0.1/classes.json")).unwrap();
    fs::write(
        root.join("3.0.0.1/classes.json"),
        classes.replace("300000000", "295000000"),
    )
    .unwrap();

    let before = assay_data::load(&root, "3.0.0.0").unwrap();
    let after = assay_data::load(&root, "3.0.0.1").unwrap();
    let impacts = impact_diff(&before, &after, &[naked_rogue()]);

    assert_eq!(impacts.len(), 1);
    let moved: Vec<_> = impacts[0].stats.iter().filter(|s| s.changed()).collect();
    assert_eq!(moved.len(), 1, "{:#?}", impacts[0].stats);
    assert_eq!(moved[0].id, "derived.move_speed");
    assert_eq!(moved[0].delta().to_string(), "-5");
    assert!(impacts[0].error.is_none());
}

#[test]
fn a_loadout_that_stops_resolving_is_reported_not_hidden() {
    let root = temp_root("broken");
    write_version(
        &root,
        "4.0.0.0",
        &Version {
            leggings_ms: "-5000000",
            extra_item: "",
            jokester: true,
            renamed_from: None,
        },
    );
    write_version(
        &root,
        "4.0.0.1",
        &Version {
            leggings_ms: "-5000000",
            extra_item: "",
            jokester: false, // the perk the loadout slots is gone
            renamed_from: None,
        },
    );

    let before = assay_data::load(&root, "4.0.0.0").unwrap();
    let after = assay_data::load(&root, "4.0.0.1").unwrap();
    let mut loadout = naked_rogue();
    loadout.perks = vec![PerkId::new("perk.rogue.jokester")];

    let impacts = impact_diff(&before, &after, &[loadout]);
    let error = impacts[0].error.as_ref().expect("the break is reported");
    assert!(error.contains("perk.rogue.jokester"), "{error}");
}

#[test]
fn an_unchanged_version_produces_no_changes() {
    let root = temp_root("identical");
    let version = Version {
        leggings_ms: "-5000000",
        extra_item: "",
        jokester: true,
        renamed_from: None,
    };
    write_version(&root, "5.0.0.0", &version);
    write_version(&root, "5.0.0.1", &version);

    let before = assay_data::load(&root, "5.0.0.0").unwrap();
    let after = assay_data::load(&root, "5.0.0.1").unwrap();
    assert!(dataset_diff(&before, &after).is_empty());
}
