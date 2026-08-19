# ADR-006 amendment — where the PDR Mod applies

**Status:** Accepted — amends ADR-006 (`ADR-000-010.md`)
**Date:** 19 August 2026
**Scope:** step 7 of the locked damage chain. Steps 1–6, 8 and 9 are unchanged.

> Written in English like everything else authored in this repo; the ADRs it
> amends are preserved verbatim in Danish as imported source documents.

## Context

ADR-006 lists the damage chain as nine operations, and steps 6–7 read:

```
6. → PDR fra kurven, cappet
7. × PDR Mod (multiplikativt)            Lethal Mark −30%
```

Read literally, step 7 multiplies *the damage* by the mod, since every other
step in the list operates on the running damage value. ADR-002 states
something different in a locked signature:

```rust
pub fn apply_pdr_mod(base: PdrPercent, m: PdrMod) -> EffectivePdr;
```

There the mod modifies *the PDR*, producing an effective PDR that reduces the
damage once. The two documents describe different arithmetic, and the
difference is large: against a 60% PDR defender with Lethal Mark (−30%),
100 base damage becomes 58 under ADR-002's reading and 28 under the literal
reading of ADR-006.

## Decision

**The PDR Mod is a multiplicative layer on the PDR, not on the damage.**

```
6. → PDR from the curve, capped                    PdrPercent
7. × PDR Mod (multiplicative, on the PDR)          → EffectivePdr
   then the damage is reduced once by EffectivePdr
```

`effective_pdr = pdr × (100 + mod) / 100`, one banker's rounding
(ADR-001 rev 2). Damage is then `damage × (100 − effective_pdr) / 100`.

## Rationale

The literal reading is not merely less precise — it is **provably wrong in
direction**. Lethal Mark is an attacker-applied debuff whose purpose is to
make the target take more damage; the duo analysis that sources these
numbers states it plainly:

> Lethal Mark (kastekniv): −30% Physical Damage Reduction Mod i 8s,
> genapplikerbar. Multiplikativ. Fighterens damage stiger direkte.
>
> — `docs/rogue-fighter-duo-hotfix123.md` §3

Under the literal reading damage would be multiplied by 0.70 — it would
**fall** by 30%, turning the Rogue's core debuff into a defensive buff for
the enemy. ADR-006 step 7 is therefore a drafting shorthand, not a competing
model, and ADR-002's typed signature carries the intended meaning.

Secondary argument: the literal reading double-reduces. Step 6 already
applies the defender's damage reduction; multiplying the reduced damage again
at step 7 compounds two reductions that the game applies as one.

## Consequences

- ✅ One statement of the mechanic instead of two that disagree
- ✅ `apply_pdr_mod` keeps ADR-002's signature; no code change was needed
- ✅ The `pdr_mod_additive` probe keeps its meaning: the ban is on *adding*
  the mod to the PDR, which this amendment does not reintroduce
- ⚠️ **Still unverified against the game.** This amendment settles what the
  documents mean, not what Ironmace implemented. The magnitude remains
  wiki-sourced, so resolved values carry `Unverified` (ADR-007) until an
  in-game test confirms that Lethal Mark scales PDR rather than subtracting
  percentage points. That test belongs to the dataset arc.
- The Rust and Python implementations already share this reading, so the
  vector corpus does **not** independently confirm it — agreement between two
  implementations of the same interpretation is not evidence about the game.
