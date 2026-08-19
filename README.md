# Assay

Headless stat resolver and patch differ for Dark and Darker.

An assay is the analytical determination of a metal's actual composition and
purity. This tool does the same for loadouts: it computes what a build is
actually worth, as opposed to what the tooltip claims — and shows exactly what
this week's patch did to it.

**Status:** scaffold. Baseline dataset: Patch 6.12 / Hotfix #123, build
`0.17.150.9384`. v1 scope is stat resolution + patch diff; TTK solver and
counter matrix are deferred to v2 (ADR-011).

## Using it

```bash
assay versions                                  # committed dataset builds
assay resolve loadouts/naked-rogue.toml         # a stat block, with confidence
assay resolve loadouts/naked-rogue.toml --explain   # every pipeline stage
assay resolve loadouts/naked-rogue.toml --json      # the canonical form
assay resolve loadouts/naked-rogue.toml --strict    # exit 2 if anything is unverified
assay diff <build-a> <build-b> --loadouts loadouts  # what a patch did, and to whom
```

Every number carries its confidence, unasked:

```
naked-rogue   hotfix-123 (0.17.150.9384)

  ~  attributes            str 9  vig 6  agi 25  dex 20  wil 10  kno 10  res 25
  ~  action_speed                7.8125
  ok armor_rating                     0
  ~  health                        108.5
  ~  move_speed                      306
  ~  pdr                             -22
  ~  physical_power_bonus            -14

  5 of 6 values are not verified against the game.
```

Those four derived numbers are what the game's own character sheet shows for
a naked Rogue at Hotfix 123 — and they are still marked unverified, because
they come from the wiki rather than from an in-game test.

`diff` works but has nothing to chew on yet: only one dataset version is
committed. Adding the Hotfix 122 numbers is a data task, and the point at
which the tool starts answering the question it exists for.

## Layout

| Path | Std? | Role |
|---|---|---|
| `crates/assay-core` | `no_std + alloc` | Domain types, schema types, newtypes, resolution pipeline, exchange model, canonical form |
| `crates/assay-data` | std | Filesystem, parsing, validation, version lookup |
| `crates/assay-diff` | std | Structural diff (level 1) + impact diff (level 2), ADR-008 |
| `crates/assay-scrape` | std | Proposal tool; outside the trust boundary (ADR-003) |
| `crates/assay-cli` | std | The `assay` binary; the only crate that may print |
| `mirror/` | Python | Independent reference implementation (ADR-010 rev 2) — written from the ADRs, never from the Rust code |
| `fixtures/` | data | Vector corpus + golden values |
| `probes/` | bash | Negative probes: every gate proven able to fail |

## Gates

| Gate | Proves | Where |
|---|---|---|
| bare-metal build of `assay-core` | core purity is compiler-enforced, not a convention | CI `Build (no_std target)` |
| clippy `disallowed_types` | no floats, no randomised-hash collections, no ambient time | `clippy.toml` + workspace lints |
| trust boundary | `assay-data` never depends on `assay-scrape` | `tools/gates/dep_direction.sh` |
| negative probes | each gate above can still fail | `probes/run.sh` |

## Decision record

The authoritative ADRs live in [docs/adr/](docs/adr/): `ADR-000-010.md` as
amended by `ADR-rev2-amendments.md` (rev 2 replaces ADR-000, 001 and 010; the
index document is kept for decision history) and by
`ADR-006-amendment-pdr-mod-layer.md` (step 7: the PDR Mod applies to the PDR,
not to the damage). `ADR-012-derived-stat-ratings.md` (derived stats are weighted
ratings over several attributes, not single-attribute curves) and
`ADR-006-amendment-penetration-resampling.md` (the exchange re-samples the
PDR curve, so it takes the dataset as a fourth input). Imported source documents are preserved verbatim (in Danish);
amendments authored here are in English. [docs/rogue-fighter-duo-hotfix123.md](docs/rogue-fighter-duo-hotfix123.md)
is the source of v1's golden fixtures.

## Conventions

- Code, comments and commit messages in English.
- Core purity is a compile-time property (`no_std + alloc`), not a lint.
- Determinism is mechanical: `BTreeMap`/`BTreeSet` everywhere order can reach
  output; fixed-point `i64` micro-units (1e-6), never floats.
- A gate that has never been seen failing is not a gate (`probes/run.sh`).
- No numeric code without at least one golden fixture and mirror coverage.

## Data licensing

Game data derives from the Dark and Darker Wiki (spellsandguns team),
CC BY-SA 4.0, with per-file provenance in each dataset manifest (ADR-003 §6).
Patch facts from official Ironmace release notes. Dark and Darker is a
trademark of Ironmace Games; this is an unaffiliated fan tool. Code is MIT
licensed; distributed datasets carry the wiki's CC BY-SA 4.0.
