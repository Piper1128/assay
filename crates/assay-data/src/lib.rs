//! Dataset I/O for Assay (ADR-003, ADR-004).
//!
//! Knows the filesystem; knows nothing about computation. Loads hand-approved,
//! versioned JSON datasets and hands owned structures into `assay-core`. The
//! trust boundary is one-way: this crate must never depend on `assay-scrape`
//! (enforced by the dependency-direction test in CI).
