//! Deliberately broken (negative probe, ADR-010 rev 2 §4): `HashMap` must be
//! rejected by clippy's `disallowed_types` (clippy.toml). Everything else in
//! this file is lint-clean on purpose, so the ONLY possible rejection is the
//! one the probe exists to prove.

/// Mentions the forbidden type; `disallowed_types` fires on the mention alone.
#[allow(dead_code)]
pub(crate) fn probe() -> std::collections::HashMap<u8, u8> {
    std::collections::HashMap::new()
}
