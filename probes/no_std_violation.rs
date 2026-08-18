//! Deliberately broken (negative probe, ADR-010 rev 2 §4): `std` must be
//! unavailable in `assay-core` on the bare-metal target. If this file builds
//! for thumbv7em-none-eabi, the purity gate is dead.

use std::fs as _probe_fs;
