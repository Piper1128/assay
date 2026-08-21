# ADR-014: damage schools

Status: Accepted
Amends: ADR-006 damage-kind amendment (generalises its gate)
Relates to: ADR-006 (exchange model), ADR-012 (derived stats as ratings)
Date: 2026-08-22
Locked: 2026-08-22

## The evidence: six cards, and a documented list

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

The wiki's Damage Types page names **twelve**, and says the list is closed:

> Fire · Ice · Lightning · Earth · Arcane · Light · Dark · Evil · Curse ·
> Divine · Air · Spirit

> All Magical damage is **one or more** of the following types, otherwise it
> is **Neutral** damage.

It names the physical three as Slash, Pierce and Blunt, which is what the
model already has — one independent source agreeing with cards we read
ourselves.

That page is `documented`, and ADR-013 has just finished saying what that is
worth: it attests and it never corroborates. It was also a `documented`
source that had Blunt Weapon Mastery wrong this morning. So the twelve are
taken as a shape to design against, not as twelve facts; the four with cards
behind them are the four that are known.

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
    Fire, Ice, Lightning, Earth, Arcane,
    Light, Dark, Evil, Curse, Divine, Air, Spirit,
}
```

`StackedEffect.when_kind` becomes `when_tag`, and the existing gate mechanism
— divert out of the sheet at ADR-005 stage 3, apply at the stat read in the
exchange — is untouched. Nothing about the nine steps changes, which is what
keeps ADR-006 locked.

**2. A strike carries a *set* of tags, not one.**

```rust
pub struct Strike { .., pub tags: BTreeSet<DamageTag> }
```

This is the wiki's "one or more" and it is the whole reason to read the page
before writing the code. The first draft of this ADR gave a strike a single
optional tag, which cannot say what the source says.

Whether "one or more" means one blow carrying two schools or a spell dealing
two blows of one each is **unmeasured**, and Flamefrost Spear — named in the
duo analysis with a `30/30` figure — reads either way. A set is safe against
both: it degrades to one element for every card we have, and it does not have
to be widened later against a dataset that has assumed otherwise.

A gate fires when the strike's set **contains** its tag. An empty set is
**Neutral**: magical damage of no school, which the page names rather than
leaves as absence. So `tags.is_empty()` is a state with a meaning, and a gate
on any school correctly stays shut for it.

**3. A tag belongs to a type, and the loader enforces it.**

Rust cannot stop `Blunt` reaching a magical strike, so the dataset loader
does: a physical strike or effect tagged with a school, or a magical one
tagged with a kind, is refused by name at load. That is where every other
nonsense value in this project is caught, and it is the same reason
`DamageKind::parse` refuses a kind nobody recognises — a tag that can never
match reads exactly like a perk that does nothing.

**4. All twelve are named now, and each is graded by what backs it.**

Four have cards behind them — Divine, Earth, Fire, Ice. The other eight come
from the closed list on a `documented` page. Naming all twelve costs one line
each and means the loader refuses a typo instead of accepting a gate that can
never fire; leaving eight out would mean refusing a school the game has.

They are not equally known and the dataset says so: a value tagged with a
school nobody has seen on a card is graded no better than the page it came
from.

**5. School-gated bonuses target Magic Power Bonus.**

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

**Projectile.** The same page lists Projectile as a damage category, and the
character sheet carries Projectile Damage Reduction. It is not a school — an
arrow is physical and pierces — but a second axis on the attack, in the way
Undead is a second axis on the target. Nothing in the dataset shoots anything
yet.

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

**A single tag rather than a set.** What the first draft of this ADR said,
before the source was read. The page's "one or more" is one sentence and it
is the difference between a field that can hold what the game has and one
that cannot.

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
