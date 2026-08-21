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

### Asking about one attack

```
assay exchange attacker.toml defender.toml --situation situations/sneak-attack-from-hide.toml --explain
```

```toml
skill = "skill.rogue.sneak_attack"

[context]
power_bonus_adjust = "-30"
```

A weapon is a chain, not a single blow — an Arming Sword runs
`Slash/Slash/Pierce` at `100%/105%/110%` — so `combo_hit = 3` asks about
the third swing. Naming a skill overrides it: a skill replaces the normal
swing rather than joining the chain.

A situation names the skill and states the circumstances. The skill's own
numbers live in the dataset, so a patch that changes Sneak Attack's scaling
shows up in `assay diff` — while they lived in a file beside the tool, such a
change was invisible. Anything written in the file still wins over the skill,
because the file is the question being asked and a question is always more
specific than what is generally true.

The circumstances stay out of the skill. Sneak Attack can be used from Hide
or not, so leaving Hide belongs to the attack rather than to the ability. It is a third
file rather than a section of a loadout because it is a fact about the
*attack* — two identical builds differ only in whether one of them is behind
the other, and putting that in a loadout would make it a property of the
build.

Omitting a field means "as the weapon swings", not zero. Those are different
statements and only one of them is usually meant.

```
2. scaling coefficient      × 0% → 0
3. physical power bonus     × (100-36)% → 0
4. flat weapon damage       +15 → 15
8. true damage              +1 (bypasses armor) → 15.091
```

That is Sneak Attack leaving Hide, and it is the case the whole source
analysis is built on: 0% scaling means the −30% Hide-exit penalty multiplies
nothing, so the flat damage and the true damage are immune to it. The
immunity is not an assertion in a document — it is step 3 reading zero.

### Taking in what someone else observed

```
assay submit their-submission.json           # what would it change?
assay submit their-submission.json --apply   # write it in
```

A submission is one person's observations, offered to the dataset — not
added to it. Three things travel with every value, and they are why the
format exists rather than being a blob of numbers: **who** saw it and when,
because two people agreeing independently is stronger than one person
insisting; **how** they saw it, because a number read by text recognition
and the same number typed off the same screenshot carry different
transcription risk; and **what they read that had nowhere to go**, because
fifty submissions quietly dropping "Demon Damage Reduction" is fifty pieces
of evidence about the schema, discarded one at a time.

The method decides what grade a value is *offered* at. Nothing promotes
itself, and review may still lower it.

A submission that disagrees with the dataset is refused whole rather than
applied in part, and exits 2 so a script can tell disagreement from failure.
Two people reading one card differently means something is wrong — a rarity
nobody recorded, a patch nobody noticed, a misread digit — and taking the
newer number throws that away at the moment it appeared. A different *grade*
is not a disagreement: a text-recognised reading of a value the dataset
already verified is corroboration.

The browser page writes submissions, so a contributor never has to learn the
format. Same writer, same reader, both in Rust.

### In a browser

**<https://piper1128.github.io/assay/>** — built and published from `main`
every time the gates pass, so it is never behind the dataset it embeds.

That URL is the answer to a problem the local build created. `ui/assay.html`
is not committed on purpose: it carries both the resolver and the dataset, so
a checked-in copy drifts behind the thing it claims to compute. But that left
anyone contributing observations working against whatever 1 MiB file they
were last sent. Now everyone refreshes the same page.

To build it yourself — offline, from a `file://` URL, with no server:

```
python tools/build-ui.py --open
```

Builds `ui/assay.html` and opens it: one file, no server, no install, no
network. Text recognition is the single exception and says so on the page —
it fetches a pinned, SRI-checked release of Tesseract the first time and then
works offline. The hash covers the entry script; the worker, the wasm core
and the training data are fetched by Tesseract itself and are pinned to exact
versions, which a hash cannot cover because they are not script tags.
Vendoring the lot the way the resolver is vendored does not fit: the training
data alone is larger than this whole page. It runs the same resolver the CLI does — `assay-core` is
`no_std + alloc` with no floats and no hash maps, which compiles to
`wasm32-unknown-unknown` unchanged, so the page does not carry a second
implementation of the pipeline. Numbers cross into JavaScript as decimal
strings; a `f64` at that boundary would reintroduce the error class this
project exists to prevent.

Screenshot an item tooltip and paste it in, and it reads the card — showing
what it understood and what it did not, because a card half-read in silence
is worse than one that says which half. Text recognition fetches its engine
once; nothing else on the page ever touches the network.

Nothing here reads the game's memory or injects an overlay. The page only
sees what you hand it.

The page is built rather than committed: an 800 KiB result in git would drift
behind the resolver it embeds. The build needs the `wasm32-unknown-unknown`
target and a `wasm-bindgen` CLI matching the pinned crate version.

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

Run the whole chain locally with `bash tools/check.sh` — same gates, same
order, so a green run here is a green run in CI.

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
