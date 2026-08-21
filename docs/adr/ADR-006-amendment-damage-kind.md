# ADR-006 amendment: damage kind

Status: Accepted
Amends: ADR-006 (exchange model), the stats read at steps 3 and 5–7
Also amends: ADR-005 (resolve pipeline), stage 3 effect collection
Date: 2026-08-21

## The defect

Physical damage has a *kind* — Slash, Pierce or Blunt — underneath the
Physical/Magic type. The kind reached the dataset when weapon chains were
transcribed, and then stopped: it sat on `ComboHit` as a validated string
that nothing read. Another value that arrives, lands somewhere, and does
nothing, which is this project's recurring failure and by now its most
predictable one.

Worse, it was recorded next to a guess about what it was for. The guess —
that armour types resist the kinds differently — is wrong. **The kind does
not change the damage value.** The weapon's number does that, and a Blunt
swing and a Slash swing at the same combo position hit for the same amount.

What the kind is actually for is conditioning. Cleric's Blunt Weapon
Mastery reads:

> Increases physical attack power by 5% when attacking with a blunt weapon.

A bonus to a derived stat, gated on what the swing is. There is no way to
express that: every `Effect` in the model applies unconditionally, because
every one of them resolves into a character sheet, and a character sheet
cannot hold "5% when the swing is Blunt" — the same character swinging the
same Arming Sword is Slash on swings one and two and Pierce on swing three.

## Decision

**1. The kind is a type, and it reaches the strike.**

```rust
pub enum DamageKind { Slash, Pierce, Blunt }
```

`ComboHit.kind` becomes a `DamageKind` rather than a validated string, and
`Strike` carries `kind: Option<DamageKind>`. A plain swing takes the kind of
the chain's first swing, because a plain swing *is* the chain's first swing.
Counting a chain varies the kind along with the scaling, so swing three of
an Arming Sword is Pierce and is treated as Pierce.

`None` means the kind is unknown — an unarmed strike, a spell, or a weapon
whose chain nobody has transcribed. A gated effect never applies to a strike
of unknown kind, and never silently applies: a bonus that fires because we
did not know is worse than one that does not fire.

**2. An effect may be gated on a kind.**

```rust
pub struct StackedEffect { .., pub when_kind: Option<DamageKind> }
```

An ungated effect behaves exactly as before, so nothing in the dataset
changes meaning.

**3. A gated effect is held out of the sheet and applied at the strike.**

ADR-005 stage 3 folds `DerivedBonus` effects into the resolved stat. A gated
one cannot go there — it is not a property of the character. `Resolved`
grows a `conditional` list that stage 3 diverts them into, untouched by the
curve and the clamp, and the exchange applies the matching ones **at the
moment it reads the stat**, before the step that uses it.

This changes no step, removes none, and reorders none, so ADR-006's lock
holds. What it changes is what a step reads — the same shape as the damage
type amendment, one level further down.

## Consequences

A gated bonus is visible in the exchange trace and invisible on the
character sheet, which is correct and will look wrong to someone comparing
the two. The trace says which gate fired and on what kind; the sheet says
the stat has a conditional term and does not fold it in. Both statements are
true and the pair of them is the honest answer — folding it in would make
the sheet lie for every weapon the character is not currently holding.

Nothing here supports gating on a weapon *class*. Community reports say
Cleric's Blunt Weapon Mastery applies only to Maces rather than to all Blunt
damage, contradicting its own description. The description is what is
modelled, because the description is the source; the contradiction is
recorded in `data/README.md` and is not built on. If it is ever measured,
the gate grows a second form and this ADR gets an amendment of its own.

## Rejected

**Fold the gate into the character sheet by looking at the equipped
weapon.** A weapon has more than one kind across its chain, so the sheet
would have to pick one and be wrong for the others. It would also make the
sheet change when you switch weapons, which is not what a sheet is.

**Multiply the final damage instead of adding to the stat.** The perk says
attack power, and attack power is step 3. Applying it at step 9 would give a
different number wherever a flat bonus is involved, because the flat bonus
lands at step 4 — after the multiply and before the reduction.
