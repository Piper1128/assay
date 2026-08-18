# Negative probes

ADR-010 rev 2 §4: **a gate that has never been seen failing is not a gate.**

`run.sh` injects each deliberately broken variant in this directory into the
workspace, asserts that the corresponding gate rejects it, and restores the
tree (via `git checkout`, so it refuses to run on a dirty tree). CI runs it as
the last step of the `gates` job.

| Probe | Breaks | Gate that must reject it |
|---|---|---|
| `no_std_violation` | `use std::fs;` in `assay-core` | bare-metal build (thumbv7em-none-eabi) |
| `hashmap_violation` | `HashMap` in `assay-diff` | clippy `disallowed_types` (clippy.toml) |
| `dep_direction` | `assay-scrape` referenced from `assay-data` | `tools/gates/dep_direction.sh` |
| `confidence_not_propagated` | `min` → `max` on the anchored line in `Confidence::zip_with` | ADR-007 propagation tests |
| `pipeline_order` | curves read the pre-party attribute sum (ADR-005 stages 3↔4 swapped) | pipeline tests (`party_buffs_shift_curve_inputs`) |

Source-mutating probes anchor on a `// probe: <name>` comment; the sed targets
only anchored lines. If a refactor loses the anchor, the mutation becomes a
no-op, the tests stay green, and the probe fails loudly — the safe direction.

Pending probes land with their subjects (ADR-010 rev 2 §4 table):
`pdr_mod_additive`, `true_damage_pre_reduction` and `scaling_ignored`
(with the exchange/damage model, ADR-006).
