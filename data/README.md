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

**Two stats fetched and deliberately not written.** Every curve in this
dataset had to reproduce the value the game prints for a naked Rogue before
it was committed — one anchor per stat, from a source that is not the wiki.
Nine passed. Two did not, and are recorded here rather than guessed at:

- **Regular Interaction Speed.** The sheet reads 46% for a Rogue with
  Dexterity 20 and Resourcefulness 25. The transcribed curve gives 15.6%
  under the stated 0.25/0.75 weighting, 11.2% reversed, and 53.6% unweighted.
  None of them is 46, so either the weighting or the curve is wrong and there
  is no way to tell which from here.
- **Debuff Duration Reduction.** The page describes an extensive table and
  produced three anchors, with a sign convention that does not line up with
  the sheet's wording — the wiki gives a duration, the sheet gives a
  reduction, and 12.4% does not fall out of either reading.

**Two assumptions the magic chain rests on**, both graded rather than
asserted, so nothing built on them can be mistaken for measured:

- **Magic Power Bonus shares the Physical Power Bonus curve.** The character
  sheet reads `-11%` for Physical Power 10 and `-11%` for Magic Power 10 on
  the same character. That is one point of agreement, not a proof. The curve
  is graded `unknown` and carries that reasoning, so every magic damage
  number comes back with it attached and `--strict` refuses it.
- **Magic Penetration reduces Magic Resistance the way armour penetration
  reduces Armor Rating.** Both print as percentages on item cards and both
  sit opposite a rating. That is symmetry rather than evidence.

**No weapon has a swing time, so no fight has a duration.** The model
computes hits-to-kill from damage and health, which is arithmetic on numbers
it already has. Turning that into seconds needs one number per weapon that no
item card prints: the interval between swings at 0% Action Speed.

The field exists on `WeaponProfile` and is empty everywhere. That is
deliberate — three times in this project a measured value arrived with no
place to put it and sat doing nothing, and a tool that cannot record what you
measured asks you to measure it twice. Measure one weapon and time-to-kill
works for it immediately.

Two assumptions in how the seconds are computed, recorded rather than hidden:

- **Action Speed divides.** At +100% a swing takes half the time. Multiplying
  by `(100 - speed)` would make +100% take no time at all, which is the kind
  of formula that looks right until someone reaches the number that breaks
  it.
- **Every swing costs its own time**, so `n` hits take `n × t`. Whether the
  first blow lands at zero or after one interval decides a close race by one
  swing, and nothing has measured which.

**Other gaps**

- Hotfix 122's build id `0.17.149.9316` comes from ADR-004's example, not from
  a page that states it. The label and release date are from the wiki.
- Leviathan (+1 damage) and Longbow (action speed) changed in Hotfix 123 but
  are absent: the Leviathan page was not located, and the Longbow note gives
  no number — weapons carry no action speed field either.

## `item.blank`

An item that grants nothing, in no slot. It exists so a piece can be described
entirely by its rolls — which is what the browser UI does with a card it read
off a screenshot, and what anyone does with gear the dataset has never heard
of.

Grading it that way is not a shortcut. The static/rolled split matters for the
*dataset*, where a printed value is a claim about every copy of an item. On a
loadout it is a fact about the copy in your hands, and every line on a card you
are looking at is equally that. So the whole card arrives as rolls, `Verified`,
and nothing is promoted or demoted on the way in.

## Weapon chains, and the timings that do not exist

Asked to find swing times and combo data and record them as `unverified`,
the answer split cleanly in half.

**The chains exist.** The wiki's weapon pages print a Primary Attacks line
per weapon, and the weapons differ from each other in ways that matter:

| Weapon | Chain | Scaling |
| --- | --- | --- |
| Arming Sword | Slash / Slash / Pierce | 100 / 105 / 110 |
| Flanged Mace | Blunt ×3 | 100 / 105 / 110 |
| Morning Star | Blunt ×3 | 100 / 105 / 110 |
| War Hammer | Blunt ×4 | 100 / 105 / 110 / 115 |
| Club | Blunt ×3 | 100 / 110 / 110 |

Two things are worth noticing. The War Hammer's chain is a swing longer
than everyone else's, so its last blow is the hardest in the dataset. And
the Club climbs on its *second* swing where the others climb on their
third, which is the sort of difference that would never show up in a tool
that treats a weapon as one number.

All of it is recorded `unverified`: the wiki has contradicted the game
three times in this project, and a chain nobody has watched land is a
transcription, not a measurement.

**The timings do not exist.** No page carries a swing time, a windup, an
animation length, or an attacks-per-second figure — for any weapon, in any
unit. So `WeaponProfile.swing_time` stays empty and time-to-kill keeps
saying it does not know, which is the correct answer until someone with
the game open produces one.

Measuring one weapon would be enough to make the race readable, because
the point is the ratio between two builds, not the absolute seconds.

**What the kind is for.** Physical damage also has a *kind* — Slash,
Pierce or Blunt — separate from the Physical/Magic split. It does **not**
change the damage value: the weapon's own number does that, and a Blunt
swing and a Slash swing of the same weapon at the same combo position hit
for the same amount. Armour does not resist the kinds differently. (An
earlier note here guessed it might; that guess was wrong and is recorded
rather than deleted, next to the flat-PDR one.)

The kind exists so perks and skills can condition on it. Cleric's Blunt
Weapon Mastery reads *"Increases physical attack power by 5% when
attacking with a blunt weapon"* — a bonus to a derived stat, gated on
what the swing is. That is the whole mechanism.

**Gating is built; seeing it needs a Cleric.** The mechanism resolves,
traces and is tested — a gated bonus stays out of the character sheet and
is added at the moment the exchange reads the stat, so it moves the number
for a mace and not for a sword. The one perk that uses it belongs to the
Cleric, and there is no Cleric class in the dataset, so nothing can slot
it yet. A character sheet screenshot is all that is missing.

While adding it: perks were **not** class-locked. `ItemDef` has
`required_classes` and `PerkDef` did not, so a Fighter could slot a Cleric
perk and resolve a stat block nobody in the game can have. Now they match.

**Now exercised.** `fixtures/slice/` carries a Cleric holding the perk and
swinging twice — once blunt, once slash. The pair matters: either case alone
would pass with the gate wired to nothing. It found a bug the moment it
existed, which is the point of the entry this replaces.

**Was not exercised by any committed fixture, and that mattered.** The Rust and the Python mirror
both implement the gate and both were checked by hand, but nothing in
`fixtures/slice/` swings a gated blow — the only gated perk is Cleric-only
and there is no Cleric class, so no loadout can take it. The cross-check
therefore agrees about the gate without ever testing it. The Cleric class
closes this at the same time as everything else it blocks.

**Open, and it contradicts the card:** community reports say Cleric's
Blunt Weapon Mastery applies only to Maces rather than to all Blunt
damage. The description is what is modelled, because the description is
the source; the contradiction is written down here and not built on.

### Two hit counts, and why both stay

A chained weapon never lands the same blow twice running, so the tool
prints two figures that answer different questions:

- **hits to kill** — this blow, repeated. The answer when you pinned a
  swing or named a skill, because that is what you asked about.
- **swings to kill** — the weapon used as it is used. Shown only for a
  plain swing of a weapon with a recorded chain.

They differ often enough to matter: across the eight loadout pairings in
`loadouts/`, three of them kill a swing sooner on the chain than the
repeated blow suggests. Collapsing them into one heading would give two
readers different arithmetic under the same word.

Unmeasured, and it would only push the chain figure up: whether the game
resets a chain on a miss or after a pause. An uninterrupted chain is the
floor, not a guess.

## Two more classes, and forty numbers that came back

Cleric and Wizard were read off naked character sheets and added. The derived
block is identical across classes — checked, not assumed — so a class is its
base attributes and nothing else.

Both sum to **105**, as the Rogue's does. One sheet was Lv. 12 and the other
Lv. 1, which answers a question nobody had asked out loud: **character level
does not move the attribute total**. These are class constants and not a
snapshot of one character.

The curves were built from a single Rogue sheet. Two new classes, at two
levels, with very different spreads, now reproduce them:

| | Cleric (sheet → ours) | Wizard (sheet → ours) |
| --- | --- | --- |
| Physical Power → bonus | 11 → −8% ✓ | 6 → −25% ✓ |
| Magic Power → bonus | 23 → 8% ✓ | 20 → 5% ✓ |
| Magic Resistance → MDR | 62 → 17.2% ✓ | 50 → 14.1% ✓ |
| Health | 120 ✓ | 109 → 108.5 ⚠ |
| Move Speed | 297 ✓ | 300 ✓ |
| Memory Capacity | 14 ✓ | 19 ✓ |
| Action Speed | −1.5% ✓ | 1.9% → 1.875 ✓ |
| Spell Casting Speed | 10.5% ✓ | 21% ✓ |

**Forty values, thirty-nine exact.** The one that is not is Wizard health:
we compute 108.5 and the sheet prints 109, which is consistent with the game
rounding a half up for display where we round half to even. That is a display
question, not a curve question, and it is the only thing these two sheets did
not settle.

**What this does to a flagged assumption.** `derived.magic_power_bonus`
carried a note saying it was *assumed* to share the Physical Power Bonus
curve on the strength of one point of agreement. There are now four points
across two independent characters — 6 → −25, 11 → −8, 20 → 5, 23 → 8 — and
they sit on one monotonic curve with a decreasing slope. Still an assumption,
and a far better supported one; the note stays until someone measures a case
where the two would have to differ.

## Blunt Weapon Mastery, corrected from the card

It was entered from a wiki mirror as *"Increases physical attack power by
5%"*. The card itself reads:

> While using a blunt weapon, gain **10% physical damage bonus**.

Both the number and the wording were wrong, and the source was `documented` —
the one method ADR-013 had just locked as attesting but never corroborating.
The rule met its first real case in the hour it was written.

It is modelled as a bonus to Physical Power Bonus, because that is the sheet
row an item's Physical Damage Bonus is believed to land in. **That belief is
still unmeasured**, and two perks now depend on the answer.

## Damage has a school, and there is no room for it

Four spells and two perks name one:

| | |
| --- | --- |
| Holy Strike | `20(1.0)` **divine** magical damage |
| Locust Swarm | `13(1.0)` **earth** magical damage |
| Fireball | `30(1.0)` direct **fire** magical damage, `10(1.0)` splash |
| Ice Bolt | `20(1.0)` **ice** magical damage |
| Faithfulness | 15% **divine** magical damage bonus |
| Fire Mastery | 5% **fire** magical damage bonus |

`DamageType { Physical, Magic }` cannot express it, and the character sheet
carries an Undead/Demon axis on top — target-type modifiers we do not model
either. This is the damage-kind amendment one level further out and it
touches ADR-006's lock, so it needs an ADR before anything is built.

Also worth recording: the `N(x)` notation on every spell is `base(scaling)`,
which is exactly `StrikeProfile`'s shape. Hotfix #124's line *"Reduced the
Burn Multiplier of Meteor Strike from 1.0 to 0.5"* confirms the reading —
the parenthesised number is the multiplier.

## Hotfix #124 (21 August 2026)

Nothing in it touches this dataset. Every balance line is Ranger, Wizard,
Warlock or Sorcerer — the classes affected are ones we have no entities for
yet, and the two monster changes are not modelled at all. Recorded so the
next person does not go looking for a diff that has nothing to show.

The build id is not printed in the patch notes, so a `0.17.151.x` directory
cannot be created from them; it has to be read off the game client.
