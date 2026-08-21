//! `assay submit` — reviewing what somebody else observed.
//!
//! Reading a submission is not applying one. The default is to say what it
//! would change and stop, because the whole reason submissions exist as a
//! separate thing is that a person stands between an observation and the
//! dataset (ADR-003).
//!
//! A submission that disagrees with the dataset is refused whole rather than
//! applied in part. Two people who read the same card differently are
//! evidence that something is wrong — a rarity nobody recorded, a patch
//! nobody noticed, a misread digit — and quietly taking the newer number
//! throws that evidence away at the exact moment it appeared.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use assay_core::schema::ItemDef;
use assay_data::attestations::{Attestation, Ledger};
use assay_data::submission::{ItemObservation, Method, Submission};
use serde_json::{Map, Value};

use crate::newest_build;

/// What reviewing a submission found.
struct Review {
    /// Items the dataset does not have.
    fresh: Vec<(String, Value)>,
    /// Items already present and identical.
    known: Vec<String>,
    /// Items present and different, field by field.
    conflicts: Vec<Conflict>,
}

struct Conflict {
    item: String,
    field: String,
    dataset: String,
    submitted: String,
}

pub(crate) fn cmd_submit(args: &[String]) -> Result<ExitCode, String> {
    let mut path: Option<PathBuf> = None;
    let mut data = PathBuf::from("data");
    let mut build: Option<String> = None;
    let mut apply = false;

    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--apply" => apply = true,
            "--data" => data = PathBuf::from(rest.next().ok_or("--data needs a directory")?),
            "--build" => build = Some(rest.next().ok_or("--build needs an id")?.clone()),
            other if other.starts_with("--") => return Err(format!("unknown option: {other}")),
            other => path = Some(PathBuf::from(other)),
        }
    }
    let path = path.ok_or("submit needs a submission file")?;

    // Size is checked before reading, not after. A submission arrives from
    // someone else now, and "read it all in, then decide it was too big" is
    // the wrong order when the file is the attacker's choice.
    let size = std::fs::metadata(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size > assay_data::submission::MAX_BYTES as u64 {
        return Err(format!(
            "{}: {size} bytes, and a submission may be at most {}. Nothing a person sends is that large.",
            path.display(),
            assay_data::submission::MAX_BYTES
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let submission = Submission::decode(&text).map_err(|e| e.to_string())?;

    let build = match build {
        Some(id) => id,
        None => newest_build(&data)?,
    };
    if submission.build != build {
        return Err(format!(
            "the submission is against build {} and this is {build}. \
             Numbers move between patches, so the two are not comparable; \
             pass --build to review it against its own.",
            submission.build
        ));
    }

    let items_path = data.join(&build).join("items.json");
    let raw = std::fs::read_to_string(&items_path)
        .map_err(|e| format!("{}: {e}", items_path.display()))?;
    let mut file: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    let review = review(&submission, &file)?;
    report(&submission, &review);

    if !review.conflicts.is_empty() {
        println!(
            "\n  nothing applied: {} field(s) disagree with the dataset.\n  \
             Settle each one first — a disagreement is the most informative \
             thing a submission can contain.",
            review.conflicts.len()
        );
        return Ok(ExitCode::from(2));
    }
    if !apply {
        if !review.fresh.is_empty() {
            println!(
                "\n  pass --apply to write {} item(s) in.",
                review.fresh.len()
            );
        }
        return Ok(ExitCode::SUCCESS);
    }
    // A submission that changes nothing used to stop here. That threw away
    // the corroboration ADR-013 is about: someone independently reading a
    // value the dataset already has is the strongest thing that can happen
    // to it, and reporting it as "nothing to apply" discarded it one
    // submission at a time.
    let build_dir = data.join(&build);
    let promotions = attest(&submission, &review, &build_dir)?;

    if review.fresh.is_empty() {
        println!("\n  nothing new to write; the readings are recorded.");
        report_promotions(&promotions);
        return Ok(ExitCode::SUCCESS);
    }

    let list = file
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .ok_or("items.json has no items array")?;
    for (_, item) in &review.fresh {
        list.push(item.clone());
    }
    list.sort_by(|a, b| {
        a.get("id")
            .and_then(Value::as_str)
            .cmp(&b.get("id").and_then(Value::as_str))
    });
    let written = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&items_path, written).map_err(|e| format!("{}: {e}", items_path.display()))?;
    println!(
        "\n  wrote {} item(s) into {}. Review the diff before committing: \
         `git diff` is the second half of this step.",
        review.fresh.len(),
        items_path.display()
    );
    report_promotions(&promotions);
    Ok(ExitCode::SUCCESS)
}

/// A field the evidence would now support raising, and what it says today.
struct Promotion {
    item: String,
    field: String,
    observers: usize,
}

/// Records who saw what, and returns the fields the ledger now supports
/// raising (ADR-013).
///
/// Called only on `--apply`. A review has to stay read-only, or looking at a
/// submission would change the thing being looked at.
fn attest(
    submission: &Submission,
    review: &Review,
    build_dir: &std::path::Path,
) -> Result<Vec<Promotion>, String> {
    let mut ledger = Ledger::load(build_dir).map_err(|e| format!("{e:?}"))?;
    let mut proposals = Vec::new();

    for observed in &submission.items {
        let def = observed
            .to_item(submission.method)
            .map_err(|e| e.to_string())?;
        let rendered = to_json(observed, submission.method, &def);
        let mut fields = Vec::new();
        graded_groups(&rendered, "", &mut fields);

        for field in fields {
            ledger.record(
                &observed.id,
                &field,
                Attestation {
                    observer: submission.observer.clone(),
                    at: submission.observed_at.clone(),
                    method: submission.method,
                },
            );
            // A promotion is only interesting where the dataset is actually
            // below what the evidence supports. A field already verified has
            // nowhere to go, and one this submission is adding for the first
            // time cannot have two observers behind it.
            if !ledger.supports_verified(&observed.id, &field) {
                continue;
            }
            if grade_at(&rendered, &field) != Some("unverified") {
                continue;
            }
            proposals.push(Promotion {
                item: observed.id.clone(),
                field: field.clone(),
                observers: ledger.independent(&observed.id, &field),
            });
        }
    }
    let _ = review;
    ledger.save(build_dir).map_err(|e| format!("{e:?}"))?;
    Ok(proposals)
}

/// The paths a grade attaches to, inside one rendered item.
///
/// A graded group is an object carrying a `confidence`, and the grade covers
/// the whole group — so the evidence has to be about the group rather than a
/// leaf inside it, or a promotion would be reasoning from attestations of a
/// different field. Scalars outside any group get a path of their own: they
/// are observations too, they simply have no grade to raise.
fn graded_groups(value: &Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.contains_key("confidence") {
                if !prefix.is_empty() {
                    out.push(prefix.to_string());
                }
                return;
            }
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                graded_groups(child, &path, out);
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.push(prefix.to_string());
            }
        }
    }
}

/// The grade a rendered item carries at a graded-group path.
fn grade_at<'a>(item: &'a Value, field: &str) -> Option<&'a str> {
    let mut node = item;
    for step in field.split('.') {
        node = node.get(step)?;
    }
    node.get("confidence")?.as_str()
}

fn report_promotions(promotions: &[Promotion]) {
    if promotions.is_empty() {
        return;
    }
    println!(
        "\n  {} field(s) now have two independent readings behind them:",
        promotions.len()
    );
    for p in promotions {
        println!(
            "  RAISE?    {} {} — {} observers",
            p.item, p.field, p.observers
        );
    }
    println!(
        "  Edit the grade to `verified` if you agree. Nothing was promoted: \
         the machine assembles the evidence and a person decides what it is worth."
    );
}

/// Compares each observation against what the dataset already says.
fn review(submission: &Submission, file: &Value) -> Result<Review, String> {
    let existing: BTreeMap<String, &Value> = file
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|i| Some((i.get("id")?.as_str()?.to_string(), i)))
                .collect()
        })
        .unwrap_or_default();

    let mut out = Review {
        fresh: Vec::new(),
        known: Vec::new(),
        conflicts: Vec::new(),
    };
    for observed in &submission.items {
        // Building the ItemDef first is the validation: unknown attributes,
        // unknown slots and over-precise decimals fail here rather than
        // landing in the dataset and failing at load.
        let def = observed
            .to_item(submission.method)
            .map_err(|e| e.to_string())?;
        let proposed = to_json(observed, submission.method, &def);
        match existing.get(&observed.id) {
            None => out.fresh.push((observed.id.clone(), proposed)),
            Some(current) => {
                let before = out.conflicts.len();
                compare(&observed.id, current, &proposed, &mut out.conflicts);
                if out.conflicts.len() == before {
                    out.known.push(observed.id.clone());
                }
            }
        }
    }
    Ok(out)
}

/// Renders an observation in the dataset's own item shape, so what is
/// reviewed is exactly what would be written.
fn to_json(observed: &ItemObservation, method: Method, def: &ItemDef) -> Value {
    let grade = method.offered_grade();
    let mut item = Map::new();
    item.insert("id".into(), Value::String(observed.id.clone()));
    item.insert("name".into(), Value::String(observed.name.clone()));
    // Rarity is written from the parsed value rather than the raw string:
    // whatever lands in the dataset is then something the loader understood.
    // It was missing here entirely — read off the card, validated on the way
    // in, and dropped on the way out, which is a checked value that does
    // nothing. Keyed after `name` so the order matches the hand-authored
    // items and a submission does not stand out for the wrong reason.
    if let Some(rarity) = def.rarity {
        item.insert("rarity".into(), Value::String(rarity.as_str().into()));
    }
    if let Some(slot) = def.slot {
        item.insert("slot".into(), Value::String(slot.as_str().into()));
    }
    if let Some(attributes) = &def.attributes {
        let mut points = Map::new();
        for (kind, value) in attributes.value() {
            points.insert(kind.as_str().into(), Value::from(*value));
        }
        item.insert(
            "attributes".into(),
            serde_json::json!({ "confidence": grade, "points": points }),
        );
    }
    let mut grants = Map::new();
    for (stat, value) in &def.grants {
        grants.insert(
            stat.as_str().into(),
            serde_json::json!({ "confidence": grade, "micro": value.value().micro() }),
        );
    }
    item.insert("grants".into(), Value::Object(grants));
    if let Some(add) = &def.move_speed_add {
        item.insert(
            "moveSpeedAdd".into(),
            serde_json::json!({ "confidence": grade, "micro": add.value().micro() }),
        );
    }
    Value::Object(item)
}

/// Walks two item objects and records every field they disagree on.
///
/// Compared as rendered JSON rather than through typed structs, for the same
/// reason `assay-diff` does: a field nobody remembered to compare is a field
/// that silently agrees with everything.
fn compare(item: &str, current: &Value, proposed: &Value, into: &mut Vec<Conflict>) {
    let flat = |v: &Value| -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        flatten("", v, &mut out);
        out
    };
    let (a, b) = (flat(current), flat(proposed));
    for key in b.keys() {
        // A grade is not a disagreement. A submission read by text
        // recognition offers `unverified` for a number the dataset already
        // has `verified`, and treating that as a conflict would make every
        // corroboration look like a fight. What the two say the value IS is
        // the only thing worth stopping for; how each of them came to see it
        // is the reviewer's input, not a contradiction.
        if key.ends_with(".confidence") {
            continue;
        }
        // Only fields the submission speaks to. A dataset field it says
        // nothing about is not a disagreement.
        let (Some(was), Some(now)) = (a.get(key), b.get(key)) else {
            continue;
        };
        if was != now {
            into.push(Conflict {
                item: item.to_string(),
                field: key.clone(),
                dataset: was.clone(),
                submitted: now.clone(),
            });
        }
    }
}

fn flatten(prefix: &str, value: &Value, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&path, inner, out);
            }
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

fn report(submission: &Submission, review: &Review) {
    println!(
        "{} · {} · {} · {:?}",
        submission.observer, submission.observed_at, submission.build, submission.method
    );
    if let Some(note) = &submission.note {
        println!("  “{note}”");
    }
    println!(
        "  offered as {} — a method, not a verdict",
        submission.method.offered_grade()
    );
    println!();

    for (id, _) in &review.fresh {
        println!("  new       {id}");
    }
    for id in &review.known {
        println!("  already   {id}");
    }
    for c in &review.conflicts {
        println!(
            "  DISAGREES {} {}: dataset {} · submitted {}",
            c.item, c.field, c.dataset, c.submitted
        );
    }
    if !submission.unrecognised.is_empty() {
        println!("\n  read but not modelled:");
        for line in &submission.unrecognised {
            println!("    {line}");
        }
        println!(
            "  These are kept on purpose. A line the schema has no home for \
             is evidence about the schema, and dropping it loses that."
        );
    }
}
