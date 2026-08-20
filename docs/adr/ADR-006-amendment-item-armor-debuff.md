# ADR-006 amendment: attacker-applied Item Armor Rating Bonus

Status: Proposed
Amends: ADR-006 (exchange model), steps 5–6; ADR-005 amendment (item armor
bonus); the shape of `Resolved`
Date: 2026-08-19

## Context

The ADR-005 amendment modelled Item Armor Rating Bonus as something a
character grants themselves. The wiki names two sources of that stat, and
only one works that way:

- **Fighter, Defense Mastery** (perk): +15% on your own equipped armour.
- **Rogue, Weakpoint Attack** (skill): *"When you successfully hit a target,
  reduce their armor rating by −30% Item Armor Rating Bonus for 3s. Only
  when melee attacking."*

The second points the other way. An attacker applies it to a defender, it is
negative, it lasts three seconds, and it only exists after a melee hit lands.
Nothing in the model currently expresses a modifier with those properties.

## Three decisions, and one obstacle

### 1. Which layer owns it

The exchange layer, not resolution. ADR-006 step 8 already holds the line
that a modifier belonging to *this attack by this attacker* must not enter
the defender's stat block, or "what is this player's PDR" stops having an
answer independent of who is swinging. Weakpoint is that kind of modifier —
more so than Lethal Mark, because it is also time-limited.

Whether the debuff is live is a fact about **the moment**, not about either
build. Two rogues with identical loadouts differ only in whether one of them
connected in the last three seconds. So it is stated in `ExchangeContext`,
never inferred from the attacker owning the skill — the same rule stacks
already follow, for the same reason.

### 2. How it composes with Defense Mastery

They are the same stat. The wiki names both *Item Armor Rating Bonus*, so
they sum, and no precedence rule is needed:

    net_bonus = Σ(defender's own bonuses) + (attacker-applied bonus)

A Fighter with Defense Mastery caught by Weakpoint resolves at +15 − 30 =
−15%. That falls out of treating the game's own stat as one quantity, which
is the whole reason it was worth checking that the wiki calls them the same
thing.

### 3. Order against armour penetration

Both reduce the armour rating that step 6 re-samples, and the order changes
the answer. It is settled by what each one is:

- The **bonus** is defender-side state — what their armour is worth right
  now, before anyone specific attacks it.
- **Penetration** is a property of the strike, applied to whatever armour it
  meets.

So the defender's rating is composed first, complete with any debuff, and
penetration subtracts from the result:

    effective_ar = item_ar × (100 + net_bonus) / 100 + other_ar − armor_pen
    pdr          = clamp(curve(effective_ar) + offset)

Step 6 already re-samples the curve at the penetrated rating; this changes
what is handed to it, not how it works. Rounding is unchanged: one banker's
rounding, still inside `stats::apply_item_armor_bonus`.

### 4. The same ability does not stack across applicators

Two rogues both landing Weakpoint on one target apply −30%, not −60%. The
second application refreshes the debuff rather than adding to it.

This is a rule about **abilities**, not about debuffs and not about this one:
the same ability never stacks with itself regardless of who brought it. Two
Jokesters in a party are +2 All Attributes, not +4, by the same rule that
makes two Weakpoints −30 rather than −60.

Resolution had that half wrong and now does not
(`collect_effects` de-duplicates on ability id; probe `ability_dedupe`).
This amendment is therefore the second consequence of one rule, and the
context below only has to get the exchange-side half right.

Two *different* abilities that both reduce Item Armor Rating Bonus would
still sum, because they are different abilities.

That is a statement about identity, so identity is what enforces it. The
context holds the mods **keyed by the ability that applied them**:

    item_armor_bonus_mods: BTreeMap<AbilityId, Confidence<Fixed>>

A map key cannot appear twice. Two rogues with Weakpoint write the same key
and produce one entry; two different abilities write two keys and sum. The
no-stacking rule is then not a check that some caller has to remember to
perform, and not a comment — it is a thing the type will not let you say.

A single summed `Fixed` was the first proposal here, and it was worse in two
ways: it would have accepted −60 without complaint, and the trace could not
have named which ability cost the defender what.

`AbilityId` is a new id newtype rather than `SkillId`, because a debuff of
this shape could as easily come from a perk, and the map must not be able to
hold `skill.x` and `perk.x` as distinct keys for one effect.

### 5. The obstacle: `Resolved` has already thrown the buckets away

`Resolved.derived["derived.armor_rating"]` is the **combined** figure. By the
time an exchange sees a defender, item armour and enchantments are one
number, and a percentage that applies to only one of them cannot be applied
to it at all. This is not a detail — it is the reason the amendment needs a
decision rather than a patch.

`Resolved` therefore records what stage 7 combined:

```rust
pub struct ArmorComposition {
    pub item:  Confidence<Fixed>,
    pub bonus: Confidence<Fixed>,   // the defender's own, in percentage points
    pub other: Confidence<Fixed>,
}
```

This is the precedent `caps` already set. `caps` exists on `Resolved` for
exactly this reason — a later re-evaluation has to clamp the way resolution
clamped — and the composition is the same kind of fact for the same kind of
consumer. A class with no armour resolves to three `Verified` zeros, so the
field is always present and never a special case.

The alternative — handing the exchange the defender's loadout — was
rejected. Re-deriving the defender inside the damage model is a much larger
hole in ADR-006's purity clause than carrying four numbers forward, and it
would let an exchange disagree with the `Resolved` it was given.

## Decision

1. `ExchangeContext` gains
   `item_armor_bonus_mods: BTreeMap<AbilityId, Confidence<Fixed>>`, empty by
   default — a neutral exchange has no debuff, and an empty sum is
   `Verified(0)` under the existing fold.
2. `Resolved` gains `armor: ArmorComposition`.
3. Step 5 composes `effective_ar` as above; step 6 re-samples at it.
4. `AbilityId` joins the id newtypes.
5. The trace shows the composition and names each debuff by its ability. A
   number that changed because someone else hit you must say who, and which
   ability of theirs.

## Tests this requires

- Defense Mastery (+15) and Weakpoint (−30) on the same defender resolve at
  −15%, not by applying one and discarding the other.
- The debuff reaches item armour and not enchantment rolls — the same
  three-way discrimination the ADR-005 amendment's test makes, from the
  other side.
- **Property: a larger debuff never yields less damage.** The sibling of the
  penetration property, and written for the same defect class — this
  amendment introduces a second signed quantity into the same subtraction,
  which is exactly where the penetration direction bug lived.
- A probe that applies the debuff to the combined rating instead of the item
  bucket must fail.
- Two applications of the same `AbilityId` resolve as one −30, never −60;
  two different abilities sum. The first case is structural, so the test is
  really asserting that the structure was not worked around.

## Open, and deliberately not decided here

- **Duration.** The three seconds are not modelled; the context states
  whether the debuff is live, and the caller owns the clock. A tool that
  answers "what does this hit do" does not need to simulate the window, and
  pretending to would be a worse lie than leaving it out.
- Whether Weakpoint's own AR reduction is affected by the defender's
  Armor Rating *Bonus* sources beyond items — moot while the wiki says
  plain Armor Rating Bonus does not exist in the game.

## Confidence

Weakpoint's −30% and its conditions are `Unverified`: wiki-sourced, not
measured in game.

The no-stacking rule is `Verified` — it comes from the same in-game
authority that settled the 60/75 PDR caps against the wiki's incorrect 65%,
and it is a rule about how the game treats abilities rather than a number
read off a page.
