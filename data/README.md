# Datasets

Hand-approved, versioned game data (ADR-003, ADR-004). This directory is the
**single source of truth**: the mirror generator reads it, the Rust loader
reads it, and `fixtures/slice/duo_slice.json` is derived from it.

One directory per game build, keyed by build id with the patch label as a
human-friendly alias. `manifest.json` carries per-file provenance and the
predecessor build, so the chain is walkable.

| Build | Label | Released |
|---|---|---|
| `0.17.149.9316` | hotfix-122 | 2026-08-06 |
| `0.17.150.9384` | hotfix-123 | 2026-08-13 |

Source: the [Dark and Darker Wiki](https://darkanddarker.wiki.spellsandguns.com/)
(spellsandguns), CC BY-SA 4.0, plus its `Patch:6.12` page for the before/after
pairs. Dark and Darker is a trademark of Ironmace Games; this is an
unaffiliated fan tool.

## What these versions do and do not contain

**They are not complete snapshots of the game.** They contain the entities the
resolver can currently express: classes, the attribute→derived curves, items
with armour rating and flat move speed, and perks/skills whose effects are in
the ADR-004 vocabulary. Weapons, cooldowns, spell coefficients and monsters
are absent because nothing consumes them yet.

**Hotfix 122 differs from Hotfix 123 in seven recorded places:** Sprint's
Additional Move Speed per stack (15 → 13), and the weapon changes to Flanged
Mace, Morning Star, War Hammer and Club. Weapon values are the Rarity I base
from each weapon's wiki page; the Hotfix 122 figures are those minus the delta
the `Patch:6.12` notes state, which is why they are graded the same as the
values they were derived from.

Still outside the schema, and therefore absent: Leviathan +1 damage (weapon
page not located), Longbow's action speed increase (no number given, and
weapons carry no action speed field yet), Lethal Mark becoming reapplicable,
the Barbarian/Cleric/Sorcerer adjustments, and monster health. A diff between
these versions is honest but narrow, and widens as the schema does.

## Confidence

Values are graded per field (ADR-007). Almost everything here is
`unverified`: it comes from the wiki, and two wiki pages agreeing is not
independent verification. Two exceptions:

- **The PDR cap (60%, raised to 75% by Defense Mastery) is `verified`** — it
  was tested in game, which is why it overrides the wiki's contradictory 65%.
- **Sprint's per-stack value is `verified`** — it comes straight from the
  Hotfix 123 patch notes, and now that stacks are modelled the number means
  what the notes say it means. A loadout that does not state how many stacks
  are up still resolves at the maximum and is graded `unknown` for *that*
  reason, which is a property of the question rather than of the data.

## Known gaps

**Where the armour curve ends is unknown.** The wiki's conversion table runs
from armour rating −300 to 600 in 28 segments — internally consistent, and
independently reproducing the −22% the page states for a character wearing
nothing. It stops at 600, reaching 61.8%, and says nothing about what happens
above that. `Curve::sample` clamps outside its point range, so we currently
answer 61.8% for any armour rating past 600. That is a modelling choice
standing in for a fact we do not have.

It matters because the PDR cap is 60%, lifted to 75% by Defense Mastery. The
base cap binds — the curve crosses 60% around armour rating 564 — but whether
75% is reachable depends entirely on whether the game keeps going at the final
0.05%/point rate or genuinely stops. Resolving this needs a measurement in
game at high armour rating, not another reading of the table.

*This entry previously claimed the missing piece was flat PDR rolls on gear.
It was not. The wiki lists no source of flat PDR from items, enchantments,
perks or skills; the actual omission was the Item Armor Rating Bonus, upstream
of the curve, now modelled per the ADR-005 amendment. The wrong guess is
recorded here because it was wrong for an instructive reason: a shortfall
downstream was assumed to have a downstream cause.*

**The Magical Damage Reduction cap has never mattered, and may be wrong.**
The chain is transcribed and validated: Will → Magic Resistance in six
segments, Magic Resistance → Magical Damage Reduction in ten, and every
segment's stated per-point rate reproduces the next anchor exactly. The wiki
caps the result at 65%.

But Will alone tops out at Magic Resistance 209 — the end of the conversion
table, at Will 100 — which the curve turns into 47.8%. The cap cannot bind,
so its value has never been exercised by anything.

That is worth saying out loud because **the wiki made exactly this claim
about Physical Damage Reduction, also 65%, and it was wrong**: measured in
game, PDR caps at 60% and Defense Mastery lifts it to 75%. The same number on
the same page for the sibling stat is not evidence of anything, and the
65% is carried here only because nothing yet depends on it. A canary test
fails if MDR ever reaches its cap.

*Correction: the wiki page states no source of Magic Resistance other than
Will, and that was checked rather than assumed — but the game disagrees with
it. An Epic pair of Loose Trousers rolls **+9 Magic Resistance**. So the cap
is reachable in play, and it holds here only because gear-sourced Magic
Resistance is not modelled yet.*

**The `From Bonuses` term is modelled now, and nothing fills it yet.** Read
straight off the character screen:

    Physical Damage Reduction   -22%
      From Armor Rating         0 (-22%)
      From Bonuses              0
    Magical Damage Reduction    1.5%
      From Magic Resistance     15 (1.5%)
      From Bonuses              0

Both reductions are `curve(rating) + Bonuses`, and the two chains are the
same shape. The second term exists in the model now: gear grants it through
`grants`, abilities through `Effect::DerivedBonus`, and `assay resolve
--explain` prints the decomposition in the same layout so one of our numbers
can be checked against one of the game's *for the same reason* rather than
merely compared. It lands after the curve and before the clamp, so a bonus
cannot push a stat past a cap.

What is missing is data: no item card read so far grants flat Physical or
Magical Damage Reduction, and no perk is known to. The row is there, and the
sources that fill it are still unidentified.

One caveat, and one question that turned out to have an answer.

The decomposition covers stage 4. **Move speed is still adjusted afterwards
by stages 5 and 6**, so its parts do not add to its total, and the readout
says a later stage moved it rather than guessing which.

*Resolved: those flat move speed adds look like the same term under a
different mechanism, and folding them in was tempting. It would have been
wrong. Confirmed in game: Agility gives base move speed and armour reduces
it flat afterwards, so the 330 cap binds on the base. Stage 5 was already
right. The two readings differ only for a build fast enough to reach the cap
— Agility 75 with 5 points of armour penalty is 325 one way and 330 the
other — so the order is pinned by a test rather than left to look
interchangeable.* This vindicates the
first guess made when the armour curve fell short of its cap — that something
*adds* to the reduction — which was then withdrawn because the wiki lists no
source of flat damage reduction. The wiki was incomplete; the sheet has a
line for it. Note that Item Armor Rating Bonus is not that line: it feeds the
rating, which is the row above.

**Other gaps**

- Hotfix 122's build id `0.17.149.9316` comes from ADR-004's example, not from
  a page that states it. The label and release date are from the wiki.
- Leviathan (+1 damage) and Longbow (action speed) changed in Hotfix 123 but
  are absent: the Leviathan page was not located, and the Longbow note gives
  no number — weapons carry no action speed field either.
