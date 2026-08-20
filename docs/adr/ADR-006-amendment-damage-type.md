# ADR-006 amendment: damage type

Status: Accepted
Amends: ADR-006 (exchange model), steps 3 and 5–7
Date: 2026-08-20

## The defect

Every one of ADR-006's nine steps is physical. Step 3 adds Physical Power
Bonus, step 5 subtracts armour penetration from Armor Rating, steps 6 and 7
turn that into Physical Damage Reduction. A magic attack has no way through
the model at all.

The dataset already computes the other side of it. Magic Resistance and
Magical Damage Reduction resolve, gear grants Magic Penetration, and nothing
consumes any of them — three stats the tool works out and then cannot use.

## Decision

A strike carries its damage type, and the type chooses **which stats the
existing steps read**. It does not add a step, remove one, or change the
order, so ADR-006's lock holds unbroken.

```rust
pub enum DamageType { Physical, Magic }
```

| step | physical | magic |
|------|----------|-------|
| 3 | Physical Power Bonus | Magic Power Bonus |
| 5 | Armor Rating | Magic Resistance |
| 6–7 | Physical Damage Reduction | Magical Damage Reduction |

True damage keeps its own field and still lands after the whole reduction
chain at step 8, because bypassing reduction is what it means — it is not a
third type.

`Strike::armor_pen` becomes `Strike::penetration`. A strike has one type and
one penetration, and which defence it reduces follows from the type. Keeping
the physical name on a field a magic attack also uses would be the kind of
small lie that survives for years.

## Two assumptions, both graded rather than asserted

**Magic Power Bonus shares the Physical Power Bonus curve.** The character
sheet reads `Physical Power Bonus -11% / From Physical Power 10` and
`Magic Power Bonus -11% / From Magic Power 10` for the same character. That
is one point of agreement, not a proof that the curves are identical. The
stat is therefore graded **`unknown`** with that reasoning attached, so every
magic damage number comes back carrying the assumption it rests on, and
`--strict` refuses it.

**Magic Penetration reduces Magic Resistance the way armour penetration
reduces Armor Rating.** Both print as percentages on item cards and both
appear opposite a rating, which is symmetry rather than evidence. Recorded in
`data/README.md` as an open measurement.

## Consequences

Nothing changes for a physical attack: `Physical` is the default, and the
existing tests and vectors pass untouched. That is the check on whether this
was the right shape — an amendment that reorganised the physical path would
have been a rewrite wearing an amendment's name.

A magic attack against a defender whose class defines no Magical Damage
Reduction is a named error, not a silent zero, the same way a missing PDR
already is.
