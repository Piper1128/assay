# Fuzzing

`decode_dataset` feeds arbitrary text to `assay_data::decode` and asserts the
only property that matters: **it never panics**. Malformed input must come
back as a typed `LoadError` (ADR-001 rev 2 §4).

Outside the workspace deliberately — cargo-fuzz needs nightly and its own
target directory, and neither belongs in an ordinary `cargo build`.

```bash
cargo +nightly fuzz run decode_dataset                    # until you stop it
cargo +nightly fuzz run decode_dataset -- -max_total_time=60
```

`seeds/` holds curated regression inputs and **is committed**; every run
replays them. `corpus/` is the ephemeral working corpus and is not. A crash
found here should land in `seeds/` with the fix, so it can never come back
quietly.

## Running it on Windows

The target builds on `x86_64-pc-windows-msvc` but the produced binary exits
with `STATUS_DLL_NOT_FOUND`: libFuzzer links the clang ASAN runtime
dynamically and the DLL is not on `PATH` in a stock MSVC install. Run it under
WSL or Linux, or put `clang_rt.asan_dynamic-x86_64.dll` from your LLVM
installation on `PATH`. CI runs it on Ubuntu, which is where the gate
actually bites.

The property tests in `assay-data` cover the same invariant on every
platform without nightly — shallower, but always on.
