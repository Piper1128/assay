# ADR-005 amendment: what gear contributes, from every slot

Status: Proposed
Amends: ADR-005 (resolution pipeline), stage 2; ADR-004 (dataset schema),
`ItemDef`; ADR-009 (loadout file)
Date: 2026-08-19

## The defect

Gear grants attributes two ways, and the model has one of them, on one kind
of slot.

Every equippable piece — armour, weapons, jewelry, cape — carries attributes
both as **static values printed on the item** and as **random rolls on the
individual copy**. Those attributes feed the whole pipeline: Will reaches
Magical Damage Reduction through the Magic Resistance chain, Strength reaches
Physical Power Bonus, Agility reaches Move Speed, and so on. The same is true
of the defensive stats on the physical side.

What exists today:

- `Roll::Attribute` — random rolls, and only on `ArmorPiece`.
- `ItemDef { armor_rating, move_speed_add, weapon }` — **no attributes at
  all**.
- `Loadout { armor: Vec<ArmorPiece>, weapons: Weapons { main_hand } }` — no
  jewelry, no cape, and a weapon that contributes damage and nothing else.

So a real build resolves low on every attribute-derived stat, and the error
is invisible: the numbers look like numbers. The Magic Resistance chain made
this conspicuous — a Fighter reads Will 15 and 7.8% Magical Damage Reduction,
when the Will on their rings and cape is part of the answer — but the chain
is not the defect. Its input is.

## Why static and rolled cannot be one field

They differ in **grade**, and the difference is the point of ADR-007.

A static attribute is a property of the item definition: dataset-sourced,
graded by whoever reviewed it, `Unverified` until measured. A roll is a
property of *this copy* — part of the question being asked — and enters as a
`Verified` fact, which is exactly why `Roll` exists separately from `ItemDef`
already (ADR-005 stage 2).

Folding static values into rolls would silently promote wiki-sourced data to
verified, and folding rolls into the item definition would make a question
about one player's gear into a claim about every copy of the item. Both
directions corrupt the grade, which is the one thing this tool sells.

## What the game shows, and how it names it

Three item cards settle most of the shape. **Loose Trousers** (Epic, Legs):
Armor Rating 25, Move Speed −2, Agility 4 in white; +2 Strength, +9 Magic
Resistance, +2.8% Demon Damage Reduction in blue. **Leather Cap** (Uncommon,
Head): Armor Rating 33, Headshot Damage Reduction 14%, Move Speed −3, Vigor 2
in white; **+11 Additional Armor Rating** in blue. **Phoenix Choker**
(Uncommon, Necklace): Magical Power 1, Magic Penetration 1% in white; +1
Additional Physical Damage in blue.

Three things follow that this amendment could not have guessed.

**The static/rolled split is the item card's own layout**, white against
blue. It is not a modelling convenience.

**"Additional" is the game's word for the second bucket.** A cap carries
`Armor Rating 33` printed on it and `+11 Additional Armor Rating` rolled onto
it, and the two are named differently on the same card. That is the
distinction the item-armor-bonus amendment invented as *item* and *other*,
and the game had a vocabulary for it already. It is also evidence for that
amendment's central assumption — an Item Armor Rating Bonus multiplying
`Armor Rating` and leaving `Additional Armor Rating` alone reads as the
obvious meaning of those two labels.

**Gear grants derived stats, not only attributes.** Magic Resistance,
Magical Power, Magic Penetration and Headshot Damage Reduction all appear on
item cards. Magic Resistance in particular is not an attribute and does not
arrive through Will, which the wiki said was its only source.

## The shape all four chains share

The character sheet breaks each defensive and offensive stat into a rating
and a second term, in the same layout every time:

    Physical Damage Reduction   -22%        Physical Power Bonus   -11%
      From Armor Rating         0 (-22%)      From Physical Power   10 (-11%)
      From Bonuses              0             From Bonuses          0
    Magical Damage Reduction    1.5%        Magic Power Bonus      -11%
      From Magic Resistance     15 (1.5%)     From Magic Power      10 (-11%)
      From Bonuses              0             From Bonuses          0

So each is `curve(rating) + bonuses`, and each rating is an attribute plus
gear: Physical Power is Strength plus gear, Magic Power is Will plus gear,
Magic Resistance is Will through a conversion plus gear, Armor Rating is
gear alone. We model the curve for three of the four and the gear seeding for
one, and no chain has its bonus term. The `+ bonuses` half is a separate
question and is not decided here — recorded in `data/README.md`.

## Decision

### 1. Items carry static attributes

```rust
pub struct ItemDef {
    // ...
    /// Attributes printed on the item, present on every copy. Sparse: an
    /// attribute the item does not grant is absent, not zero (ADR-001
    /// canonical encoding: absence is not null).
    pub attributes: Option<Confidence<BTreeMap<AttributeKind, i32>>>,
    /// Derived stats printed on the item — armour rating, magic resistance,
    /// magical power, magic penetration. Seeds the graph the way gear-
    /// sourced armour rating already does (ADR-012).
    pub grants: BTreeMap<DerivedStatId, Confidence<Fixed>>,
}
```

Sparse rather than an `AttributeBlock`, because a full block cannot say the
difference between "grants no Will" and "grants Will 0", and the canonical
encoding already forbids that conflation everywhere else.

`grants` replaces the special-cased `armor_rating` field. One item card can
carry armour rating, magic resistance and magical power at once, and adding
a field per stat means the schema changes every time the game does — where
ADR-012 has already established that a new derived stat is a dataset job.
`Roll` gains the matching general case, replacing `Roll::ArmorRating`:

```rust
Roll::Derived(DerivedStatId, Fixed)   // "+11 Additional Armor Rating"
```

Which bucket a contribution lands in follows the card's own wording: the
printed line is item-sourced, the `Additional` roll is not, exactly as
`ItemDef` and `Roll` already divide them.

### 2. A loadout has gear, not just armour

`Loadout.armor: Vec<ArmorPiece>` becomes `Loadout.gear: Vec<GearPiece>`, and
`GearPiece` carries an explicit slot:

```rust
pub enum Slot { Head, Chest, Legs, Hands, Feet, Cape, Necklace, Ring, Weapon }
```

The pipeline does not need the slot to add stats up — a piece is a piece —
so this is not the slot's job. It is here for two things the pipeline cannot
do without it: validating that a loadout is possible at all (one cape, two
rings), and answering the question in §4 below.

Weapons stay addressable as `weapons.main_hand` for the exchange, which needs
to know *which* piece is being swung, but their stats arrive through `gear`
like everything else's. A weapon that grants +3 Strength is not a special
case; it is a piece with attributes.

### 3. Stage 2 sums both, keeping the grades apart

Stage 2 already applies rolled attributes before stage 3's perks. It gains
the static ones, folded at the item's own grade, so a wiki-sourced item
attribute degrades the attribute block the way any unverified input does and
a rolled one does not.

Order within stage 2 does not matter — addition commutes, and there is one
rounding point for attributes because there is none: they are whole points.

## Open, and named rather than guessed

**Does a piece of jewelry that grants armour rating count as "item
armour"?** The Item Armor Rating
Bonus multiplies "armour rating from equipped armour, excluding
enchantments" (ADR-005 amendment: item armor bonus). Whether a ring's armour
rating is inside that base is undocumented. This is why `Slot` exists: when
the answer arrives it is expressible without another schema change. Until
then, every piece's `armor_rating` stays in the item bucket, which is the
current behaviour and is recorded here as an assumption rather than left as
an accident.

**Does the Will → Magic Resistance table continue past Will 100?** The
transcribed curve ends at Will 100 → Magic Resistance 209 and `Curve::sample`
clamps beyond it. Gear attributes make Will above 100 ordinary rather than
hypothetical, so this stops being academic the moment this amendment lands.
The sibling of the armour curve's end (`data/README.md`), and it needs a
measurement, not another reading.

**Per-rarity roll ranges** stay deferred. This amendment lets a loadout state
the rolls it has; it does not model what rolls are possible.

## Consequences

Every attribute-derived stat rises for a geared build, on both the physical
and magical sides. `assay-diff` must enumerate the new item field in the same
commit — `fields()` is hand-written, and a schema field that escapes it makes
the differ silently blind, which has happened once already and is documented
in that module.

The loadout file grows a `slot` on each piece and an `attributes` table that
is already there in spirit; `[[armor]]` becomes `[[gear]]`. Existing loadout
files break loudly rather than resolving low, because an unknown section is
rejected rather than ignored (ADR-009 deviation 2).

## Confidence

That gear carries both static and rolled attributes across all slots is
`Verified` — in-game knowledge from the same source that settled the PDR caps
and the no-stacking rule. The individual item values are `Unverified` until
each is transcribed and reviewed.
