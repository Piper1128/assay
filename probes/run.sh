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

# Set to 0 by `mutate` when its edit changed nothing. A probe whose anchor
# has drifted injects no violation, so the gate passes and the run reads as
# a code bug when it is a probe bug — the most expensive kind of green.
mutated=1

mutate() {
    # $1 = file, rest = sed expression
    local file="$1"
    shift
    local before
    before=$(git hash-object "$file")
    sed -i "$@" "$file"
    if [ "$(git hash-object "$file")" = "$before" ]; then
        mutated=0
    else
        mutated=1
    fi
}

verdict() {
    # $1 = probe name, $2 = 0 if the gate rejected the violation
    if [ "$mutated" -eq 0 ]; then
        echo "── probe $1: injected nothing — its anchor moved, so it is not probing"
        status=1
    elif [ "$2" -eq 0 ]; then
        echo "── probe $1: gate rejected the violation — OK"
    else
        echo "── probe $1: gate ACCEPTED the violation — PROBE FAILED"
        status=1
    fi
    mutated=1
}

require_clean crates/assay-core/src/lib.rs crates/assay-core/src/confidence.rs \n    crates/assay-core/src/derived.rs \
    crates/assay-core/src/resolve.rs crates/assay-core/src/exchange.rs \
    crates/assay-core/src/stats.rs crates/assay-diff/src/lib.rs \n    crates/assay-data/Cargo.toml crates/assay-cli/src/situation.rs

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
mutate crates/assay-core/src/confidence.rs '/probe: confidence-propagation/ s/\.min(/.max(/'
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
mutate crates/assay-core/src/resolve.rs '/probe: pipeline-order/ s/attributes_final/attributes_after_gear/'
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/resolve.rs
verdict pipeline_order "$gate_rejected"

# ── damage_type ──────────────────────────────────────────────────────────────
# A strike's type chooses which stats the nine steps read. Forcing it back to
# physical must fail: a magic attack would be reduced by armour rating, which
# is the right shape reading the wrong stat (ADR-006 amendment: damage type).
mutate crates/assay-core/src/exchange.rs '/probe: damage-type/ s/strike\.damage_type/DamageType::Physical/'
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/exchange.rs
verdict damage_type "$gate_rejected"

# ── ability_bonus ────────────────────────────────────────────────────────────
# An ability's flat contribution reaches the `From Bonuses` row the game
# prints under every stat that has one. Dropping it must fail.
mutate crates/assay-core/src/resolve.rs '/probe: ability-bonus/ s/map(|_| value)/map(|_| crate::fixed::Fixed::ZERO)/'
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/resolve.rs
verdict ability_bonus "$gate_rejected"

# ── seed_adds ────────────────────────────────────────────────────────────────
# A gear-granted stat ADDS to what its definition computes. Dropping the seed
# must fail: Magic Resistance would read 15 from Will alone, ignoring the +9
# the trousers roll (ADR-005 amendment: gear attributes).
mutate crates/assay-core/src/derived.rs '/probe: seed-adds/ s/gear\.cloned()/None/'
if cargo test --quiet -p assay-core -p assay-data >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/derived.rs
verdict seed_adds "$gate_rejected"

# ── ability_dedupe ───────────────────────────────────────────────────────────
# An ability applies once however many people bring it. Letting a duplicate
# through must fail: two Jokesters would be +4 All Attributes instead of +2.
mutate crates/assay-core/src/resolve.rs '/probe: ability-dedupe/ s/seen\.insert(id\.as_str()\.to_string())/true/'
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
mutate crates/assay-core/src/resolve.rs '/probe: item-armor-bonus-base/ s/item_ar\.clone()/item_ar.clone().zip_with(other_ar.clone(), |a, b| a + b)/'
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/resolve.rs
verdict item_armor_bonus_base "$gate_rejected"

# ── debuff_item_base ─────────────────────────────────────────────
# An attacker-applied Item Armor Rating Bonus multiplies the defender's worn
# armour, not their enchantments. Widening its base must fail, the same way
# widening resolution's does (ADR-006 amendment: item armor debuff).
mutate crates/assay-core/src/exchange.rs '/probe: debuff-item-base/ s/composition\.item\.clone()/composition.item.clone().zip_with(composition.other.clone(), |a, b| a + b)/'
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/exchange.rs
verdict debuff_item_base "$gate_rejected"

# ── scaling_ignored ──────────────────────────────────────────────────────────
# Hardcoding the skill scaling coefficient to 100% (ADR-006 step 2) must fail
# the Sneak Attack fixture: 0% scaling is what makes it immune to the
# Hide-exit power penalty.
mutate crates/assay-core/src/exchange.rs '/probe: scaling-coefficient/ s/strike\.scaling\.clone()/crate::confidence::Confidence::Verified(crate::stats::ScalingCoefficient::new(crate::fixed::Fixed::from_int(100)))/'
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
mutate crates/assay-core/src/exchange.rs '/probe: true-damage-post-reduction/ s/reduced\.clone()/with_flat.clone()/'
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
mutate crates/assay-core/src/stats.rs '/probe: pdr-mod-multiplicative/ s/\.mul_div_half_even(PERCENT + m\.value(), PERCENT)/+ m.value()/'
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/stats.rs
verdict pdr_mod_additive "$gate_rejected"

# ── combo_counts_from_zero ───────────────────────────────────────────────────
# Reading the chain from zero must fail. A weapon's swings are named 1, 2, 3
# on the page they were transcribed from, and off-by-one here does not crash:
# it quietly answers about the wrong swing, at a scaling 5 points out.
mutate crates/assay-cli/src/situation.rs '/probe: combo-counts-from-one/ s/n - 1;/n;/'
if cargo test --quiet -p assay-cli >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-cli/src/situation.rs
verdict combo_counts_from_zero "$gate_rejected"

# ── gate_ignores_kind ────────────────────────────────────────────────────────
# A gate that fires on every swing (ADR-006 damage-kind amendment) must fail:
# Blunt Weapon Mastery would pay out while its holder swings a sword, which is
# the exact wrong answer the gate exists to avoid — and an easy one to write,
# because it looks like the condition is still there.
mutate crates/assay-core/src/exchange.rs '/probe: gate-checks-kind/ s/bonus.kind == kind;/true;/'
if cargo test --quiet -p assay-core >/dev/null 2>&1; then
    gate_rejected=1
else
    gate_rejected=0
fi
git checkout --quiet -- crates/assay-core/src/exchange.rs
verdict gate_ignores_kind "$gate_rejected"

# ── final tree check ─────────────────────────────────────────────────────────
if ! git diff --quiet -- crates/; then
    echo "probes: tree left dirty after restore — fix run.sh"
    status=1
fi

exit $status
