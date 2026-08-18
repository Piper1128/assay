//! Pure domain logic for Assay: stat newtypes, the resolution pipeline and the
//! exchange model (ADR-000 rev 2).
//!
//! `no_std + alloc`: the compiler forbids I/O, wall-clock time, threads and
//! non-deterministic collections in this crate. Everything the core needs from
//! the outside world enters through traits defined here and implemented in the
//! `std` crates. The core does not know that files exist.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod confidence;
pub mod curve;
pub mod fixed;
pub mod ids;
pub mod loadout;
pub mod resolve;
pub mod schema;
pub mod stats;

pub use confidence::{Confidence, ConfidenceLevel};
pub use curve::{Curve, CurveError, Interpolation};
pub use fixed::{Fixed, ParseFixedError, SCALE};
pub use ids::{ClassId, CurveId, ItemId, PerkId, SkillId};
pub use loadout::{ArmorPiece, Loadout, PartyBuffs, Roll};
pub use resolve::{ResolveError, Resolved, StageNote, resolve};
pub use schema::{
    AttributeBlock, AttributeKind, ClassDef, DatasetSource, DerivedCurves, Effect, InMemoryDataset,
    ItemDef, PerkDef, SkillDef,
};
