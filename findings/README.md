# Findings

Each `NNNN-slug.md` here is a finding: a short note arguing one thing a figure shows, compiled into
the app and served at `/findings/NNNN-slug` with the figure live beside it (DESIGN.md Decisions 25
and 44–47, PRD-0008).

```markdown
+++
title = "Scrams fell by more than half, and stayed down"
summary = "One sentence for the index and the link preview."
view = "/trends"
query = "?from=1999-01-01"
figure = "scram-rate"
written = 2026-11-26
through = 2026-11-25
+++

The note, in markdown.
```

- `cargo xtask finding <url> <slug>` writes the next-numbered stub from a view's URL, with the
  query in its canonical order and both dates filled in.
- `view` is a chart view's path (`/`, `/trends`, `/scrams`, `/ffd`); `query` is its state, left out
  at the defaults; `figure` is the id of one of its figures (`crates/downpower-app/src/views.rs`).
- `written` is the day the note was written and `through` the data's last day then. Every number in
  the note holds as of those dates.
- The note is CommonMark with tables, footnotes and strikethrough. Raw HTML and `#` headings are
  rejected: the page's title is its only `<h1>`.
- The build fails on a malformed file, and `cargo make test` fails when a finding's query no
  longer reads back unchanged or its figure is gone.
- Notes are in Aaron's voice, and each lands only after he has read it (Decision 47).
