//! Canonical encoding of resolved stat blocks (ADR-001 rev 2 §3).
//!
//! The diff never compares serialised text that happened to come out of some
//! serialiser — it compares **canonical forms**, and this module defines the
//! only one. The Python mirror (ADR-010 rev 2 §3) must reproduce it
//! byte-identically, so the grammar is spelled out here and nowhere else:
//!
//! - JSON-shaped text with **no whitespace**.
//! - Object keys in **lexicographic (byte) order** at every level.
//! - Numbers are integers only: [`Fixed`] values appear as their raw micro
//!   count (`"micro":7812500`), attributes as whole points. No floats exist
//!   in the canonical form.
//! - **Absence and null are distinct**: an absent field is omitted entirely;
//!   `null` never appears.
//! - Every value carries its grade: `{"confidence":"verified"|"unverified"|
//!   "unknown", ...}`; the `note` key exists exactly when the grade is
//!   `unknown`.
//! - Strings escape `"`, `\` and control characters below `0x20` (as
//!   `\u00XX`); nothing else is escaped.
//!
//! The trace is presentation, not state — it is not part of the canonical
//! form.

use alloc::string::String;

use crate::confidence::{Confidence, ConfidenceLevel};
use crate::fixed::Fixed;
use crate::resolve::Resolved;
use crate::schema::{AttributeBlock, AttributeKind};

/// Renders the canonical form of a resolved stat block.
#[must_use]
pub fn canonical_statblock(resolved: &Resolved) -> String {
    let mut out = String::new();
    out.push('{');
    // Top-level keys in lexicographic order.
    write_graded_fixed(&mut out, "action_speed", &resolved.action_speed);
    out.push(',');
    write_graded_fixed(&mut out, "armor_rating", &resolved.armor_rating);
    out.push(',');
    write_graded_attributes(&mut out, "attributes", &resolved.attributes);
    out.push(',');
    write_graded_fixed(&mut out, "health", &resolved.health);
    out.push(',');
    write_graded_fixed(&mut out, "move_speed", &resolved.move_speed);
    out.push(',');
    write_graded_fixed(&mut out, "pdr", &resolved.pdr);
    out.push(',');
    write_graded_fixed(
        &mut out,
        "physical_power_bonus",
        &resolved.physical_power_bonus,
    );
    out.push('}');
    out
}

/// Renders the canonical form of an exchange outcome (ADR-006), in the same
/// grammar as [`canonical_statblock`]. The trace is presentation and is not
/// part of it.
#[must_use]
pub fn canonical_exchange(outcome: &crate::exchange::ExchangeOutcome) -> String {
    let mut out = String::new();
    out.push('{');
    write_graded_fixed(
        &mut out,
        "damage",
        &outcome.damage.clone().map(|d| d.value()),
    );
    out.push(',');
    write_graded_fixed(
        &mut out,
        "effective_pdr",
        &outcome.effective_pdr.clone().map(|p| p.value()),
    );
    out.push('}');
    out
}

fn level_str(level: ConfidenceLevel) -> &'static str {
    match level {
        ConfidenceLevel::Verified => "verified",
        ConfidenceLevel::Unverified => "unverified",
        ConfidenceLevel::Unknown => "unknown",
    }
}

/// `"key":{"confidence":…,"micro":…[,"note":…]}` — inner keys are already
/// lexicographic: confidence < micro < note.
fn write_graded_fixed(out: &mut String, key: &str, value: &Confidence<Fixed>) {
    write_json_string(out, key);
    out.push_str(":{");
    write_json_string(out, "confidence");
    out.push(':');
    write_json_string(out, level_str(value.level()));
    out.push(',');
    write_json_string(out, "micro");
    out.push(':');
    push_i64(out, value.value().micro());
    if let Some(note) = value.note() {
        out.push(',');
        write_json_string(out, "note");
        out.push(':');
        write_json_string(out, note);
    }
    out.push('}');
}

/// `"key":{"confidence":…[,"note":…],"points":{…}}` — inner keys are already
/// lexicographic: confidence < note < points; attribute names are emitted in
/// their own lexicographic order.
fn write_graded_attributes(out: &mut String, key: &str, value: &Confidence<AttributeBlock>) {
    write_json_string(out, key);
    out.push_str(":{");
    write_json_string(out, "confidence");
    out.push(':');
    write_json_string(out, level_str(value.level()));
    if let Some(note) = value.note() {
        out.push(',');
        write_json_string(out, "note");
        out.push(':');
        write_json_string(out, note);
    }
    out.push(',');
    write_json_string(out, "points");
    out.push_str(":{");
    // Lexicographic attribute order, fixed here and mirrored in Python:
    // agility, dexterity, knowledge, resourcefulness, strength, vigor, will.
    const LEX_ORDER: [AttributeKind; 7] = [
        AttributeKind::Agility,
        AttributeKind::Dexterity,
        AttributeKind::Knowledge,
        AttributeKind::Resourcefulness,
        AttributeKind::Strength,
        AttributeKind::Vigor,
        AttributeKind::Will,
    ];
    let block = value.value();
    let mut first = true;
    for kind in LEX_ORDER {
        if !first {
            out.push(',');
        }
        first = false;
        write_json_string(out, kind.as_str());
        out.push(':');
        push_i64(out, i64::from(block.get(kind).points()));
    }
    out.push_str("}}");
}

fn push_i64(out: &mut String, value: i64) {
    // Formatting an integer is exact; no float ever touches the canon.
    out.push_str(&alloc::format!("{value}"));
}

/// JSON string escaping, minimal and closed: `"`, `\` and control characters
/// below 0x20 (as `\u00XX`). Nothing else changes, so the encoding of a
/// given string is unique.
fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                out.push_str(&alloc::format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;
    use crate::resolve::StageNote;

    fn graded(micro: i64) -> Confidence<Fixed> {
        Confidence::Unverified(Fixed::from_micro(micro))
    }

    fn sample_resolved() -> Resolved {
        let mut block = AttributeBlock::default();
        block.add(AttributeKind::Strength, 9);
        block.add(AttributeKind::Agility, 25);
        Resolved {
            attributes: Confidence::Unverified(block),
            physical_power_bonus: graded(-14_000_000),
            action_speed: graded(7_812_500),
            move_speed: graded(306_000_000),
            health: graded(108_500_000),
            armor_rating: Confidence::Verified(Fixed::ZERO),
            pdr: graded(-22_000_000),
            trace: Vec::new(),
        }
    }

    #[test]
    fn canonical_form_is_the_documented_grammar() {
        // The mirror contract, spelled out once in full.
        let expected = concat!(
            "{\"action_speed\":{\"confidence\":\"unverified\",\"micro\":7812500},",
            "\"armor_rating\":{\"confidence\":\"verified\",\"micro\":0},",
            "\"attributes\":{\"confidence\":\"unverified\",\"points\":{",
            "\"agility\":25,\"dexterity\":0,\"knowledge\":0,\"resourcefulness\":0,",
            "\"strength\":9,\"vigor\":0,\"will\":0}},",
            "\"health\":{\"confidence\":\"unverified\",\"micro\":108500000},",
            "\"move_speed\":{\"confidence\":\"unverified\",\"micro\":306000000},",
            "\"pdr\":{\"confidence\":\"unverified\",\"micro\":-22000000},",
            "\"physical_power_bonus\":{\"confidence\":\"unverified\",\"micro\":-14000000}}"
        );
        assert_eq!(canonical_statblock(&sample_resolved()), expected);
    }

    #[test]
    fn note_key_exists_exactly_for_unknown() {
        let mut resolved = sample_resolved();
        assert!(!canonical_statblock(&resolved).contains("\"note\""));
        resolved.pdr = Confidence::Unknown {
            assumed: Fixed::from_int(-22),
            note: "cap interaction untested".to_string(),
        };
        let canon = canonical_statblock(&resolved);
        assert!(canon.contains(
            "\"pdr\":{\"confidence\":\"unknown\",\"micro\":-22000000,\"note\":\"cap interaction untested\"}"
        ));
    }

    #[test]
    fn trace_is_not_part_of_the_canon() {
        let mut a = sample_resolved();
        let mut b = sample_resolved();
        a.trace = vec![];
        b.trace = vec![StageNote {
            stage: 1,
            label: "x",
            detail: "y".to_string(),
        }];
        assert_eq!(canonical_statblock(&a), canonical_statblock(&b));
    }

    #[test]
    fn string_escaping_is_closed_and_unique() {
        let mut resolved = sample_resolved();
        resolved.health = Confidence::Unknown {
            assumed: Fixed::ZERO,
            note: "quote \" backslash \\ newline \n".to_string(),
        };
        let canon = canonical_statblock(&resolved);
        assert!(canon.contains("quote \\\" backslash \\\\ newline \\u000a"));
    }

    #[test]
    fn encoding_is_deterministic_across_runs() {
        let a = canonical_statblock(&sample_resolved());
        let b = canonical_statblock(&sample_resolved());
        assert_eq!(a, b);
    }
}
