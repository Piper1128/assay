# ADR-014: damage schools

Status: Proposed
Amends: ADR-006 damage-kind amendment (generalises its gate)
Relates to: ADR-006 (exchange model), ADR-012 (derived stats as ratings)
Date: 2026-08-22

## The evidence

Six Cleric and Wizard cards, read off the game:

| | |
| --- | --- |
| Holy Strike | `20(1.0)` **divine** magical damage |
| Locust Swarm | `13(1.0)` **earth** magical damage |
| Fireball | `30(1.0)` direct **fire** magical damage, `10(1.0)` splash |
| Ice Bolt | `20(1.0)` **ice** magical damage |
| Faithfulness | Gain 15% **divine** magical damage bonus |
| Fire Mastery | Gain 5% **fire** magical damage bonus |

Every magical attack names a school, and perks grant school-specific damage
bonuses. `DamageType { Physical, Magic }` cannot express either.

## The defect, and what it is not

This is not a new mechanism. It is the damage-kind amendment again, on the
other side of the type:

> Physical damage has a **kind** — slash, pierce, blunt.
> Magical damage has a **school** — divine, earth, fire, ice.

Both are a tag on the strike. Neither changes the number: the weapon decides
physical damage and the spell decides magical damage, exactly as
`20(1.0)` says. Both exist so that perks and skills can condition on them —
Blunt Weapon Mastery on a kind, Fire Mastery on a school, in the same
sentence shape and with the same arithmetic behind it.

Building a second, parallel gate for schools would mean two tag fields on
`Strike`, two on `StackedEffect`, two gate checks in the exchange and two
probes, to express one idea twice. The defect is not that schools are
missing. It is that the gate was built narrower than the thing it models.

## Decision

**1. The gate generalises. `DamageKind` becomes `DamageTag`.**

```rust
pub enum DamageTag {
    // physical
    Slash, Pierce, Blunt,
    // magical
    Divine, Earth, Fire, Ice,
}
```

`Strike.kind` becomes `Strike.tag`, `StackedEffect.when_kind` becomes
`when_tag`, and the existing gate mechanism — divert out of the sheet at
ADR-005 stage 3, apply at the stat read in the exchange — is untouched.
Nothing about the nine steps changes, which is what keeps ADR-006 locked.

**2. A tag belongs to a type, and the loader enforces it.**

Rust cannot stop `Blunt` reaching a magical strike, so the dataset loader
does: a physical strike or effect tagged with a school, or a magical one
tagged with a kind, is refused by name at load. That is where every other
nonsense value in this project is caught, and it is the same reason
`DamageKind::parse` refuses a kind nobody recognises — a tag that can never
match reads exactly like a perk that does nothing.

**3. The list is closed, and grows by measurement.**

Four schools are attested. There are almost certainly more, and an unknown
one is refused rather than accepted as a string, for the reason above. Adding
one is a one-line change and a card.

**4. School-gated bonuses target Magic Power Bonus.**

Mirroring the physical case: *"gain 5% fire magical damage bonus"* is modelled
as a `DerivedBonus` on `derived.magic_power_bonus`, gated on `Fire`.

**This rests on an unmeasured belief** — that an effect's *Damage Bonus* lands
in the sheet's *Power Bonus* row under `From Bonuses`. Blunt Weapon Mastery
already depends on it and so does Great Helm's +1.7%. This ADR makes a third
thing depend on it and does not settle it; it is recorded in
`data/README.md` and one screenshot closes it.

## Consequences

`0(1.0)` spells become authorable: the `N(x)` notation on every card is
`base(scaling)`, which is `StrikeProfile`'s existing shape. Hotfix #124's
line *"Reduced the Burn Multiplier of Meteor Strike from 1.0 to 0.5"*
confirms the reading — the parenthesised number is the multiplier. So the
spells above can enter the dataset as skills the moment this lands, and
`assay diff` will then see a patch that reweights one.

Renaming `when_kind` to `when_tag` is a dataset change, not only a code one.
Three items and one perk carry the old key. It is mechanical and it is
better done while four files carry it than when forty do.

## Deferred, deliberately

**The Undead/Demon axis.** The character sheet carries Undead Damage Bonus,
Demon Damage Bonus, Undead Damage Reduction and Demon Damage Reduction. These
are *not* schools: they are properties of who is being hit, not of the blow.
Modelling them needs a defender type, and the dataset has no monsters at all,
so there is nothing to gate on yet. Recorded rather than designed, because
designing against zero entities is how a mechanism gets built for a consumer
that never arrives.

**Direct versus splash.** Fireball deals `30(1.0)` direct and `10(1.0)`
splash. That is two strikes from one cast, not a flavour of one, and it wants
a skill that yields more than a single `StrikeProfile`.

**Damage over time.** *"Burn: the target takes `3(0.5)` fire magical
damage"*, and Locust Swarm deals its damage per second over six. The exchange
model answers "what does this blow do"; a duration is a different question
and pulling it into the nine steps would be the first thing to bend ADR-006's
lock.

**Debuffs an attack inflicts.** Frostbite takes 20% move speed and 20% action
speed; Faithfulness takes 15% move speed bonus for one second. The model has
`ItemArmorBonus` debuffs already, so the shape exists — but every one of
these carries a duration, which lands in the same deferral as above.

## Rejected

**A separate `School` type beside `DamageKind`.** Two fields, two gates, two
probes, for one idea. The nonsense combinations it would prevent at compile
time are prevented at load instead, where every other invalid value in this
dataset is already caught.

**A free-text school.** A typo would be a gate that never fires, which is
indistinguishable from a perk that does nothing — the exact failure this
project has now named five times.

**Making a school change the damage.** Nothing in six cards suggests it.
`20(1.0)` is the same shape whether the school is divine or ice, and the
bonus perks are separate effects. A school that quietly multiplied would be
inventing a mechanic to fit a type.

**Treating divine and fire as damage *types* beside Physical and Magic.**
Every card says "divine **magical** damage" and "fire **magical** damage".
The school qualifies magic; it does not replace it. Promoting them would
break the step-3 and step-5 stat selection that the damage-type amendment
locked, and for a reading the cards contradict.
