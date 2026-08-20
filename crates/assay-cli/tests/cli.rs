//! Drives the built binary against the committed dataset and loadouts.
//!
//! The unit tests cover parsing; this covers the thing a user actually runs,
//! including the exit codes a script would branch on.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assay(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_assay"))
        .current_dir(repo_root())
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

#[test]
fn resolve_prints_the_games_numbers() {
    let out = assay(&["resolve", "loadouts/naked-rogue.toml"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    // Read off the Rogue character sheet: action speed 7.5%, health 109,
    // PDR -22%, physical power bonus -11%, magic resistance 15 -> 1.5%.
    // Move speed prints as 305 there, but the sheet also shows 101.8%, and
    // 305.4/300 is exactly that -- the percentage carries the precision the
    // integer rounds away, so 305.4 is the answer and not a near miss.
    for expected in ["7.5", "109", "305.4", "-11", "1.5"] {
        assert!(text.contains(expected), "missing {expected} in:\n{text}");
    }
}

#[test]
fn confidence_is_visible_without_asking() {
    // ADR-007: the user must not have to opt in to knowing a number is a
    // guess.
    let text = stdout(&assay(&["resolve", "loadouts/naked-rogue.toml"]));
    assert!(text.contains('~'), "no confidence marker in:\n{text}");
    assert!(
        text.contains("not verified against the game"),
        "no summary line in:\n{text}"
    );
}

#[test]
fn explain_shows_every_pipeline_stage() {
    let text = stdout(&assay(&[
        "resolve",
        "loadouts/rogue-duo-buffed.toml",
        "--explain",
    ]));
    for stage in 1..=8 {
        assert!(
            text.contains(&format!("    {stage}. ")),
            "stage {stage} missing"
        );
    }
    // The stage-3-before-4 lock should be legible in the output itself.
    assert!(text.contains("attribute sum final"));
}

#[test]
fn json_output_is_the_canonical_form() {
    let text = stdout(&assay(&["resolve", "loadouts/naked-rogue.toml", "--json"]));
    let line = text.trim();
    assert!(line.starts_with('{') && line.ends_with('}'));
    assert!(!line.contains(' '), "canonical form carries no whitespace");
    assert!(line.contains("\"micro\":7500000"), "{line}");
}

#[test]
fn strict_exits_two_on_unverified_data() {
    // 2, not 1: a script must be able to tell "the data is not verified"
    // from "the tool failed".
    let out = assay(&["resolve", "loadouts/naked-rogue.toml", "--strict"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("below verified"), "{err}");
}

#[test]
fn unknown_options_and_commands_fail_loudly() {
    let bad_flag = assay(&["resolve", "loadouts/naked-rogue.toml", "--verbose"]);
    assert_eq!(bad_flag.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad_flag.stderr).contains("unknown option"));

    let bad_cmd = assay(&["frobnicate"]);
    assert_eq!(bad_cmd.status.code(), Some(1));
}

#[test]
fn diff_without_two_builds_says_which_exist() {
    // Better than an empty result that reads like "nothing changed".
    let out = assay(&["diff"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("needs two build ids"), "{err}");
    assert!(err.contains("0.17.150.9384"), "{err}");
}

#[test]
fn diffing_a_version_against_itself_reports_no_changes() {
    let build = "0.17.150.9384";
    let out = assay(&["diff", build, build, "--loadouts", "loadouts"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(text.contains("no data changes"), "{text}");
    assert!(text.contains("unchanged"), "{text}");
}

#[test]
fn versions_lists_the_committed_builds() {
    let text = stdout(&assay(&["versions"]));
    assert!(text.contains("0.17.150.9384"), "{text}");
    assert!(text.contains("hotfix-123"), "{text}");
}

/// Writes a submission to a temp file and returns its path.
fn submission(name: &str, body: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("assay-{name}.json"));
    std::fs::write(&path, body).expect("temp file writes");
    path
}

#[test]
fn a_submission_is_reviewed_and_not_applied() {
    // The default is to say what it would change and stop. Applying is a
    // second, deliberate act — that separation is the whole point of the
    // format existing (ADR-003).
    let path = submission(
        "new",
        r#"{"submission":1,"observer":"tester","observedAt":"2026-08-20",
            "build":"0.17.150.9384","method":"screenshot-ocr",
            "items":[{"id":"item.not_in_the_dataset","name":"Something",
                      "grants":{"derived.armor_rating":"7"}}]}"#,
    );
    let out = assay(&["submit", path.to_str().unwrap()]);
    let text = stdout(&out);
    assert!(text.contains("new       item.not_in_the_dataset"), "{text}");
    assert!(text.contains("--apply"), "it must say how to apply: {text}");
    assert!(
        text.contains("offered as unverified"),
        "text recognition offers unverified whatever it sounds like: {text}"
    );
    // And it really did not write: the item is still unknown to resolve.
    let again = assay(&["submit", path.to_str().unwrap()]);
    assert!(stdout(&again).contains("new       item.not_in_the_dataset"));
}

#[test]
fn a_disagreement_stops_everything_and_says_which_field() {
    // Two people reading one card differently is evidence that something is
    // wrong. Taking the newer number would throw it away exactly when it
    // showed up, so nothing is applied and the field is named.
    let path = submission(
        "conflict",
        r#"{"submission":1,"observer":"tester","observedAt":"2026-08-20",
            "build":"0.17.150.9384","method":"in-game",
            "items":[{"id":"item.leather_cap","name":"Leather Cap (Uncommon)",
                      "slot":"head","grants":{"derived.armor_rating":"38"}}]}"#,
    );
    let out = assay(&["submit", path.to_str().unwrap()]);
    let text = stdout(&out);
    assert!(text.contains("DISAGREES"), "{text}");
    assert!(
        text.contains("33000000") && text.contains("38000000"),
        "{text}"
    );
    assert!(text.contains("nothing applied"), "{text}");
    // 2, not 1: a script has to tell "they disagree" from "it broke", the
    // same distinction `--strict` already draws.
    assert_eq!(out.status.code(), Some(2), "{text}");
}

#[test]
fn a_grade_is_not_a_disagreement() {
    // A card read by text recognition offers `unverified` for a number the
    // dataset already has `verified`. That is corroboration, and calling it
    // a conflict would make every confirmation look like a fight.
    let path = submission(
        "corroborate",
        r#"{"submission":1,"observer":"tester","observedAt":"2026-08-20",
            "build":"0.17.150.9384","method":"screenshot-ocr",
            "items":[{"id":"item.leather_cap","name":"Leather Cap (Uncommon)",
                      "slot":"head",
                      "grants":{"derived.armor_rating":"33",
                                "derived.headshot_damage_reduction":"14"},
                      "attributes":{"vigor":2},"moveSpeedAdd":"-3"}]}"#,
    );
    let out = assay(&["submit", path.to_str().unwrap()]);
    let text = stdout(&out);
    assert!(!text.contains("DISAGREES"), "{text}");
    assert!(text.contains("already   item.leather_cap"), "{text}");
    assert_eq!(out.status.code(), Some(0), "{text}");
}

#[test]
fn a_line_with_nowhere_to_go_is_kept_and_shown() {
    // Fifty submissions quietly dropping "Demon Damage Reduction" is fifty
    // pieces of evidence about the schema, thrown away one at a time.
    let path = submission(
        "unrecognised",
        r#"{"submission":1,"observer":"tester","observedAt":"2026-08-20",
            "build":"0.17.150.9384","method":"screenshot-ocr",
            "items":[],"unrecognised":["Demon Damage Reduction 2.8%"]}"#,
    );
    let text = stdout(&assay(&["submit", path.to_str().unwrap()]));
    assert!(text.contains("Demon Damage Reduction"), "{text}");
    assert!(text.contains("not modelled"), "{text}");
}

#[test]
fn a_submission_against_another_build_is_refused() {
    // Numbers move between patches, so a submission and a dataset from
    // different builds are not comparable and a silent comparison would be
    // a wrong answer rather than an error.
    let path = submission(
        "wrong-build",
        r#"{"submission":1,"observer":"tester","observedAt":"2026-08-20",
            "build":"0.99.999.9999","method":"in-game",
            "items":[{"id":"item.x","name":"X","grants":{}}]}"#,
    );
    let out = assay(&["submit", path.to_str().unwrap()]);
    assert!(!out.status.success());
    let text = String::from_utf8_lossy(&out.stderr).to_string() + &stdout(&out);
    assert!(text.contains("0.99.999.9999"), "{text}");
}
