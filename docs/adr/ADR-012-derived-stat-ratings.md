# ADR-012 — Derived stats are ratings, not single-attribute curves

**Status:** Proposed — awaiting lock
**Date:** 19 August 2026
**Supersedes:** the `DerivedCurves` shape in ADR-004
**Refines:** ADR-005 stage 4 (the locked pipeline; ADR-005 requires a new ADR to touch it — this is that ADR)
**Unchanged:** ADR-005's stage *order*, in particular stage 3 before stage 4

> Authored in this repo, so English; the imported ADRs it amends stay verbatim in Danish.

## Context

ADR-004 modelled each derived stat as one curve over one attribute:

```rust
pub struct DerivedCurves {
    pub strength_to_physical_power: CurveId,
    pub agility_to_action_speed: CurveId,
    pub agility_to_move_speed: CurveId,
    pub vigor_to_health: CurveId,
    pub armor_to_pdr: CurveId,
}
```

Contact with the real Patch 6.12 / Hotfix 123 data shows that is not how the
game works. Two of those five stats read **two** attributes through a
weighted sum before any curve is consulted:

| Derived stat | Actual input |
|---|---|
| Physical Power Bonus | Physical Power = 1.00 × STR |
| Action Speed | Action Speed Rating = 0.25 × AGI + **0.75 × DEX** |
| Base Health | Base Health Rating = **0.25 × STR** + 0.75 × VIG, then **+25 for every class** |
| Move Speed | 1.00 × AGI, on a 300 baseline, hard cap 330 |
| Physical Damage Reduction | Armor Rating (gear-sourced, not an attribute) |

### Evidence

The wiki's published curves reproduce the wiki's published Rogue statblock
exactly — four for four, to the digit — only under the rating model:

| Stat | Computation | Result | Wiki |
|---|---|---|---|
| Physical Power Bonus | PP = 9; −20% + 3%/pt × 2 | **−14%** | −14% |
| Action Speed | ASR = 0.25×25 + 0.75×20 = 21.25; 1.25%/pt × 6.25 | **7.8125%** | 7.8125% |
| Base Health | BHR = 0.25×9 + 0.75×6 = 6.75; 70 + 2×6.75, +25 | **108.5** | 108.5 |
| Move Speed | 300 + 0.6/pt × 10 | **306** | 306 |

Under the single-attribute model, Action Speed cannot be derived from Agility
alone: 7.8125% is a **binary fraction because of the weights**, not by
coincidence. 6.25 rating points × 1.25% per point = 7.8125. This is the value
ADR-001 rev 2 chose micro-units for — the mechanism behind it is now known.

Sources: the Dark and Darker Wiki `Stats`, `Rogue` and `Armor_Rating` pages
(CC BY-SA 4.0), which state they are current for Patch 6.12 Hotfix 123.

## Decision

**A derived stat is a weighted sum of inputs, run through a curve, offset and
clamped.** One shape covers every case above, including the 1:1 ones (a
single weight of 1.00) and the gear-sourced ones.

```rust
/// What feeds a rating. Attributes come from ADR-005 stage 3; derived inputs
/// let one derived stat feed another (Armor Rating → PDR, Will → Magic
/// Resistance → Magical Damage Reduction).
pub enum RatingInput {
    Attribute(AttributeKind),
    Derived(DerivedStatId),
}

pub struct DerivedStatDef {
    pub id: DerivedStatId,                        // derived.action_speed
    pub weights: BTreeMap<RatingInput, Fixed>,    // 0.25 AGI + 0.75 DEX
    pub curve: CurveId,
    pub offset: Fixed,                            // +25 health · 300 move-speed baseline
    pub floor: Option<Fixed>,                     // PPB lower limit −100%
    pub cap: Option<Fixed>,                       // move speed 330 · PDR cap
}
```

`BTreeMap` per ADR-001 rev 2: weights iterate in sorted order, so the rating
is byte-reproducible.

### Evaluation

```
rating       = Σ (weight × input)          each product rounds half-to-even (ADR-001)
derived      = curve.sample(rating)
derived     += offset
derived      = clamp(derived, floor, cap)
```

**Offsets are separate from curves on purpose.** Move Speed's 300 baseline
and Health's flat +25 could have been baked into the curve points, but
keeping them apart makes a patch diff say *"baseline changed"* or *"curve
shape changed"* instead of blurring the two. That distinction is the whole
point of ADR-008 level 1.

### Rounding

Each weighted term is one `Fixed` multiplication and rounds half to even at
that single point (ADR-001 rev 2); the terms then sum exactly. For every
weight the game currently ships — quarters — the products are exact and no
rounding occurs at all. Summing all products in `i128` and rounding once was
considered; it was rejected because it needs a second multiplication
primitive to express, and it is indistinguishable on the shipped data.

### Pipeline (ADR-005 stage 4, refined)

Stage 4 splits in two, and **the stage order is otherwise untouched**:

```
4a. attributes → ratings          weighted sums
4b. ratings    → derived stats    curve → offset → clamp
```

Derived stats may depend on other derived stats, so stage 4b evaluates in
dependency order. **Stage 3 still comes before stage 4**, and this ADR makes
that lock stricter rather than looser: party buffs now move *two* attributes
into a single rating, so applying them late corrupts more, not less. The
`pipeline_order` probe keeps its meaning unchanged.

### Schema validation (ADR-004)

- Every `RatingInput::Derived` must name a stat defined in the same dataset
- The dependency graph must be acyclic — a cycle is a dataset error, never a
  runtime loop
- A stat with no weights is an error; an empty rating is not a valid stat

## What this ADR does not settle

**The PDR cap.** The wiki states a flat 65% cap and describes Defense Mastery
as granting *Item Armor Rating Bonus* — more rating before the curve, not a
higher ceiling. `docs/rogue-fighter-duo-hotfix123.md` states a 60% cap raised
to 75% by Defense Mastery. **The two sources disagree**, and one of them is
our own fixture document. This ADR fixes the *structure* (a cap belongs to
the derived-stat definition and perks may raise it, per ADR-005 stage 7); the
*value* stays `Unverified` (ADR-007) until tested in game. Do not let the
structural change quietly pick a winner.

## Consequences

- ✅ The model matches the game: all four Rogue derived stats reproduce exactly
- ✅ Adding a derived stat (Magic Resistance, Memory Capacity, Item Equip
  Speed) becomes dataset work, not code work — the same argument ADR-004 makes
  for curves, now extended to what feeds them
- ✅ `armor_to_pdr` stops being a special case: gear-sourced Armor Rating is
  just another rating input
- ✅ A weight change from Ironmace shows up as a data diff, not a code change
- ❌ Five named fields become a graph that must be validated (acyclicity,
  resolvable references). Accepted: the validation is cheap and catches
  dataset errors that the old shape made unrepresentable-but-also-unstatable
- ❌ The slice fixtures' placeholder curves must be rebuilt against the real
  ones. Their Action Speed already reads 7.177734% where the game reads
  7.8125% — the vector is honestly graded `unverified`, but it is wrong, and
  this ADR is what makes it fixable

## Alternatives considered

**Keep single-attribute curves, fold the second attribute into the curve.**
Impossible: a curve over Agility cannot express a dependence on Dexterity.
Only reachable by baking a specific Dexterity value into the curve, which
would silently produce wrong numbers for every other loadout — precisely the
failure class this project exists to prevent.

**Hardcode the two hybrid ratings as named fields** (`action_speed_rating`,
`base_health_rating`). Smaller change, but it stops at exactly the two stats
we happen to have looked at, and the wiki documents at least four more
rating-shaped conversions we have not modelled yet.
