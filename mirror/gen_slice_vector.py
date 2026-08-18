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

from assay_mirror import canonical_statblock, dataset_from_json, resolve

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


DATASET = {
    "classes": [
        {
            "id": "class.rogue",
            "name": "Rogue",
            "base_attributes": unverified(
                "points",
                {
                    "strength": 9,
                    "vigor": 6,
                    "agility": 25,
                    "dexterity": 20,
                    "will": 10,
                    "knowledge": 10,
                    "resourcefulness": 25,
                },
            ),
            "pdr_cap": unverified("micro", m("60")),
            "curves": {
                "strength_to_physical_power": "curve.slice.str_to_ppb",
                "agility_to_action_speed": "curve.slice.agi_to_as",
                "agility_to_move_speed": "curve.slice.agi_to_ms",
                "vigor_to_health": "curve.slice.vig_to_hp",
                "armor_to_pdr": "curve.slice.ar_to_pdr",
            },
        }
    ],
    "curves": [
        {
            "id": "curve.slice.str_to_ppb",
            "confidence": "unverified",
            "points": [[m("0"), m("-32")], [m("50"), m("68")]],
        },
        {
            "id": "curve.slice.agi_to_as",
            "confidence": "unverified",
            "points": [[m("0"), m("-17.1875")], [m("32"), m("14")]],
        },
        {
            "id": "curve.slice.agi_to_ms",
            "confidence": "unverified",
            "points": [[m("0"), m("281")], [m("25"), m("306")], [m("75"), m("331")]],
        },
        {
            "id": "curve.slice.vig_to_hp",
            "confidence": "unverified",
            "points": [[m("0"), m("90")], [m("6"), m("108.5")], [m("50"), m("220")]],
        },
        {
            "id": "curve.slice.ar_to_pdr",
            "confidence": "unverified",
            "points": [[m("0"), m("-22")], [m("100"), m("20")], [m("400"), m("83")]],
        },
    ],
    "items": [
        {
            "id": "item.dark_leather_leggings",
            "name": "Dark Leather Leggings",
            "armor_rating": unverified("micro", m("36")),
            "move_speed_add": unverified("micro", m("-4")),
        }
    ],
    "perks": [
        {
            "id": "perk.rogue.jokester",
            "name": "Jokester",
            "effects": [
                {"confidence": "unverified", "kind": "all_attributes", "points": 2}
            ],
        },
        {
            "id": "perk.fighter.defense_mastery",
            "name": "Defense Mastery",
            "effects": [
                {"confidence": "unverified", "kind": "raise_pdr_cap", "micro": m("75")}
            ],
        },
    ],
    "skills": [
        {
            "id": "skill.fighter.fortified_ground",
            "name": "Fortified Ground",
            "effects": [
                {"confidence": "unverified", "kind": "all_attributes", "points": 3}
            ],
        }
    ],
}

LOADOUTS = [
    {
        "name": "naked-rogue",
        "class": "class.rogue",
        "perks": [],
        "skills": [],
        "armor": [],
        "party": {"perks": [], "skills": []},
    },
    {
        # ADR-005 stage 3 coverage: Jokester (own) + Fortified Ground (party)
        # land on the attribute sum BEFORE the curves.
        "name": "rogue-duo-buffed",
        "class": "class.rogue",
        "perks": ["perk.rogue.jokester"],
        "skills": [],
        "armor": [],
        "party": {"perks": [], "skills": ["skill.fighter.fortified_ground"]},
    },
    {
        # Gear stages: attribute roll (2), flat move speed (5), armor→PDR (7),
        # plus a cap-raiser interacting with stage 7.
        "name": "rogue-geared",
        "class": "class.rogue",
        "perks": ["perk.fighter.defense_mastery"],
        "skills": [],
        "armor": [
            {
                "id": "item.dark_leather_leggings",
                "rolls": [
                    {"kind": "attribute", "attribute": "dexterity", "points": 4},
                    {"kind": "move_speed_add", "micro": m("2")},
                ],
            }
        ],
        "party": {"perks": [], "skills": []},
    },
]


def build_vector() -> str:
    indexed = dataset_from_json(DATASET)
    loadouts = []
    for loadout in LOADOUTS:
        resolved = resolve(indexed, loadout)
        loadouts.append({**loadout, "expected_canonical": canonical_statblock(resolved)})
    vector = {
        "description": (
            "Slice vector: pipeline mechanics over a placeholder dataset "
            "(unverified test curves). Expected values computed by the "
            "independent Python mirror; replayed by the Rust vector test."
        ),
        "dataset": DATASET,
        "loadouts": loadouts,
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
