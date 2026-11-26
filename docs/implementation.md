# Rust implementation record

The scope is A1–A11 in initial-design.md. The first delivery connects cloud drafts, team-wide publication, completion and synchronization. Subsequent work covers progress, dependencies and workspace policies. The package and dependency direction follow file-layout.md.

## Team title comparison

The mail domain owns a title comparison key borrowed from the original title. Remove leading and trailing whitespace once. Preserve the spelling, internal spacing, case and Unicode composition. The board will compare these keys within a team across every published letter, independent of filters or completion. The key itself makes no decision about which inputs are valid titles.

## Independent review

A normalized string stored in the envelope would lose original spelling. Borrow a key from the original instead. A global index would merge teams and a filtered index would miss collisions. Keep the key free of any index or session state. This component is reviewed for implementation. Verify Unicode edge whitespace, internal whitespace and case sensitivity in unit tests. Cross-client collision recovery requires A3 and remains pending.

## Pending product decisions

First launch guides setup and preferences allow later changes. The board and detail occupy separate screens. Up and Down select a letter, Enter opens it and Escape returns to the same selection. Compose edits the body inside the TUI. All users may perform operations. Identity supplies attribution. Ratatui, Crossterm, Serde, serde_json and toml are approved. Further components require a reviewed implementation design before coding.

## Implementation comparison

TitleKey in src/mail/letter.rs borrows the trimmed title. Internal tests exercise case, whitespace and composition. The public integration test exercises lookup and the documentation example compiles. All three passed with cargo test. Nightly branch coverage reported 100% lines and zero measurable branches. A3 remains open until the publication path uses this key.

## Board navigation design and review

The TUI receives a published list from the session. It tracks the selected letter by identifier and holds either board or detail state. Enter and Escape change screens, arrows move within the supplied list. The caller owns ordering. The view never reads Git. Render the selected screen through Ratatui. Independent review identified that retaining a row number would open the wrong letter after a refresh. Resolve by using stable identifiers and exercise reordered input, empty lists, edge movement and detail rendering with the test backend.

BoardScreen implements the reviewed key transitions and resolves a selection by ID after reordering. The Ratatui backend test renders both screens and exercises empty and boundary cases. Navigation has 8/8 measured branches covered. The live session, setup, preferences and composer still need integration.
