//! The `assay` binary — the only crate that may write to stdout (ADR-000 rev 2).
//!
//! Subcommands land with their subjects: `resolve --explain` with the
//! resolution pipeline (ADR-005), `diff` with the diff engine (ADR-008).

fn main() {
    println!(
        "assay {}: scaffold — no subcommands yet",
        env!("CARGO_PKG_VERSION")
    );
}
