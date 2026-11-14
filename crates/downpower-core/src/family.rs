//! The eight cause families (DESIGN.md Decision 17), and sets of them for the views' filters.

use serde::{Deserialize, Serialize};

/// The eight families, which are the barcode's colors (Decision 17). The discriminant is the
/// family's three bits in a barcode cell, so the order here is part of the buffer format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Refueling,
    FuelCycle,
    Forced,
    Maintenance,
    Equipment,
    Environmental,
    Grid,
    Unattributed,
}

impl Family {
    /// Every family, in discriminant order.
    pub const ALL: [Self; 8] = [
        Self::Refueling,
        Self::FuelCycle,
        Self::Forced,
        Self::Maintenance,
        Self::Equipment,
        Self::Environmental,
        Self::Grid,
        Self::Unattributed,
    ];

    /// The order the charts stack and list families in, bottom up. It is also the order their
    /// colors were validated in as neighbors (`style/_families.scss`), so it changes only with them.
    pub const STACK: [Self; 8] = [
        Self::Refueling,
        Self::FuelCycle,
        Self::Maintenance,
        Self::Equipment,
        Self::Environmental,
        Self::Grid,
        Self::Forced,
        Self::Unattributed,
    ];

    /// The family's three bits.
    #[must_use]
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// The family whose three bits are `index`.
    #[must_use]
    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    /// The name a reader sees.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Refueling => "Refueling",
            Self::FuelCycle => "Fuel cycle",
            Self::Forced => "Forced",
            Self::Maintenance => "Maintenance",
            Self::Equipment => "Equipment",
            Self::Environmental => "Environmental",
            Self::Grid => "Grid",
            Self::Unattributed => "Unattributed",
        }
    }

    /// The name in URLs and CSS: the serde name with hyphens, `fuel-cycle`.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Refueling => "refueling",
            Self::FuelCycle => "fuel-cycle",
            Self::Forced => "forced",
            Self::Maintenance => "maintenance",
            Self::Equipment => "equipment",
            Self::Environmental => "environmental",
            Self::Grid => "grid",
            Self::Unattributed => "unattributed",
        }
    }

    /// The family named `slug`.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.slug() == slug)
    }
}

/// A set of families, one bit each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Families(u8);

impl Families {
    pub const ALL: Self = Self(u8::MAX);
    pub const NONE: Self = Self(0);

    #[must_use]
    pub const fn contains(self, family: Family) -> bool {
        self.0 & (1 << family.index()) != 0
    }

    #[must_use]
    pub const fn with(self, family: Family) -> Self {
        Self(self.0 | (1 << family.index()))
    }

    #[must_use]
    pub const fn without(self, family: Family) -> Self {
        Self(self.0 & !(1 << family.index()))
    }

    /// The set with `family` flipped.
    #[must_use]
    pub const fn toggled(self, family: Family) -> Self {
        Self(self.0 ^ (1 << family.index()))
    }

    /// The members, in stack order.
    pub fn iter(self) -> impl Iterator<Item = Family> {
        Family::STACK.into_iter().filter(move |family| self.contains(*family))
    }

    /// Comma-separated slugs in stack order, as a URL carries the set.
    #[must_use]
    pub fn to_param(self) -> String {
        self.iter().map(Family::slug).collect::<Vec<_>>().join(",")
    }

    /// The set a URL parameter names. Unknown slugs are dropped, so an old link still opens.
    #[must_use]
    pub fn from_param(param: &str) -> Self {
        param.split(',').filter_map(|slug| Family::from_slug(slug.trim())).fold(Self::NONE, Self::with)
    }
}

impl FromIterator<Family> for Families {
    fn from_iter<I: IntoIterator<Item = Family>>(families: I) -> Self {
        families.into_iter().fold(Self::NONE, Self::with)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn indexes_round_trip_and_stop_at_eight() {
        for family in Family::ALL {
            assert_eq!(Family::from_index(family.index()), Some(family));
        }
        assert_eq!(Family::from_index(8), None);
    }

    #[test]
    fn the_stack_holds_every_family_once() {
        let mut stack = Family::STACK.to_vec();
        stack.sort();
        assert_eq!(stack, Family::ALL.to_vec());
    }

    #[test]
    fn slugs_round_trip() {
        for family in Family::ALL {
            assert_eq!(Family::from_slug(family.slug()), Some(family));
        }
        assert_eq!(Family::from_slug("fuel_cycle"), None);
    }

    #[test]
    fn a_set_round_trips_through_its_url_parameter_in_stack_order() {
        let families: Families = [Family::Forced, Family::Refueling, Family::FuelCycle].into_iter().collect();
        assert_eq!(families.to_param(), "refueling,fuel-cycle,forced");
        assert_eq!(Families::from_param(&families.to_param()), families);
        assert_eq!(Families::from_param("forced,bogus"), Families::NONE.with(Family::Forced));
        assert_eq!(Families::ALL.iter().count(), 8);
    }

    #[test]
    fn toggling_flips_one_member() {
        let families = Families::ALL.toggled(Family::Grid);
        assert!(!families.contains(Family::Grid));
        assert!(families.contains(Family::Forced));
        assert_eq!(families.toggled(Family::Grid), Families::ALL);
        assert_eq!(Families::ALL.without(Family::Grid), families);
    }
}
