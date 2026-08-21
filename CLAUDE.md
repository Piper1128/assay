# Assay

Headless stat-resolver og patch-differ til Dark and Darker. **Offentligt/MIT-repo.**

**⚠ Kode OG commits er på ENGELSK her** — modsat resten af huset. Publikum er
DnD-spillere, ikke IronCore-udviklere.

Rust edition 2024, `rust-version = 1.97`, workspace i `crates/*`; `fuzz/` er uden for.
Crates: `assay-core` (`no_std + alloc`, ingen serde), `assay-data` (fil-I/O + serde),
`assay-diff`, `assay-scrape`, `assay-cli`, `assay-wasm`.

## Gates
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo build --target thumbv7em-none-eabi -p assay-core   # no_std-porten
cargo test --workspace
bash tools/gates/dep_direction.sh                        # afhængighedsretning
python3 mirror/gen_slice_vector.py --check               # Python-spejlet
bash probes/run.sh                                       # alle 8 prober
```
CI har desuden et **determinism**-job (samme loadout resolvet to gange skal give
byte-identisk JSON) og `fuzz-smoke` på `decode_dataset`. `submission.yml` tager imod
community-indleveringer via issues.

**⚠ Der er INGEN pre-commit-hook i dette repo** (verificeret: ingen `hooks/`, ingen
`core.hooksPath`). Modsat [nano-chain](../nano-chain), hvor den findes. Kør derfor
`cargo fmt --all` + `clippy` **manuelt før hver commit** — ellers falder det først i CI.

## Determinisme
`clippy.toml` forbyder de samme typer som nano-chain, med to dokumenterede
undtagelser (ADR-001 rev 2): `f64` i `assay-cli`s præsentationslag, og
`assay-scrape` må læse wall-clock. Undtagelser står i filen — udvid dem ikke uden ADR.

## Arkitektur-beslutning der er låst
`IronCore.Warehouse` og Assay holdes **ADSKILT** — licens (MIT vs kommerciel),
domæner overlapper ikke, publikum er forskelligt. **Genbrug MØNSTERET**
(katalog-som-data, CHECK→FK), aldrig instansen. Det er netop dét der tester
framework-påstanden.

Intet warehouse i Assay endnu, og det er **bevidst**: vi ved ikke om det bliver 20
eller 20.000 indleveringer. Indleveringsformatet producerer den viden først.

ADR'er: `docs/adr/`. ⚠ Der er en flagget tvetydighed i ADR-006 — se
`project_assay.md` i memory-banken før du rører skoler/tags.
