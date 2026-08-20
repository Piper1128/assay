"""Independent Python mirror of Assay's resolution pipeline (ADR-010 rev 2).

Written from the ADRs in docs/adr/ — never from the Rust code. A mirror
derived from the implementation inherits its misunderstandings and is
worthless. If Rust and mirror disagree, both are suspect until the ADR
decides who is right; if the ADR is ambiguous, the ADR is the bug.

Mirrors:
- fixed-point i64 micro-units with banker's rounding   (ADR-001 rev 2)
- linear curve sampling, one rounding per sample        (ADR-004)
- confidence with minimum-rule propagation              (ADR-007)
- the locked resolution stage order 1-8                 (ADR-005)
- the locked exchange step order 1-9                    (ADR-006)
- the canonical statblock grammar                       (ADR-001 rev 2 §3)

Only pure ints — a float anywhere in this file is a bug.
"""

from __future__ import annotations

SCALE = 1_000_000

# ── fixed-point (ADR-001 rev 2) ──────────────────────────────────────────────


def div_round_half_even(n: int, d: int) -> int:
    """n / d rounded half to even. d must be positive; callers normalise sign.

    Python's divmod is floor division with a remainder in 0..d for d > 0,
    which matches Rust's div_euclid/rem_euclid — the tie test is sign-free.
    """
    assert d > 0
    q, r = divmod(n, d)
    twice = 2 * r
    if twice > d or (twice == d and q % 2 != 0):
        q += 1
    return q


def mul_div(a: int, num: int, den: int) -> int:
    """a * num / den in micro-units, one banker's rounding at the division."""
    assert den != 0
    prod = a * num
    if den < 0:
        prod, den = -prod, -den
    return div_round_half_even(prod, den)


def fx_mul(a: int, b: int) -> int:
    """Fixed multiplication: full product, one rounding back to micro."""
    return mul_div(a, b, SCALE)


# ── curves (ADR-004) ─────────────────────────────────────────────────────────


def curve_sample(points: list[tuple[int, int]], x: int) -> int:
    """Linear sample: clamped outside the range, exact at points, one
    rounding between neighbours: y = y0 + (y1-y0)*(x-x0)/(x1-x0)."""
    assert points, "curve has no points"
    for (a, _), (b, _) in zip(points, points[1:]):
        assert b > a, "curve inputs must be strictly ascending"
    if x <= points[0][0]:
        return points[0][1]
    if x >= points[-1][0]:
        return points[-1][1]
    hi = next(i for i, p in enumerate(points) if p[0] > x)
    x0, y0 = points[hi - 1]
    x1, y1 = points[hi]
    if x == x0:
        return y0
    return y0 + mul_div(y1 - y0, x - x0, x1 - x0)


# ── confidence (ADR-007) ─────────────────────────────────────────────────────

_RANK = {"unknown": 0, "unverified": 1, "verified": 2}


def conf(level: str, value, note: str | None = None) -> dict:
    """A graded value. note exists exactly when level is unknown."""
    assert level in _RANK
    assert (note is not None) == (level == "unknown")
    return {"level": level, "value": value, "note": note}


def zip_with(a: dict, b: dict, f) -> dict:
    """Minimum-rule combination: the result's grade is the minimum of the
    inputs' grades; notes from unknown inputs are joined with '; '."""
    level = a["level"] if _RANK[a["level"]] <= _RANK[b["level"]] else b["level"]
    notes = [c["note"] for c in (a, b) if c["note"] is not None]
    note = "; ".join(notes) if level == "unknown" else None
    return conf(level, f(a["value"], b["value"]), note)


def map_conf(c: dict, f) -> dict:
    """Transforms the value; grade and note travel unchanged (the Rust
    Confidence::map — NOT zip_with(c, c, …), which would join the note with
    itself)."""
    return conf(c["level"], f(c["value"]), c["note"])


def fold_sum(values: list[dict]) -> dict:
    """Sums graded micro values. The empty sum is Verified(0): the absence
    of modifiers is a certain fact, not a guess."""
    acc = conf("verified", 0)
    for v in values:
        acc = zip_with(acc, v, lambda x, y: x + y)
    return acc


# ── resolution pipeline (ADR-005, locked stage order) ────────────────────────

ATTRIBUTES = ["strength", "vigor", "agility", "dexterity", "will", "knowledge", "resourcefulness"]


def _scaled(payload: dict, stacks: int) -> dict:
    """An effect payload at `stacks` stacks. A stacking effect's value is per
    stack, so the magnitude multiplies; a raised ceiling does not stack."""
    kind = payload["kind"]
    if kind in ("all_attributes", "attribute"):
        return {**payload, "points": payload["points"] * stacks}
    if kind in ("move_speed_add", "move_speed_bonus"):
        return {**payload, "micro": fx_mul(payload["micro"], stacks * SCALE)}
    return dict(payload)


def _apply_stacks(entry: dict, source_id: str, source_name: str, stacks: dict) -> dict:
    """Resolves one dataset effect against the loadout's stack counts.

    Stating no count for a stacking effect resolves it at the maximum and
    downgrades it to unknown, so the assumption travels with the number
    (ADR-007).
    """
    max_stacks = entry["value"].get("max_stacks")
    if max_stacks is None:
        return entry
    requested = stacks.get(source_id)
    if requested is not None:
        if requested > max_stacks:
            raise ValueError(f"{source_id} stacks at most {max_stacks}, not {requested}")
        return map_conf(entry, lambda payload: _scaled(payload, requested))
    note = (
        f"{source_name} resolved at {max_stacks} of {max_stacks} stacks; "
        "the loadout does not say how many are active"
    )
    existing = entry["note"]
    return conf(
        "unknown",
        _scaled(entry["value"], max_stacks),
        f"{existing}; {note}" if existing else note,
    )


def _effects(dataset: dict, loadout: dict) -> list[dict]:
    """Own perks, own skills, party perks, party skills — in that order, each
    list in loadout declaration order, each already scaled to its stacks.

    An ability applies once however many people bring it: two Jokesters in a
    party are +2 All Attributes, not +4. Order gives precedence, so a copy you
    hold yourself beats a teammate's. This is a different question from
    `max_stacks`, which is one ability applied repeatedly by its owner.
    """
    out: list[dict] = []
    stacks = loadout.get("stacks", {})
    seen: set[str] = set()

    def add(table: str, ids: list[str], party: bool) -> None:
        for entity_id in ids:
            if entity_id in seen:
                continue
            seen.add(entity_id)
            entity = dataset[table][entity_id]
            name = f"{entity['name']} (party)" if party else entity["name"]
            for entry in entity["effects"]:
                out.append(_apply_stacks(entry, entity_id, name, stacks))

    add("perks", loadout["perks"], False)
    add("skills", loadout["skills"], False)
    add("perks", loadout["party"]["perks"], True)
    add("skills", loadout["party"]["skills"], True)
    return out


def evaluate_derived(defs: list[dict], attributes: dict, seeded: dict, cap_overrides: dict) -> dict:
    """Derived stats as weighted ratings (ADR-012), in dependency order.

    rating   = sum(weight * input)   each product rounds half to even
    derived  = curve(rating) + offset, then clamped by floor/cap.

    A definition whose id is already seeded is skipped: a seeded value is
    provided (gear-sourced armour rating), not computed. Cycles and dangling
    references are dataset errors.
    """
    computed = dict(seeded)
    defined = {d["id"] for d in defs}
    pending = [d for d in defs if d["id"] not in computed]

    while pending:
        progressed = False
        still = []
        for d in pending:
            ready = True
            for w in d["weights"]:
                if w["kind"] == "derived" and w["ref"] not in computed:
                    if w["ref"] not in defined:
                        raise ValueError(f"{d['id']} references undefined input {w['ref']}")
                    ready = False
            if not ready:
                still.append(d)
                continue

            rating = conf("verified", 0)
            for w in d["weights"]:
                if w["kind"] == "attribute":
                    term = map_conf(
                        attributes, lambda block, k=w["ref"], wt=w["weight"]: fx_mul(block[k] * SCALE, wt)
                    )
                else:
                    term = map_conf(computed[w["ref"]], lambda v, wt=w["weight"]: fx_mul(v, wt))
                rating = zip_with(rating, term, lambda a, b: a + b)

            cap = cap_overrides.get(d["id"], d.get("cap"))
            floor = d.get("floor")
            offset = d.get("offset", 0)

            def apply(rating_value, points, off=offset, fl=floor, cp=cap):
                value = curve_sample(points, rating_value) + off
                if fl is not None:
                    value = max(value, fl)
                if cp is not None:
                    value = min(value, cp)
                return value

            computed[d["id"]] = zip_with(rating, d["curve"], apply)
            progressed = True
        if not progressed:
            raise ValueError(f"cyclic derived-stat dependency among {[d['id'] for d in still]}")
        pending = still
    return computed


def resolve(dataset: dict, loadout: dict) -> dict:
    """Resolves a loadout; returns the derived stats plus the attribute block.

    Stage order is the ADR-005 lock: the attribute sum is final after stage 3,
    strictly before any rating is formed in stage 4a.
    """
    cls = dataset["classes"][loadout["class"]]

    # Stage 1: class base attributes.
    attributes = {
        "level": cls["base_attributes"]["level"],
        "value": dict(cls["base_attributes"]["value"]),
        "note": cls["base_attributes"]["note"],
    }

    # Stage 2: attributes from gear rolls (facts of the question: Verified,
    # so they do not degrade the block's grade).
    for piece in loadout["armor"]:
        for roll in piece["rolls"]:
            if roll["kind"] == "attribute":
                attributes["value"][roll["attribute"]] += roll["points"]

    # Stage 3: attributes from perks/skills/party. FINAL after this loop.
    effects = _effects(dataset, loadout)
    for effect in effects:
        payload = effect["value"]
        if payload["kind"] == "all_attributes":
            attributes = zip_with(
                attributes,
                effect,
                lambda block, _p, pts=payload["points"]: {k: v + pts for k, v in block.items()},
            )
        elif payload["kind"] == "attribute":
            attributes = zip_with(
                attributes,
                effect,
                lambda block, _p, kk=payload["attribute"], pts=payload["points"]: {
                    **block,
                    kk: block[kk] + pts,
                },
            )

    # Stage 7 (prepared): gear-sourced armour rating seeds the graph, and
    # cap raises are collected before evaluation.
    #
    # ADR-005 amendment: an Item Armor Rating Bonus multiplies the armour the
    # pieces themselves carry, and nothing else. Enchantment rolls are the
    # formula's "other Armor Rating" term and stay outside the multiply, so
    # the two sums are kept apart until they are combined below.
    item_ar_parts = [
        dataset["items"][piece["id"]]["armor_rating"]
        for piece in loadout["armor"]
        if dataset["items"][piece["id"]]["armor_rating"] is not None
    ]
    other_ar_parts = [
        conf("verified", roll["micro"])
        for piece in loadout["armor"]
        for roll in piece["rolls"]
        if roll["kind"] == "armor_rating"
    ]
    item_bonus = fold_sum(
        [
            {**e, "value": e["value"]["micro"]}
            for e in effects
            if e["value"]["kind"] == "item_armor_bonus"
        ]
    )
    scaled_item_ar = zip_with(
        fold_sum(item_ar_parts),
        item_bonus,
        lambda ar, bonus: div_round_half_even(ar * (100_000_000 + bonus), 100_000_000),
    )
    armor_rating = zip_with(scaled_item_ar, fold_sum(other_ar_parts), lambda a, b: a + b)
    seeded = {"derived.armor_rating": armor_rating}
    cap_overrides: dict = {}
    for effect in effects:
        if effect["value"]["kind"] == "raise_cap":
            target = effect["value"]["target"]
            raised = effect["value"]["micro"]
            cap_overrides[target] = max(cap_overrides.get(target, raised), raised)

    # Stage 4a/4b: attributes -> ratings -> derived stats.
    derived = evaluate_derived(cls["derived"], attributes, seeded, cap_overrides)

    # Stage 5: flat adds, on the move-speed entry.
    adds: list[dict] = []
    for piece in loadout["armor"]:
        item = dataset["items"][piece["id"]]
        if item["move_speed_add"] is not None:
            adds.append(item["move_speed_add"])
        for roll in piece["rolls"]:
            if roll["kind"] == "move_speed_add":
                adds.append(conf("verified", roll["micro"]))
    for effect in effects:
        if effect["value"]["kind"] == "move_speed_add":
            adds.append(map_conf(effect, lambda p: p["micro"]))
    if "derived.move_speed" in derived:
        derived["derived.move_speed"] = zip_with(
            derived["derived.move_speed"], fold_sum(adds), lambda ms, add: ms + add
        )

    # Stage 6: percentage bonuses: ms * (100 + sum) / 100, one rounding.
    bonuses: list[dict] = []
    for effect in effects:
        if effect["value"]["kind"] == "move_speed_bonus":
            bonuses.append(map_conf(effect, lambda p: p["micro"]))
    hundred = 100 * SCALE
    if "derived.move_speed" in derived:
        derived["derived.move_speed"] = zip_with(
            derived["derived.move_speed"],
            fold_sum(bonuses),
            lambda ms, bonus: mul_div(ms, hundred + bonus, hundred),
        )

    # Stage 8: situational mods stay separate (exchange layer, ADR-006).

    # The cap in force per capped stat, after any perk raised it. An exchange
    # re-evaluating a stat at another input must clamp the same way, and the
    # raise came from the loadout rather than the dataset (ADR-006 amendment).
    caps = {}
    for d in cls["derived"]:
        cap = cap_overrides.get(d["id"], d.get("cap"))
        if cap is not None:
            caps[d["id"]] = cap

    return {
        "class": loadout["class"],
        "attributes": attributes,
        "derived": derived,
        "caps": caps,
    }


# ── exchange / damage model (ADR-006) ────────────────────────────────────────

PERCENT = 100 * SCALE


def apply_percent(damage: int, percent: int) -> int:
    """damage * (100 + percent) / 100, one rounding."""
    return mul_div(damage, PERCENT + percent, PERCENT)


def _pdr_at(dataset: dict, defender: dict, armor: int) -> dict:
    """The defender's PDR at an arbitrary armour rating: the real curve,
    re-sampled, offset and clamped exactly as resolution would have
    (ADR-006 amendment: penetration re-sampling)."""
    cls = dataset["classes"][defender["class"]]
    d = next(x for x in cls["derived"] if x["id"] == "derived.pdr")
    cap = defender["caps"].get("derived.pdr", d.get("cap"))
    floor = d.get("floor")
    offset = d.get("offset", 0)

    def apply(points):
        value = curve_sample(points, armor) + offset
        if floor is not None:
            value = max(value, floor)
        if cap is not None:
            value = min(value, cap)
        return value

    return map_conf(d["curve"], apply)


def exchange_damage(
    dataset: dict, attacker: dict, defender: dict, strike: dict, context: dict
) -> dict:
    """The nine locked steps. Returns {'damage': graded, 'effective_pdr': graded}.

    Takes the dataset because step 6 re-samples the defender's PDR curve at
    the penetrated armour rating. ADR-006's original three-input purity was
    relaxed for exactly that reason; see
    docs/adr/ADR-006-amendment-penetration-resampling.md.

    Steps 6-7 follow ADR-002's locked apply_pdr_mod(PdrPercent, PdrMod) ->
    EffectivePdr: the mod modifies the PDR, which then reduces damage once.
    Rust and mirror share that reading, so the vector does not settle it -
    the ADR does.
    """
    # 1: base damage.
    damage = strike["base"]

    # 2: × scaling coefficient. 0% is load-bearing (Sneak Attack).
    damage = zip_with(damage, strike["scaling"], lambda d, c: mul_div(d, c, PERCENT))

    # 3: + Physical Power Bonus, with the situational adjustment.
    power = zip_with(
        attacker["derived"]["derived.physical_power_bonus"],
        context["power_bonus_adjust"],
        lambda p, a: p + a,
    )
    damage = zip_with(damage, power, apply_percent)

    # 4: + flat Buff Weapon Damage.
    damage = zip_with(damage, strike["flat_bonus"], lambda d, f: d + f)

    # 5: defender's armor rating, reduced by penetration (floored at zero).
    armor = zip_with(
        defender["derived"]["derived.armor_rating"],
        strike["armor_pen"],
        lambda ar, pen: max(mul_div(ar, PERCENT - pen, PERCENT), 0),
    )

    # 6: PDR re-sampled from the curve at the penetrated rating. Rescaling
    # the resolved PDR instead is wrong in direction whenever PDR is
    # negative, which is the defect the amendment fixed.
    pdr = zip_with(armor, _pdr_at(dataset, defender, armor["value"]), lambda _a, p: p)

    # 7: × PDR Mod, multiplicative on the PDR; then reduce the damage once.
    effective_pdr = zip_with(pdr, context["pdr_mod"], lambda p, m: mul_div(p, PERCENT + m, PERCENT))
    damage = zip_with(damage, effective_pdr, lambda d, p: mul_div(d, PERCENT - p, PERCENT))

    # 8: + True Damage, AFTER reduction — it bypasses armor by definition.
    damage = zip_with(damage, strike["true_damage"], lambda d, t: d + t)

    # 9: × hit location multiplier.
    damage = zip_with(damage, context["hit_location_bonus"], apply_percent)

    return {"damage": damage, "effective_pdr": effective_pdr}


def canonical_exchange(outcome: dict) -> str:
    """Canonical form of an exchange outcome — same grammar as the statblock:
    no whitespace, lexicographic keys, integers only."""
    return (
        "{"
        + _graded_fixed("damage", outcome["damage"])
        + ","
        + _graded_fixed("effective_pdr", outcome["effective_pdr"])
        + "}"
    )


# ── canonical encoding (ADR-001 rev 2 §3) ────────────────────────────────────


def _js(s: str) -> str:
    """Closed string escaping: quote, backslash, control chars < 0x20."""
    out = ['"']
    for ch in s:
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ord(ch) < 0x20:
            out.append(f"\\u{ord(ch):04x}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def _graded_fixed(key: str, c: dict) -> str:
    parts = [f'{_js(key)}:{{"confidence":{_js(c["level"])},"micro":{c["value"]}']
    if c["note"] is not None:
        parts.append(f',"note":{_js(c["note"])}')
    parts.append("}")
    return "".join(parts)


def _graded_attributes(key: str, c: dict) -> str:
    parts = [f'{_js(key)}:{{"confidence":{_js(c["level"])}']
    if c["note"] is not None:
        parts.append(f',"note":{_js(c["note"])}')
    inner = ",".join(f"{_js(k)}:{c['value'][k]}" for k in sorted(c["value"]))
    parts.append(f',"points":{{{inner}}}}}')
    return "".join(parts)


def canonical_statblock(resolved: dict) -> str:
    """The canonical form: no whitespace, lexicographic keys, integers only,
    absence distinct from null, note exactly for unknown. Derived stats are
    emitted by id in sorted order (ADR-012); "attributes" sorts first."""
    parts = [_graded_attributes("attributes", resolved["attributes"])]
    for key in sorted(resolved["derived"]):
        parts.append(_graded_fixed(key, resolved["derived"][key]))
    return "{" + ",".join(parts) + "}"


# ── vector-JSON adapters ─────────────────────────────────────────────────────


def graded_from_json(node: dict, value_key: str) -> dict:
    """Reads {"confidence": …, <value_key>: …[, "note": …]} into a graded value."""
    return conf(node["confidence"], node[value_key], node.get("note"))


def dataset_from_json(node: dict) -> dict:
    """Indexes a vector-file dataset by id for resolution."""
    curves = {}
    for cu in node["curves"]:
        curves[cu["id"]] = conf(
            cu["confidence"], [tuple(p) for p in cu["points"]], cu.get("note")
        )

    classes = {}
    for c in node["classes"]:
        derived = []
        for d in c["derived"]:
            derived.append(
                {
                    "id": d["id"],
                    "weights": d["weights"],
                    "curve": curves[d["curve"]],
                    "offset": d.get("offset", 0),
                    "floor": d.get("floor"),
                    "cap": d.get("cap"),
                }
            )
        classes[c["id"]] = {
            "name": c["name"],
            "base_attributes": graded_from_json(c["base_attributes"], "points"),
            "derived": derived,
        }

    items = {}
    for it in node["items"]:
        items[it["id"]] = {
            "name": it["name"],
            "armor_rating": (
                graded_from_json(it["armor_rating"], "micro")
                if it.get("armor_rating") is not None
                else None
            ),
            "move_speed_add": (
                graded_from_json(it["move_speed_add"], "micro")
                if it.get("move_speed_add") is not None
                else None
            ),
        }

    def effect_list(defs: list[dict]) -> dict:
        table = {}
        for d in defs:
            table[d["id"]] = {
                "name": d["name"],
                "effects": [
                    conf(
                        e["confidence"],
                        {k: v for k, v in e.items() if k != "confidence"},
                        e.get("note"),
                    )
                    for e in d["effects"]
                ],
            }
        return table

    return {
        "classes": classes,
        "curves": curves,
        "items": items,
        "perks": effect_list(node["perks"]),
        "skills": effect_list(node["skills"]),
    }
