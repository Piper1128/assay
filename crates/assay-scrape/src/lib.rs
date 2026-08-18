//! Scraper for Assay (ADR-003).
//!
//! Outside the trust boundary. Produces a proposal diff against the committed
//! dataset — never a dataset, never an overwrite. Everything `assay-core`
//! ever sees has passed a human. Rate-limited, respects robots.txt, and the
//! project must stay fully functional with this crate dead.
