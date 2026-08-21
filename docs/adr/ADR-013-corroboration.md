# ADR-013: corroboration

Status: Proposed
Relates to: ADR-003 (trust boundary), ADR-007 (grades and assumptions)
Date: 2026-08-21

## The defect

`Submission.observer` is required, validated, printed in the review header —
and never used as evidence. The doc comment on the field says why it is
there:

> A handle is enough; the point is that two submissions can be told apart,
> not that anyone is identified.

And `load` refuses an empty one with:

> a submission needs an observer: two people agreeing is only evidence if
> you can tell them apart

Both sentences describe a mechanism that does not exist. Nothing counts
observers, nothing notices that two people reported the same number, and a
value read independently by three people is graded exactly as one read by
one. That is this project's own recurring failure — a value that arrives,
lands somewhere, and does nothing — sitting inside the field whose entire
purpose is to prevent it.

It did not matter while there was one contributor. It is the difference
between a second contributor being an extra pair of hands and a second
contributor being a second witness.

## Decision

**1. Evidence goes in a ledger, beside the dataset rather than inside it.**

`data/<build>/attestations.json`, keyed by the field a value lives at:

```json
{
  "item.leather_cap": {
    "grants.derived.armor_rating": [
      { "observer": "piper",  "at": "2026-08-19", "method": "in-game" },
      { "observer": "friend", "at": "2026-08-21", "method": "screenshot-ocr" }
    ]
  }
}
```

Not inline in `items.json`. An item file is read by people deciding whether
a number looks right, and burying four lines of provenance under every value
would cost that far more than it buys.

**2. `assay submit --apply` appends an attestation for every field the
submission touched** — both the ones it added and the ones the dataset
already agreed with. Agreement is the interesting case: a submission that
changes nothing is currently reported as `already` and thrown away, and it
is exactly the corroboration this ADR is about.

**3. Two attestations count as two only if the observers differ and neither
method is `documented`.**

The independence rule is the whole design. A hundred people reading one wiki
page is one source, not a hundred, and a grade that rose because a number was
popular would be worse than no grade at all — it would launder a single
unverified claim into a verified one through sheer repetition.

`documented` therefore attests but never corroborates: it is recorded, and it
never counts toward a promotion.

**4. Promotion is proposed, never automatic.**

When the ledger newly supports a higher grade than a value carries, `assay
submit` says so and the pull request says so. A human edits the grade and
merges. ADR-003 does not move: the machine assembles the evidence, a person
decides what it is worth.

The rule it proposes against, and the only one:

> A value graded `unverified` whose ledger holds attestations from two
> distinct observers, neither of them `documented`, may be raised to
> `verified`.

**5. A given observer has at most one attestation per field.** A second
reading from the same person replaces their first rather than adding to it —
which is the mechanism the `observer` field was introduced for, finally
consuming it.

## Consequences

The ledger only ever grows, and only on agreement: a submission that
disagrees is already refused whole, so a contested value never reaches it.
That makes the ledger append-only in practice without needing to be so by
rule.

A value may sit at `unverified` with two attestations behind it for as long
as nobody merges the promotion. That is correct and will look like a bug.
The alternative is a number whose grade changed without a person looking at
it, which is the thing ADR-003 exists to prevent.

Existing values have no attestations and stay exactly as authored. The ledger
raises; it never contradicts. There is deliberately no gate demanding that a
grade be derivable from the ledger — such a gate would invalidate the whole
current dataset, which was authored by review rather than by submission, and
review is the authority here.

## Rejected

**A numeric confidence score.** Grades are a small ordered set on purpose,
and the minimum rule propagates the worst of them. A score invites averaging,
which is precisely the operation the minimum rule exists to forbid: two
values you half-know do not make one you know.

**Counting `documented` attestations.** See the independence rule. This is
the failure mode that would make the whole feature actively harmful.

**Promotion on merge, without a human.** That is not a smaller version of
ADR-003; it is the absence of it.

**Attestations inline in `items.json`.** Makes the file no longer readable by
the person whose reading is the actual gate.

**Trusting the timestamp.** `observed_at` is a string a person wrote and is
not parsed into a date. Ordering attestations by it, or expiring them, would
be building on a field that has never claimed to be reliable.
