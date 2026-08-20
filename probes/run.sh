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
    crates/assay-core/src/resolve.rs crates/assay-core/src/exchange.rs \
    crates/assay-core/src/stats.rs crates/assay-diff/src/lib.rs crates/assay-data/Cargo.toml

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

# ── pipeline_order ───────────────────────────────────────────────────────────
# Feeding the curves the attribute sum from BEFORE stage 3 (party/perk
# attributes applied after the curve lookup, ADR-005 stages 3↔4) must fail
# the pipeline tests — the exact ordering the Rogue/Fighter synergy rests on.
sed -i '/probe: pipeline-order/ s/attributes_final/attributes_after_gear/' crates/assay-core/src/resolve.rs
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/resolve.rs
verdict pipeline_order "$gate_rejected"

# ── ability_dedupe ───────────────────────────────────────────────────────────
# An ability applies once however many people bring it. Letting a duplicate
# through must fail: two Jokesters would be +4 All Attributes instead of +2.
sed -i '/probe: ability-dedupe/ s/seen\.insert(id\.as_str()\.to_string())/true/' crates/assay-core/src/resolve.rs
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/resolve.rs
verdict ability_dedupe "$gate_rejected"

# ── item_armor_bonus_base ────────────────────────────────────────────────────
# Widening the Item Armor Rating Bonus base past the item bucket (ADR-005
# amendment) must fail: enchantments are outside the multiplier, and a test
# that cannot tell that apart is not testing the amendment.
sed -i '/probe: item-armor-bonus-base/ s/item_ar\.clone()/item_ar.clone().zip_with(other_ar.clone(), |a, b| a + b)/' crates/assay-core/src/resolve.rs
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/resolve.rs
verdict item_armor_bonus_base "$gate_rejected"

# ── scaling_ignored ──────────────────────────────────────────────────────────
# Hardcoding the skill scaling coefficient to 100% (ADR-006 step 2) must fail
# the Sneak Attack fixture: 0% scaling is what makes it immune to the
# Hide-exit power penalty.
sed -i '/probe: scaling-coefficient/ s/self\.strike\.scaling\.clone()/crate::confidence::Confidence::Verified(crate::stats::ScalingCoefficient::new(crate::fixed::Fixed::from_int(100)))/' crates/assay-core/src/exchange.rs
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/exchange.rs
verdict scaling_ignored "$gate_rejected"

# ── true_damage_pre_reduction ────────────────────────────────────────────────
# True Damage added BEFORE the reduction chain (ADR-006 step 8 moved ahead of
# 5-7) must fail: armor would eat damage that bypasses armor by definition.
sed -i '/probe: true-damage-post-reduction/ s/reduced\.clone()/with_flat.clone()/' crates/assay-core/src/exchange.rs
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/exchange.rs
verdict true_damage_pre_reduction "$gate_rejected"

# ── pdr_mod_additive ─────────────────────────────────────────────────────────
# Adding PdrMod to PDR instead of multiplying (ADR-006 step 7 / ADR-002's
# locked apply_pdr_mod signature) must fail: Lethal Mark would read 30%
# effective PDR instead of 42%.
sed -i '/probe: pdr-mod-multiplicative/ s/\.mul_div_half_even(PERCENT + m\.value(), PERCENT)/+ m.value()/' crates/assay-core/src/stats.rs
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/stats.rs
verdict pdr_mod_additive "$gate_rejected"

# ── final tree check ─────────────────────────────────────────────────────────
if ! git diff --quiet -- crates/; then
    echo "probes: tree left dirty after restore — fix run.sh"
    status=1
fi

exit $status
