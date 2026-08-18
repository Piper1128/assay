//! Pure domain logic for Assay: stat newtypes, the resolution pipeline and the
//! exchange model (ADR-000 rev 2).
//!
//! `no_std + alloc`: the compiler forbids I/O, wall-clock time, threads and
//! non-deterministic collections in this crate. Everything the core needs from
//! the outside world enters through traits defined here and implemented in the
//! `std` crates. The core does not know that files exist.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod fixed;
pub mod stats;

pub use fixed::{Fixed, ParseFixedError, SCALE};
