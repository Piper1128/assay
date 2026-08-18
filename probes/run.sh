#!/usr/bin/env bash
# Negative probes (ADR-010 rev 2 §4): a gate that has never been seen failing
# is not a gate. Each probe injects a deliberate violation, asserts that the
# gate REJECTS it, and restores the tree. Run from the repo root.
#
# The script mutates tracked files and restores them with `git checkout`, so it
# refuses to start if the paths it touches are dirty.
set -u

status=0

require_clean() {
    if ! git diff --quiet -- "$@"; then
        echo "probes: working tree is dirty under: $*"
        echo "probes: commit or stash first — probes restore via git checkout"
        exit 2
    fi
}

verdict() {
    # $1 = probe name, $2 = 0 if the gate rejected the violation
    if [ "$2" -eq 0 ]; then
        echo "── probe $1: gate rejected the violation — OK"
    else
        echo "── probe $1: gate ACCEPTED the violation — PROBE FAILED"
        status=1
    fi
}

require_clean crates/assay-core/src/lib.rs crates/assay-core/src/confidence.rs \
    crates/assay-diff/src/lib.rs crates/assay-data/Cargo.toml

# ── no_std_violation ─────────────────────────────────────────────────────────
# `use std::fs` in assay-core must fail the bare-metal build (E0433).
cp probes/no_std_violation.rs crates/assay-core/src/no_std_violation.rs
printf '\nmod no_std_violation;\n' >> crates/assay-core/src/lib.rs
if cargo build --quiet --target thumbv7em-none-eabi -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/lib.rs
rm -f crates/assay-core/src/no_std_violation.rs
verdict no_std_violation "$gate_rejected"

# ── hashmap_violation ────────────────────────────────────────────────────────
# HashMap in assay-diff must fail clippy's disallowed_types (clippy.toml).
cp probes/hashmap_violation.rs crates/assay-diff/src/hashmap_violation.rs
printf '\nmod hashmap_violation;\n' >> crates/assay-diff/src/lib.rs
if cargo clippy --quiet -p assay-diff -- -D warnings >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-diff/src/lib.rs
rm -f crates/assay-diff/src/hashmap_violation.rs
verdict hashmap_violation "$gate_rejected"

# ── dep_direction ────────────────────────────────────────────────────────────
# assay-data referencing assay-scrape must fail the trust-boundary gate.
printf '\n# probe injection\n# assay-scrape.workspace = true\n' >> crates/assay-data/Cargo.toml
if bash tools/gates/dep_direction.sh >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-data/Cargo.toml
verdict dep_direction "$gate_rejected"

# ── confidence_not_propagated ────────────────────────────────────────────────
# Combining confidence with max instead of min (ADR-007) must fail the
# propagation tests. Mutates the anchored line; a vanished anchor makes the
# sed a no-op, the tests pass, and the probe fails loudly — the right way.
sed -i '/probe: confidence-propagation/ s/\.min(/.max(/' crates/assay-core/src/confidence.rs
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/confidence.rs
verdict confidence_not_propagated "$gate_rejected"

# ── final tree check ─────────────────────────────────────────────────────────
if ! git diff --quiet -- crates/; then
    echo "probes: tree left dirty after restore — fix run.sh"
    status=1
fi

exit $status
