#!/usr/bin/env bash
# One-way trust boundary (ADR-003 §5): assay-data must never depend on
# assay-scrape. The no_std gate cannot catch this (both crates are std), so it
# stays an explicit textual gate. Probed by probes/run.sh (dep_direction).
set -u

if grep -q 'assay-scrape' crates/assay-data/Cargo.toml; then
    echo "GATE dep_direction: assay-data references assay-scrape — forbidden (ADR-003 §5)"
    exit 1
fi
echo "GATE dep_direction: ok"
