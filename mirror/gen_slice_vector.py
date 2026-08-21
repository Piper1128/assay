#!/usr/bin/env python3
"""Slice vector generator (ADR-010 rev 2 §3).

Builds fixtures/slice/duo_slice.json from the independent mirror: a
vector-local mini-dataset, three loadouts, and the expected canonical
statblock for each, computed by mirror code that shares nothing with the
Rust implementation. The Rust vector test replays the same file.

Run from the repo root:   python mirror/gen_slice_vector.py
Check mode (CI):          python mirror/gen_slice_vector.py --check

A vector that changes silently is a correctness regression nobody decided —
regenerate only together with a justified pipeline or grammar change, and
say so in the commit message.

Curve shapes in this dataset are TEST-AUTHORED PLACEHOLDERS graded
unverified: they exercise the pipeline mechanics. Real wiki-derived curves
arrive with the dataset arc and are verified against the in-game character
sheet — that is the golden-fixture step this vector deliberately is not.
"""

from __future__ import annotations

import json
import pathlib
import sys

from assay_mirror import (
    canonical_exchange,
    canonical_statblock,
    conf,
    dataset_from_json,
    exchange_damage,
    resolve,
)

SCALE = 1_000_000


def m(units_str: str) -> int:
    """Exact decimal → micro int, without floats: '7.8125' → 7812500."""
    negative = units_str.startswith("-")
    body = units_str.lstrip("+-")
    whole, _, frac = body.partition(".")
    assert len(frac) <= 6, units_str
    micro = int(whole or "0") * SCALE + int((frac or "0").ljust(6, "0"))
    return -micro if negative else micro


def unverified(value_key: str, value) -> dict:
    return {"confidence": "unverified", value_key: value}


DATASET_BUILD = "0.17.150.9384"


def load_dataset(build: str = DATASET_BUILD) -> dict:
    """Reads the committed dataset (ADR-003: the hand-approved JSON in the
    repo is the only source of truth). The vector embeds a copy of what it
    loaded, so the fixture stays self-contained while the dataset directory
    remains the thing that is maintained."""
    root = pathlib.Path(__file__).resolve().parent.parent / "data" / build
    parts = {}
    for name, key in [
        ("classes.json", "classes"),
        ("curves.json", "curves"),
        ("items.json", "items"),
        ("perks.json", "perks"),
        ("skills.json", "skills"),
    ]:
        parts[key] = json.loads((root / name).read_text(encoding="utf-8"))[key]
    parts["manifest"] = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    return parts


DATASET = load_dataset()

LOADOUTS = [
    {
        "name": "naked-rogue",
        "class": "class.rogue",
        "perks": [],
        "skills": [],
        "gear": [],
        "party": {"perks": [], "skills": []},
    },
    {
        # The gated perk (ADR-006 damage-kind amendment). Blunt Weapon
        # Mastery's bonus must stay OFF this sheet — it depends on the swing,
        # not the character — so this loadout pins the divert, and the
        # exchange below pins the apply.
        "name": "cleric-mastery",
        "class": "class.cleric",
        "perks": ["perk.cleric.blunt_weapon_mastery"],
        "skills": [],
        "gear": [],
        "party": {"perks": [], "skills": []},
    },
    {
        # ADR-005 stage 3 coverage: Jokester (own) + Fortified Ground (party)
        # land on the attribute sum BEFORE the curves.
        "name": "rogue-duo-buffed",
        "class": "class.rogue",
        "perks": ["perk.rogue.jokester"],
        "skills": [],
        "gear": [],
        # The party also holds Jokester. It must change nothing: an ability
        # applies once however many people bring it, so this loadout's
        # expected values are identical to the same one without it.
        "party": {
            "perks": ["perk.rogue.jokester"],
            "skills": ["skill.fighter.fortified_ground"],
        },
    },
    {
        # Gear stages: attribute roll (2), flat move speed (5), armor→PDR (7),
        # plus a cap-raiser interacting with stage 7. Defense Mastery also
        # carries a 15% Item Armor Rating Bonus, and the armor_rating roll is
        # the enchantment that bonus must leave alone (ADR-005 amendment) —
        # so this one loadout pins both halves of the split.
        "name": "rogue-geared",
        "class": "class.rogue",
        "perks": ["perk.fighter.defense_mastery"],
        "skills": [],
        "gear": [
            {
                "slot": "legs",
                "id": "item.dark_leather_leggings",
                "rolls": [
                    {"kind": "attribute", "attribute": "dexterity", "points": 4},
                    {"kind": "move_speed_add", "micro": m("2")},
                    {"kind": "derived", "stat": "derived.armor_rating", "micro": m("10")},
                ],
            }
        ],
        "party": {"perks": [], "skills": []},
    },
    {
        # Three item cards read in game, in three slots. Covers everything the
        # gear amendment added at once: static attributes (Vigor 2 on the cap,
        # Agility 4 on the trousers), a derived roll that must not be
        # multiplied (+11 Additional Armor Rating), and a seed that has to be
        # ADDED to what the class computes rather than replacing it (+9 Magic
        # Resistance on top of the 15 Will produces).
        "name": "rogue-kitted",
        "class": "class.rogue",
        "perks": [],
        "skills": [],
        "gear": [
            {
                "slot": "head",
                "id": "item.leather_cap",
                "rolls": [
                    {"kind": "derived", "stat": "derived.armor_rating", "micro": m("11")}
                ],
            },
            {
                "slot": "legs",
                "id": "item.loose_trousers",
                "rolls": [
                    {"kind": "attribute", "attribute": "strength", "points": 2},
                    {
                        "kind": "derived",
                        "stat": "derived.magic_resistance",
                        "micro": m("9"),
                    },
                ],
            },
            {"slot": "necklace", "id": "item.phoenix_choker", "rolls": []},
        ],
        "party": {"perks": [], "skills": []},
    },
]


def graded_micro(value_str: str, confidence: str = "verified") -> dict:
    return {"confidence": confidence, "micro": m(value_str)}


# Exchange cases (ADR-006). Attacker and defender are named by loadout, so
# the exchange rides on the SAME resolution the statblock cases lock.
EXCHANGES = [
    {
        "name": "plain-swing",
        "attacker": "naked-rogue",
        "defender": "naked-rogue",
        "strike": {
            "base": graded_micro("20"),
            "scaling": graded_micro("100"),
            "flat_bonus": graded_micro("0"),
            "armor_pen": graded_micro("0"),
            "true_damage": graded_micro("0"),
        },
        "context": {
            "power_bonus_adjust": graded_micro("0"),
            "pdr_mod": graded_micro("0"),
            "hit_location_bonus": graded_micro("0"),
        },
    },
    {
        # A blunt swing by the holder of a blunt-gated perk: the bonus is
        # absent from the sheet and present in the damage, which is the whole
        # of the damage-kind amendment in one case.
        "name": "gated-blunt-swing",
        "attacker": "cleric-mastery",
        "defender": "naked-rogue",
        "strike": {
            "base": graded_micro("20"),
            "scaling": graded_micro("100"),
            "flat_bonus": graded_micro("0"),
            "armor_pen": graded_micro("0"),
            "true_damage": graded_micro("0"),
            "kind": "blunt",
        },
        "context": {
            "power_bonus_adjust": graded_micro("0"),
            "pdr_mod": graded_micro("0"),
            "hit_location_bonus": graded_micro("0"),
        },
    },
    {
        # The same swing by the same character, with a sword. The gate must
        # stay shut, and the two damages must differ — a pair, because either
        # one alone would pass with the gate wired to nothing.
        "name": "gated-slash-swing",
        "attacker": "cleric-mastery",
        "defender": "naked-rogue",
        "strike": {
            "base": graded_micro("20"),
            "scaling": graded_micro("100"),
            "flat_bonus": graded_micro("0"),
            "armor_pen": graded_micro("0"),
            "true_damage": graded_micro("0"),
            "kind": "slash",
        },
        "context": {
            "power_bonus_adjust": graded_micro("0"),
            "pdr_mod": graded_micro("0"),
            "hit_location_bonus": graded_micro("0"),
        },
    },
    {
        # THE case: Sneak Attack from Hide. 0% scaling means the -30%
        # Hide-exit power penalty cannot touch it (duo analysis §5).
        "name": "sneak-attack-from-hide",
        "attacker": "naked-rogue",
        "defender": "rogue-geared",
        "strike": {
            "base": graded_micro("10"),
            "scaling": graded_micro("0"),
            "flat_bonus": graded_micro("15"),
            "armor_pen": graded_micro("0"),
            "true_damage": graded_micro("1"),
        },
        "context": {
            "power_bonus_adjust": graded_micro("-30"),
            "pdr_mod": graded_micro("0"),
            "hit_location_bonus": graded_micro("0"),
        },
    },
    {
        # Weakpoint Attack: the attacker imposes -30% Item Armor Rating Bonus
        # on the defender. rogue-geared wears 36 of item armour with 10
        # enchanted onto it, so the debuff must take 36 to 25.2 and leave the
        # 10 alone -- the whole point of carrying the composition forward.
        "name": "weakpoint-debuff",
        "attacker": "naked-rogue",
        "defender": "rogue-geared",
        "strike": {
            "base": graded_micro("20"),
            "scaling": graded_micro("100"),
            "flat_bonus": graded_micro("0"),
            "armor_pen": graded_micro("0"),
            "true_damage": graded_micro("0"),
        },
        "context": {
            "power_bonus_adjust": graded_micro("0"),
            "pdr_mod": graded_micro("0"),
            "hit_location_bonus": graded_micro("0"),
            "item_armor_bonus_mods": {
                "skill.rogue.weakpoint_attack": graded_micro("-30"),
            },
        },
    },
    {
        # A magic attack. The type picks Magic Power Bonus at step 3 and the
        # magic resistance chain at 5 through 7, and changes neither the
        # order of the steps nor their number.
        "name": "magic-bolt",
        "attacker": "rogue-duo-buffed",
        "defender": "rogue-geared",
        "strike": {
            "type": "magic",
            "base": graded_micro("20"),
            "scaling": graded_micro("100"),
            "flat_bonus": graded_micro("0"),
            "armor_pen": graded_micro("0"),
            "true_damage": graded_micro("0"),
        },
        "context": {
            "power_bonus_adjust": graded_micro("0"),
            "pdr_mod": graded_micro("0"),
            "hit_location_bonus": graded_micro("0"),
        },
    },
    {
        # Lethal Mark: -30 PDR Mod, multiplicative on the defender's PDR,
        # plus Thrust penetration and a back attack.
        "name": "lethal-mark-back-attack",
        "attacker": "rogue-duo-buffed",
        "defender": "rogue-geared",
        "strike": {
            "base": graded_micro("18"),
            "scaling": graded_micro("100"),
            "flat_bonus": graded_micro("2"),
            "armor_pen": graded_micro("15"),
            "true_damage": graded_micro("1"),
        },
        "context": {
            "power_bonus_adjust": graded_micro("30"),
            "pdr_mod": graded_micro("-30"),
            "hit_location_bonus": graded_micro("2"),
        },
    },
]


def build_vector() -> str:
    indexed = dataset_from_json(DATASET)
    loadouts = []
    resolved_by_name = {}
    for loadout in LOADOUTS:
        resolved = resolve(indexed, loadout)
        resolved_by_name[loadout["name"]] = resolved
        loadouts.append({**loadout, "expected_canonical": canonical_statblock(resolved)})

    def graded_from(node: dict) -> dict:
        return conf(node["confidence"], node["micro"], node.get("note"))

    exchanges = []
    for case in EXCHANGES:
        # `type` and `kind` are names, not graded numbers: the damage type
        # chooses which stats the steps read, and the kind is what a perk
        # gates on. Everything else in a strike is a value with a grade.
        strike = {
            k: (v if k in ("type", "kind") else graded_from(v))
            for k, v in case["strike"].items()
        }
        context = {
            k: (
                {inner: graded_from(g) for inner, g in v.items()}
                if k == "item_armor_bonus_mods"
                else graded_from(v)
            )
            for k, v in case["context"].items()
        }
        outcome = exchange_damage(
            indexed,
            resolved_by_name[case["attacker"]],
            resolved_by_name[case["defender"]],
            strike,
            context,
        )
        exchanges.append({**case, "expected_canonical": canonical_exchange(outcome)})

    vector = {
        "build": DATASET["manifest"]["build"],
        "description": (
            "Slice vector over the committed dataset for the build named "
            "above. Expected values computed by the independent Python "
            "mirror; replayed byte-for-byte by the Rust vector test."
        ),
        "dataset": {k: v for k, v in DATASET.items() if k != "manifest"},
        "loadouts": loadouts,
        "exchanges": exchanges,
    }
    return json.dumps(vector, indent=2, ensure_ascii=False) + "\n"


def main() -> int:
    out_path = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "slice" / "duo_slice.json"
    content = build_vector()
    if "--check" in sys.argv[1:]:
        if not out_path.exists():
            print(f"MIRROR CHECK: {out_path} missing — run: python mirror/gen_slice_vector.py")
            return 1
        committed = out_path.read_text(encoding="utf-8")
        if committed != content:
            print("MIRROR CHECK: committed vector differs from mirror output.")
            print("Either the mirror or the vector changed without the other; regenerate")
            print("deliberately with: python mirror/gen_slice_vector.py — and justify it.")
            return 1
        print("MIRROR CHECK: ok")
        return 0
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(content, encoding="utf-8", newline="\n")
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
