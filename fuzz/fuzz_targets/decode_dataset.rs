//! The dataset decoder must never panic on input (ADR-001 rev 2 §4).
//!
//! Datasets are hand-edited and, once `assay-scrape` exists, will arrive
//! through a proposal process. Malformed input has to produce a typed
//! `LoadError` — a crash would take the tool down on data it was built to be
//! suspicious of.
#![no_main]

use arbitrary::Arbitrary;
use assay_data::DatasetText;
use libfuzzer_sys::fuzz_target;

/// Six independent strings rather than one blob: the decoder reads six
/// files, and a single blob would almost never get past the first parse.
#[derive(Debug, Arbitrary)]
struct Input<'a> {
    build: &'a str,
    manifest: &'a str,
    classes: &'a str,
    curves: &'a str,
    items: &'a str,
    perks: &'a str,
    skills: &'a str,
}

fuzz_target!(|input: Input| {
    let text = DatasetText {
        manifest: input.manifest.to_string(),
        classes: input.classes.to_string(),
        curves: input.curves.to_string(),
        items: input.items.to_string(),
        perks: input.perks.to_string(),
        skills: input.skills.to_string(),
    };
    // The result is irrelevant; not panicking is the property.
    let _ = assay_data::decode(&text, input.build);
});
