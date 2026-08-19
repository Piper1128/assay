//! Stable entity identity (ADR-004).
//!
//! Slug-based ids — `class.rogue`, `item.flanged_mace`,
//! `perk.rogue.lethal_mark` — assigned once and never reused. The id stays
//! permanent even when the game's display name changes; renames are explicit
//! data (`renamed_from`, ADR-008), never heuristics. Ids are ordinary strings
//! wrapped per entity kind so a `PerkId` can never be handed to an item
//! lookup. All ids are `Ord`, so they are free `BTreeMap` keys (ADR-001
//! rev 2: sorted iteration is the house collection order).
//!
//! Slug grammar is validated where datasets are loaded (`assay-data`,
//! ADR-004 schema validation) — the core wraps what it is given.

use alloc::string::String;
use core::fmt;

macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
        pub struct $name(String);

        impl $name {
            /// Wraps a slug, e.g. `"class.rogue"`.
            #[must_use]
            pub fn new(slug: impl Into<String>) -> Self {
                Self(slug.into())
            }

            /// The slug as text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_newtype!(
    /// Identity of a class definition (`class.rogue`).
    ClassId
);
id_newtype!(
    /// Identity of an item definition (`item.dark_leather_leggings`).
    ItemId
);
id_newtype!(
    /// Identity of a perk definition (`perk.rogue.jokester`).
    PerkId
);
id_newtype!(
    /// Identity of a skill definition (`skill.fighter.fortified_ground`).
    SkillId
);
id_newtype!(
    /// Identity of a curve definition (`curve.rogue.str_to_physical_power`).
    CurveId
);
id_newtype!(
    /// Identity of a derived-stat definition (`derived.action_speed`),
    /// ADR-012.
    DerivedStatId
);

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use alloc::collections::BTreeMap;
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn ids_order_lexicographically_as_btree_keys() {
        let mut map: BTreeMap<ItemId, i32> = BTreeMap::new();
        map.insert(ItemId::new("item.rondel_dagger"), 1);
        map.insert(ItemId::new("item.castillon_dagger"), 2);
        map.insert(ItemId::new("item.dark_leather_leggings"), 3);
        let keys: alloc::vec::Vec<&str> = map.keys().map(ItemId::as_str).collect();
        assert_eq!(
            keys,
            [
                "item.castillon_dagger",
                "item.dark_leather_leggings",
                "item.rondel_dagger"
            ]
        );
    }

    #[test]
    fn display_is_the_slug() {
        assert_eq!(ClassId::new("class.rogue").to_string(), "class.rogue");
    }
}
