//! Findings (DESIGN.md Decisions 25 and 44–47, PRD-0008): the table the build script compiles in
//! from `findings/*.md`, their pages, and each view's line of the findings drawn on it.

use downpower_core::findings::Finding;

/// Every finding, by number.
pub static FINDINGS: &[Finding] = include!(concat!(env!("OUT_DIR"), "/findings.rs"));

#[cfg(test)]
mod tests {
    use downpower_core::findings::date;

    use super::*;
    use crate::state::ViewState;
    use crate::views::View;

    /// Whether a finding draws a figure its view has, at a state its view reads: its query sets only
    /// the view's parameters and reads back unchanged, so a renamed or reordered parameter fails here
    /// instead of quietly drawing the default view.
    fn check(finding: &Finding) -> Result<(), String> {
        let stem = finding.stem;
        let views = View::ALL.map(View::path).join(", ");
        let view = View::from_path(finding.view).ok_or_else(|| format!("{stem}: no chart view at {:?}; there are {views}", finding.view))?;
        let reads = view.reads();
        if let Some((name, _)) = downpower_core::explorer::pairs(finding.query).into_iter().find(|(name, _)| !reads.contains(&name.as_str())) {
            return Err(format!("{stem}: {} doesn't read `{name}`; it reads {}", finding.view, reads.join(", ")));
        }
        let canonical = ViewState::parse_query(finding.query).to_query();
        if canonical != finding.query {
            return Err(format!("{stem}: the query {:?} reads back as {canonical:?}", finding.query));
        }
        if view.figure(finding.figure).is_none() {
            let figures: Vec<&str> = view.figures().iter().map(|figure| figure.id).collect();
            return Err(format!("{stem}: {} has no figure {:?}; it has {}", finding.view, finding.figure, figures.join(", ")));
        }
        Ok(())
    }

    fn finding(view: &'static str, query: &'static str, figure: &'static str) -> Finding {
        Finding {
            number: 1,
            stem: "0001-a",
            title: "A",
            summary: "A.",
            view,
            query,
            figure,
            written: date(2026, 11, 26),
            through: date(2026, 11, 25),
            html: "<p>A.</p>\n",
        }
    }

    fn error(finding: &Finding) -> String {
        check(finding).err().unwrap_or_default()
    }

    #[test]
    fn uat_findings_catalog_every_finding_draws_a_figure_its_view_has() {
        for finding in FINDINGS {
            if let Err(error) = check(finding) {
                panic!("{error}");
            }
        }
    }

    #[test]
    fn a_finding_on_a_view_figure_at_a_state_it_reads_checks() {
        assert_eq!(check(&finding("/trends", "?from=1999-01-01", "scram-rate")), Ok(()));
        assert_eq!(check(&finding("/", "", "lost")), Ok(()));
        assert_eq!(check(&finding("/ffd", "?table=1", "ffd-rate")), Ok(()));
    }

    #[test]
    fn uat_findings_checks_reject_an_unknown_view() {
        assert!(error(&finding("/events", "", "scram-rate")).contains("/events"));
    }

    #[test]
    fn uat_findings_checks_reject_a_query_that_doesnt_round_trip() {
        let error = error(&finding("/trends", "?families=forced,grid", "scram-rate"));
        assert!(error.contains("?families=grid,forced"), "{error}");
        assert!(!self::error(&finding("/trends", "?from=1999-1-1", "scram-rate")).is_empty(), "an unparsed date reads back as nothing");
    }

    #[test]
    fn uat_findings_checks_reject_a_parameter_the_view_doesnt_read() {
        let error = error(&finding("/ffd", "?from=2010-01-01", "ffd-rate"));
        assert!(error.contains("`from`"), "{error}");
    }

    #[test]
    fn uat_findings_checks_reject_an_unknown_figure() {
        let error = error(&finding("/trends", "", "scrams"));
        assert!(error.contains("scram-rate"), "the message lists the view's figures: {error}");
    }
}
