//! Diff engine for Assay (ADR-008).
//!
//! Two levels, one machine: the structural dataset diff (level 1) and the
//! loadout impact diff (level 2). The same engine also renders the scraper's
//! proposal diffs (ADR-003) — one piece of machinery, two uses. Operates only
//! on canonical forms defined by `assay-core` (ADR-001 rev 2 §3).
