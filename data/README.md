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

**PDR has a contributor the model does not have.** The armour→PDR curve is now
transcribed from the wiki's conversion table — 28 segments from armour rating
−300 to 600, internally consistent, and independently reproducing the −22%
the page states for a character wearing nothing. But it **tops out at 61.8%
at armour rating 600**, while 75% PDR with Defense Mastery is *verified in
game*. The armour curve alone cannot produce that number.

So `PDR = clamp(curve(armour rating))` is incomplete: something else adds to
it — flat PDR rolls on gear are the obvious candidate, since the game rolls
Physical Damage Reduction as an item stat. Until that is modelled, a
heavily-armoured build resolves low, and the 75% cap never binds because
nothing can reach it. Discovered by putting real data in; flagged rather than
patched, because the fix is a schema question.

**Other gaps**

- Hotfix 122's build id `0.17.149.9316` comes from ADR-004's example, not from
  a page that states it. The label and release date are from the wiki.
- Leviathan (+1 damage) and Longbow (action speed) changed in Hotfix 123 but
  are absent: the Leviathan page was not located, and the Longbow note gives
  no number — weapons carry no action speed field either.
