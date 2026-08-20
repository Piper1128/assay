#!/usr/bin/env bash
# The CI gate chain, locally. Same gates, same order, same meaning — so a
# green run here means a green run there, and development is not blocked
# when Actions cannot allocate a runner.
#
# Usage:  bash tools/check.sh [--fast]
#   --fast  skips the negative probes, which rebuild the workspace several
#           times. Run the full thing before pushing.
set -uo pipefail

fast=0
[ "${1:-}" = "--fast" ] && fast=1

failed=()
step() {
    local name="$1"; shift
    printf '\n\033[1m── %s\033[0m\n' "$name"
    if "$@"; then
        printf '   ok\n'
    else
        printf '   FAILED\n'
        failed+=("$name")
    fi
}

clippy_clean() {
    # `-D warnings` makes clippy exit non-zero on any finding, so its exit
    # code is the whole test.
    cargo clippy --workspace --all-targets -- -D warnings
}

determinism() {
    # ADR-001 rev 2 §5, the fresh-process half: the same input must give
    # byte-identical canonical output from two separate processes.
    local bad=0
    for loadout in loadouts/*.toml; do
        local a b
        a=$(cargo run -q -p assay-cli -- resolve "$loadout" --json) || return 1
        b=$(cargo run -q -p assay-cli -- resolve "$loadout" --json) || return 1
        if [ "$a" != "$b" ]; then
            printf '   non-deterministic across processes: %s\n' "$loadout"
            bad=1
        fi
    done
    [ "$bad" -eq 0 ] || return 1
    printf '   corpus checksum: '
    for loadout in loadouts/*.toml; do
        cargo run -q -p assay-cli -- resolve "$loadout" --json
    done | sha256sum | cut -d' ' -f1
}

step "format"          cargo fmt --all --check
step "clippy"          clippy_clean
step "build (host)"    cargo build --workspace
step "build (no_std)"  cargo build --target thumbv7em-none-eabi -p assay-core
step "tests"           cargo test --workspace
step "trust boundary"  bash tools/gates/dep_direction.sh
# The browser page embeds the resolver, so it is built from the same source
# rather than kept beside it. Skipped where the wasm toolchain is absent --
# announced, never silently.
if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown    && command -v wasm-bindgen >/dev/null 2>&1; then
    step "ui"     bash tools/build-ui.sh
else
    printf '
[1m-- ui[0m
   skipped: needs the wasm32 target and wasm-bindgen
'
fi

step "mirror"          python3 mirror/gen_slice_vector.py --check
step "determinism"     determinism

if [ "$fast" -eq 0 ]; then
    # Probes rewrite tracked files and restore them with git, so they refuse
    # to run on a dirty tree. That is the guard working, not a failure.
    step "negative probes" bash probes/run.sh
else
    printf '\n\033[1m── negative probes\033[0m\n   skipped (--fast)\n'
fi

printf '\n'
if [ ${#failed[@]} -eq 0 ]; then
    printf '\033[32mall gates green\033[0m\n'
else
    printf '\033[31mfailed: %s\033[0m\n' "${failed[*]}"
    exit 1
fi
