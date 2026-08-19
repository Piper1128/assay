# In-game verification worksheet

Everything in `data/` is `unverified` unless noted: it comes from the wiki,
and two wiki pages agreeing is not independent verification (ADR-007). This
is the list of observations that would change that, ordered by what they buy.

Record raw pairs — no formatting needed — and they get transcribed into the
dataset as `verified`, after which `assay diff` shows whether anything moved.

There is an interactive version of this sheet that judges each reading as you
type it and assembles the reply for you:
<https://claude.ai/code/artifact/a1afc360-5a20-4422-9880-98e63eb707a0>

**Patch tested:** ____________  (values are per-build; record which one)

**Does the character sheet show decimals?** ____________
If it rounds, an observation confirms a value only to the precision shown.
A sheet reading "7.8%" is consistent with our 7.8125% but does not pin the
digits, and the note in the dataset must say so.

---

## 1. The AR→PDR curve — highest value

Not merely unverified: **invented**. The wiki's breakpoints above AR 75 were
never recovered, so the committed curve is a placeholder, and every PDR
number and all damage output rests on it.

Method: wear armour, read *Armor Rating* and *Physical Damage Reduction* off
the character sheet, note the pair. The curve reportedly bends at low
ratings, which is where most builds live, so those points matter most.

| Armor Rating | PDR observed | Placeholder says |
|---|---|---|
| 0 (naked) | | −22 % |
| ~6 | | −14.5 % |
| ~12 | | −17.0 % … see note |
| ~20 | | −13.6 % |
| ~75 | | 9.5 % |
| ~150 | | 30.5 % |
| ~250 | | 51.5 % |
| highest you can reach | | cap bites at 60 % |

*The placeholder is a straight line through three invented points, so its
low-AR values are not meaningful predictions — they are there only to make a
wildly different observation obvious. Eight real pairs are plenty; four beats
what we have.*

---

## 2. Naked baselines — cheapest

One character sheet per class confirms the base attributes **and** all four
curves at that input.

| | Rogue predicted | Rogue observed | Fighter predicted | Fighter observed |
|---|---|---|---|---|
| Health | 108.5 | | 125 | |
| Move Speed | 306 | | 300 | |
| Action Speed | 7.8125 % | | 0 % | |
| Physical Power Bonus | −14 % | | 0 % | |
| Magic Resistance | 1.5 % | | 7.8 % | |

A mismatch here is worth reporting immediately — it would mean a base
attribute or a curve is wrong at its most-used point.

---

## 3. Weapon tooltips — nearly free

Rarity I values.

| Weapon | Damage predicted | Damage observed | Armor pen predicted | Armor pen observed |
|---|---|---|---|---|
| Flanged Mace | 31 | | 10 % | |
| Morning Star | 31 | | 10 % | |
| War Hammer | 32 | | 10 % | |
| Club | 29 | | 5 % | |

---

## 4. The two decisions made by reasoning

These are the only places where an interpretation was chosen rather than a
value read. Both need visible damage numbers; skip them if those are hard to
read reliably.

**Lethal Mark applies to the PDR, not to the damage**
(`ADR-006-amendment-pdr-mod-layer.md`). Against a defender at 60 % PDR, the
mark should multiply damage by about **1.72**. The rejected reading predicts
about 1.43.

- damage unmarked: ________  marked: ________  ratio: ________

**Armour penetration must never help the defender**
(`ADR-006-amendment-penetration-resampling.md`). Hit a lightly-armoured
target with and without a penetrating weapon; penetration must make the hit
**larger**.

- damage without pen: ________  with pen: ________

---

## Recording format

Raw pairs are fine:

```
patch: 6.12 hotfix 123, sheet shows 1 decimal
rogue naked: hp 108.5, ms 306, as 7.8, ppb -14, pdr -22
AR 36  -> PDR -6.9
AR 112 -> PDR 22.4
flanged mace: 31 dmg, 10% pen
```
