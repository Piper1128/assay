# ADR-005 amendment: gear contributes attributes, from every slot

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

## Decision

### 1. Items carry static attributes

```rust
pub struct ItemDef {
    // ...
    /// Attributes printed on the item, present on every copy. Sparse:
    /// an attribute the item does not grant is absent, not zero
    /// (ADR-001 canonical encoding: absence is not null).
    pub attributes: Option<Confidence<BTreeMap<AttributeKind, i32>>>,
}
```

Sparse rather than an `AttributeBlock`, because a full block cannot say the
difference between "grants no Will" and "grants Will 0", and the canonical
encoding already forbids that conflation everywhere else.

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

**Does jewelry armour rating count as "item armour"?** The Item Armor Rating
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
