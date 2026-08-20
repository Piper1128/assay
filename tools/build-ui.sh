#!/usr/bin/env bash
# Builds ui/assay.html from the wasm crate and ui/template.html.
#
# The `wasm-bindgen` CLI and the crate must be the same version; the crate is
# pinned exactly so the two cannot drift apart silently.
set -eu
cd "$(dirname "$0")/.."
cargo build --quiet -p assay-wasm --release --target wasm32-unknown-unknown
wasm-bindgen --target no-modules --no-typescript --out-dir ui/pkg \
    target/wasm32-unknown-unknown/release/assay_wasm.wasm 2>/dev/null
python tools/build-ui.py
