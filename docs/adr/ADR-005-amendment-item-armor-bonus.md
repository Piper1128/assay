# ADR-005 amendment: Item Armor Rating Bonus

Status: Accepted
Amends: ADR-005 (resolution pipeline), stage 7 (defensive chain)
Date: 2026-08-19

## The defect

The pipeline computed the defensive chain as

    PDR = clamp(curve(Σ item armor_rating), floor, cap)

That sum is not the armour rating the game converts. The wiki states the
conversion input as

    Final Armor Rating = Armor Rating from armor × (1 + Item Armor Rating Bonus)
                       + other Armor Rating

Two things follow, and we modelled neither.

**A multiplier exists and was missing.** Fighter's Defense Mastery reads:
"Gain an additional 15% Item Armor Rating Bonus from equipped armor, and
raise your Physical Damage Reduction cap to 75%." The perk has two halves.
The dataset carried only the cap raise, so the perk that most changes a
tank's armour changed nothing but a ceiling that never bound.

**The multiplier has a restricted base.** It applies to armour rating from
equipped armour pieces only — not to enchantment rolls on those pieces, and
not to armour rating from any other source. A single summed bucket cannot
express that, because once the parts are added together the distinction is
gone.

## How the gap was found, and what it was not

The base PDR cap is 60% and Defense Mastery lifts it to 75% (both verified
in-game). The documented curve reaches 61.8% at armour rating 600 and the
table stops there. So a raised cap appeared unreachable, and the first
hypothesis was that gear rolls flat Physical Damage Reduction.

The wiki does not support that: it lists no source of flat PDR from items,
enchantments, perks or skills. Building a `pdr_bonus` field would have added
a mechanism the game does not have. The real missing piece was upstream of
the curve, not downstream of it.

## Decision

Stage 7 splits armour rating into two buckets and combines them before the
curve is sampled.

1. **Item armour** — the `armor_rating` of each equipped armour piece.
2. **Other armour** — `Roll::ArmorRating` enchantments on those pieces.
   The bucket is named for what it is rather than for its only current
   member, because the formula's `other` term is open.

Then, with `bonus` the sum of all `Effect::ItemArmorBonus` in force:

    final_ar = item_ar × (100 + bonus) / 100 + other_ar

One banker's rounding, in `stats::apply_item_armor_bonus`. The multiply
happens before the addition, so `other_ar` is never touched by it — the
ordering *is* the exclusion rule, and it is why the buckets stay separate
until this line.

`Effect::ItemArmorBonus` stacks: a percentage is a quantity, unlike the
ceiling in `RaiseCap`.

## Consequences

A Fighter in plate now resolves higher than before, and the 75% cap becomes
something the pipeline can express even where the documented curve cannot
yet reach it.

**The curve's end is still an open question, and this amendment does not
close it.** `Curve::sample` clamps outside its point range, so armour rating
above 600 yields 61.8% — a modelling choice, not an observed fact. The wiki
table ends at 600 with no stated continuation. Whether the game continues at
the final 0.05%/point rate, or genuinely stops, decides whether 75% is
reachable at all. Recorded in `data/README.md` as an open gap; not guessed at
here.

## Confidence

Defense Mastery's 15% is `Unverified` — wiki-sourced, not measured. The
existing cap raise stays `Verified`; it was confirmed in-game.
