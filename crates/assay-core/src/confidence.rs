//! Confidence as a first-class part of the domain model (ADR-007).
//!
//! The source data is incomplete and the wiki marks fields as unverified. An
//! analysis tool that cannot tell "this number is right" from "this number is
//! a guess" is worse than no tool, because it makes the user act on guesses
//! with confidence. So every value carries its grade, and the grade
//! propagates: the result of a computation is at most as trustworthy as its
//! least trustworthy input (minimum rule).
//!
//! Deviation from the ADR sketch, flagged: the sketch spells the `Unknown`
//! note as `&'static str`, but notes are set under dataset review (ADR-003)
//! and therefore arrive as *data*, not as literals baked into the binary.
//! The note is an owned `String`.

use alloc::string::String;

/// How trustworthy a value is. Ordered so that `min` implements the ADR-007
/// propagation rule: `Unknown < Unverified < Verified`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ConfidenceLevel {
    /// An assumption with a stated reason. Lowest.
    Unknown,
    /// Wiki/community sourced, not confirmed.
    Unverified,
    /// Confirmed in patch notes or by our own in-game test. Highest.
    Verified,
}

/// A value together with how much it can be trusted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Confidence<T> {
    /// Confirmed in patch notes or by our own in-game test.
    Verified(T),
    /// Wiki/community sourced, not confirmed.
    Unverified(T),
    /// An assumption; the note says what was assumed and why.
    Unknown {
        /// The value the computation proceeds with.
        assumed: T,
        /// Why this is a guess — surfaced in output, never dropped.
        note: String,
    },
}

impl<T> Confidence<T> {
    /// The trust grade of this value.
    #[must_use]
    pub fn level(&self) -> ConfidenceLevel {
        match self {
            Confidence::Verified(_) => ConfidenceLevel::Verified,
            Confidence::Unverified(_) => ConfidenceLevel::Unverified,
            Confidence::Unknown { .. } => ConfidenceLevel::Unknown,
        }
    }

    /// Borrows the carried value regardless of grade.
    #[must_use]
    pub fn value(&self) -> &T {
        match self {
            Confidence::Verified(v) | Confidence::Unverified(v) => v,
            Confidence::Unknown { assumed, .. } => assumed,
        }
    }

    /// Surrenders the carried value, discarding the grade. Named `into_` so
    /// the discard is visible at the call site.
    #[must_use]
    pub fn into_value(self) -> T {
        match self {
            Confidence::Verified(v) | Confidence::Unverified(v) => v,
            Confidence::Unknown { assumed, .. } => assumed,
        }
    }

    /// The note explaining an assumption, if this is an [`Confidence::Unknown`].
    #[must_use]
    pub fn note(&self) -> Option<&str> {
        match self {
            Confidence::Unknown { note, .. } => Some(note),
            _ => None,
        }
    }

    /// Transforms the value; the grade travels unchanged.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Confidence<U> {
        match self {
            Confidence::Verified(v) => Confidence::Verified(f(v)),
            Confidence::Unverified(v) => Confidence::Unverified(f(v)),
            Confidence::Unknown { assumed, note } => Confidence::Unknown {
                assumed: f(assumed),
                note,
            },
        }
    }

    /// Combines two graded values (ADR-007 propagation rule): the result's
    /// grade is the **minimum** of the inputs' grades. One `Unverified` input
    /// makes the whole result `Unverified`; notes from `Unknown` inputs are
    /// carried and joined so they can be surfaced in output.
    #[must_use]
    pub fn zip_with<U, V>(self, other: Confidence<U>, f: impl FnOnce(T, U) -> V) -> Confidence<V> {
        let level = self.level().min(other.level()); // probe: confidence-propagation
        let note = join_notes(self.note(), other.note());
        let value = f(self.into_value(), other.into_value());
        match level {
            ConfidenceLevel::Verified => Confidence::Verified(value),
            ConfidenceLevel::Unverified => Confidence::Unverified(value),
            ConfidenceLevel::Unknown => Confidence::Unknown {
                assumed: value,
                note,
            },
        }
    }
}

/// Joins the notes of up to two `Unknown` inputs. Empty only if neither input
/// was `Unknown` — and then it is never read, because the combined level
/// cannot be `Unknown` either.
fn join_notes(a: Option<&str>, b: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(a) = a {
        out.push_str(a);
    }
    if let Some(b) = b {
        if !out.is_empty() {
            out.push_str("; ");
        }
        out.push_str(b);
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::string::ToString;

    use super::*;

    fn graded(level: ConfidenceLevel, v: i64) -> Confidence<i64> {
        match level {
            ConfidenceLevel::Verified => Confidence::Verified(v),
            ConfidenceLevel::Unverified => Confidence::Unverified(v),
            ConfidenceLevel::Unknown => Confidence::Unknown {
                assumed: v,
                note: "assumed".to_string(),
            },
        }
    }

    #[test]
    fn propagation_is_minimum_over_all_nine_combinations() {
        use ConfidenceLevel::{Unknown, Unverified, Verified};
        for a in [Verified, Unverified, Unknown] {
            for b in [Verified, Unverified, Unknown] {
                let combined = graded(a, 2).zip_with(graded(b, 3), |x, y| x + y);
                assert_eq!(combined.level(), a.min(b), "{a:?} ⊕ {b:?}");
                assert_eq!(*combined.value(), 5);
            }
        }
    }

    #[test]
    fn one_unverified_input_degrades_a_verified_result() {
        // The ADR's own phrasing of the rule, as a direct example.
        let verified = Confidence::Verified(10);
        let unverified = Confidence::Unverified(1);
        let result = verified.zip_with(unverified, |a, b| a + b);
        assert_eq!(result.level(), ConfidenceLevel::Unverified);
    }

    #[test]
    fn unknown_notes_are_carried_and_joined() {
        let a = Confidence::Unknown {
            assumed: 1,
            note: "Ambush buff consumption".to_string(),
        };
        let b = Confidence::Unknown {
            assumed: 2,
            note: "Trickster on non-knives".to_string(),
        };
        let combined = a.zip_with(b, |x, y| x + y);
        assert_eq!(
            combined.note(),
            Some("Ambush buff consumption; Trickster on non-knives")
        );
    }

    #[test]
    fn map_preserves_grade_and_note() {
        let unknown = Confidence::Unknown {
            assumed: 4,
            note: "guess".to_string(),
        };
        let mapped = unknown.map(|v| v * 2);
        assert_eq!(*mapped.value(), 8);
        assert_eq!(mapped.level(), ConfidenceLevel::Unknown);
        assert_eq!(mapped.note(), Some("guess"));
    }
}
