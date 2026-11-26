//! The chart views a finding can be drawn on (DESIGN.md Decision 45, PRD-0008 T-002): each one's
//! path, the figures it draws by id, and the query parameters it reads.

use leptos::prelude::*;

use crate::barcode::{self, BarcodeView};
use crate::chart::Figure;
use crate::ffd::{self, FfdFigure};
use crate::filters::FamilyKey;
use crate::lost::{self, StackedArea};
use crate::scrams::{self, ScramsFigure};
use crate::state::ViewState;
use crate::trends::{self, TrendsFigure};

/// The home view's figures: the barcode, and the stacked area beneath it.
const HOME: [Figure; 2] = [barcode::FIGURE, lost::FIGURE];

/// A chart view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Home,
    Trends,
    Scrams,
    Ffd,
}

impl View {
    pub const ALL: [Self; 4] = [Self::Home, Self::Trends, Self::Scrams, Self::Ffd];

    /// The view's path.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Home => "/",
            Self::Trends => "/trends",
            Self::Scrams => "/scrams",
            Self::Ffd => "/ffd",
        }
    }

    /// The view at `path`.
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.path() == path)
    }

    /// The figures the view draws, in order. The page draws them from this list, and a finding
    /// names one by its id.
    #[must_use]
    pub fn figures(self) -> &'static [Figure] {
        match self {
            Self::Home => &HOME,
            Self::Trends => &trends::FIGURES,
            Self::Scrams => &scrams::FIGURES,
            Self::Ffd => &ffd::FIGURES,
        }
    }

    /// The index of the figure `id` in [`View::figures`], if the view draws it.
    #[must_use]
    pub fn figure(self, id: &str) -> Option<usize> {
        self.figures().iter().position(|figure| figure.id == id)
    }

    /// Whether figure `index` takes its colors from the cause families. On its page the filters'
    /// legend is their key; a finding's page draws the key itself.
    #[must_use]
    pub const fn family_key(self, index: usize) -> bool {
        matches!((self, index), (Self::Home, _) | (Self::Trends, 0 | 1))
    }

    /// The `ViewState` parameters the view reads, so a finding's query sets nothing its view
    /// ignores: `/scrams` takes only the range, `/ffd` only the tables.
    #[must_use]
    pub const fn reads(self) -> &'static [&'static str] {
        match self {
            Self::Home => &["from", "to", "layer", "families", "events", "table"],
            Self::Trends => &["from", "to", "layer", "families", "table"],
            Self::Scrams => &["from", "to", "table"],
            Self::Ffd => &["table"],
        }
    }
}

/// Figure `index` of `view`, alone at `state`: what a finding's page draws (Decision 45).
#[component]
pub fn ViewFigure(view: View, index: usize, state: Memo<ViewState>) -> impl IntoView {
    let key = view.family_key(index).then(|| view! { <FamilyKey state /> });
    let figure = match (view, index) {
        (View::Home, 0) => view! { <BarcodeView state /> }.into_any(),
        (View::Home, _) => view! { <StackedArea state /> }.into_any(),
        (View::Trends, _) => view! { <TrendsFigure state index /> }.into_any(),
        (View::Scrams, _) => view! { <ScramsFigure state index /> }.into_any(),
        (View::Ffd, _) => view! { <FfdFigure state index /> }.into_any(),
    };
    view! {
        {key}
        {figure}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn uat_findings_figures_colored_by_family_take_the_family_key() {
        let keyed = |view: View| {
            (0..view.figures().len())
                .filter(|index| view.family_key(*index))
                .map(|index| view.figures()[index].id)
                .collect::<Vec<_>>()
        };
        assert_eq!(keyed(View::Home), ["barcode", "lost"]);
        assert_eq!(keyed(View::Trends), ["loss-by-year", "loss-by-month"]);
        assert!(keyed(View::Scrams).is_empty() && keyed(View::Ffd).is_empty(), "their figures carry their own legends");
    }

    #[test]
    fn uat_findings_figures_every_view_names_its_figures_with_unique_kebab_ids() {
        for view in View::ALL {
            assert_eq!(View::from_path(view.path()), Some(view));
            let figures = view.figures();
            assert!(!figures.is_empty(), "{view:?} lists its figures");
            let ids: HashSet<&str> = figures.iter().map(|figure| figure.id).collect();
            assert_eq!(ids.len(), figures.len(), "{view:?}'s figure ids are unique");
            for figure in figures {
                let kebab = figure.id.split('-').all(|word| !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_lowercase()));
                assert!(kebab, "{view:?}: {:?} is lowercase words joined by hyphens", figure.id);
                assert!(!figure.label.is_empty(), "{view:?}: {:?} has a label", figure.id);
            }
        }
        assert_eq!(View::from_path("/events"), None, "the explorer has no figures to draw");
        assert_eq!(View::Trends.figure("scram-rate"), Some(3));
        assert_eq!(View::Trends.figure("scrams"), None);
    }
}
