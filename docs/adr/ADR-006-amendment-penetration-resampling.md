# ADR-006 amendment — the exchange may read the dataset

**Status:** Accepted — 19 August 2026
**Amends:** ADR-006 (`ADR-000-010.md`), the purity clause and step 5–6
**Unchanged:** the nine-step order, and
`ADR-006-amendment-pdr-mod-layer.md`

## Context

ADR-006 states that damage is *"en ren funktion af tre inputs"* — attacker,
defender, context — and the implementation honoured it. Step 5 reduces the
defender's armour rating by the attacker's penetration, and step 6 turns
armour rating into PDR through a curve.

With only the two resolved stat blocks in hand, the exchange could not reach
the curve, so it rescaled the defender's already-resolved PDR in proportion
to the armour that survived. **That is wrong in direction, not merely
imprecise.** Light armour resolves to a negative PDR — the wiki's curve gives
−22% at 0 armour rating — and scaling a negative number toward zero makes a
penetrating hit deal *less* damage. Penetration must never help the defender.

Measured on the committed Hotfix 123 dataset, defender at armour rating 36:

| | PDR |
|---|---|
| no penetration | −6.88% |
| 10% penetration, curve re-sampled | **−8.392%** |
| 10% penetration, old rescaling | −6.192% |

The error grows with penetration, so a heavier-penetrating weapon read as a
*smaller* hit — the exact class of plausible-but-wrong number this project
exists to prevent.

## Decision

**The exchange takes the dataset the defender was resolved against, and
re-samples the PDR curve at the penetrated armour rating.**

```
5. armour rating × (100 − penetration) / 100, floored at zero
6. PDR = clamp(curve.sample(penetrated_rating) + offset, floor, cap in force)
```

The purity clause is relaxed from three inputs to four. ADR-006's stated
*reason* for it survives intact: damage still cannot be a method on either
party, because an `Exchange` remains a value object that owns nothing and
mutates nothing. Purity over exactly three inputs was the mechanism, not the
goal.

### The guard that makes this safe

Reading the dataset introduces a failure the three-input form could not have:
passing the *wrong* dataset. Resolving a defender against Hotfix 123 and then
computing damage against Hotfix 122's curves would produce a plausible wrong
number silently.

So the mismatch is made impossible to ignore:

- `DatasetSource` gains `build()`. A dataset that cannot say which version it
  is cannot be checked, and every dataset already knows.
- `Resolved` records the build it was resolved against.
- `Exchange::damage()` fails with `DatasetMismatch` when they differ.

`Resolved` also gains the class it resolved and the cap in force per capped
stat. The class is needed to find the derived-stat definition; the cap is
needed because a perk may have raised it (Defense Mastery, PDR 60% → 75%) and
a re-evaluation must clamp the same way. **Curves are deliberately not
carried** — that is the bulk, it is identical across every loadout, and
fetching it from the dataset is the whole point of this amendment.

## Alternatives considered

**Carry the curves on `Resolved`.** Keeps three-input purity and makes
version mismatch structurally impossible, which is a real advantage. Rejected
because it generalises badly: Will → Magic Resistance → Magical Damage
Reduction is the same shape and is documented on the wiki but not yet built,
and each new chain would need another field. It also duplicates identical
curve data across every resolved loadout in an impact diff.

**Approximate differently** — model penetration as a damage multiplier rather
than an armour reduction. Rejected: the game's mechanic is armour reduction,
and choosing an easier-to-compute model over the real one is how a tool
starts lying confidently.

## Consequences

- ✅ Penetration moves damage in the direction it must
- ✅ Future chains (magic resistance) re-sample the same way, with no new
  fields on `Resolved`
- ✅ A version mismatch is a named error rather than a wrong number
- ❌ Exchange tests need a dataset where they previously built two `Resolved`
  by hand
- ❌ `Resolved` grows three fields, and the damage model now depends on the
  `DatasetSource` trait
- ⚠️ **The result stays `Unverified`.** Re-sampling matches our reading of the
  mechanic — penetration reduces armour rating, armour rating maps through
  the curve — but Ironmace's actual formula has not been observed. Only an
  in-game test moves it to `Verified`.

## Guard against a repeat

A property test now asserts the invariant directly: for any armour rating and
any two penetration values, **more penetration never yields less damage**.
The defect was a direction error, and a direction invariant is what catches
direction errors — a golden fixture at one penetration value would not have.
