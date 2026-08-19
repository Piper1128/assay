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
- **Sprint is `unknown`** — the 13 is from the patch notes, but the schema
  does not model stacks, so applying it as a flat add describes one stack of
  three. The note says so, and the grade makes it propagate.

## Known gaps

- The AR→PDR curve is a **placeholder**. The wiki's breakpoints above AR 75
  were not recovered in full, and inventing them is the failure mode this
  project exists to prevent.
- Hotfix 122's build id `0.17.149.9316` comes from ADR-004's example, not from
  a page that states it. The label and release date are from the wiki.
