# Rust implementation record

The scope is A1–A11 in initial-design.md. The first delivery connects cloud drafts, team-wide publication, completion and synchronization. Subsequent work covers progress, dependencies and workspace policies. The package and dependency direction follow file-layout.md.

## Team title comparison

The mail domain owns a title comparison key borrowed from the original title. Remove leading and trailing whitespace once. Preserve the spelling, internal spacing, case and Unicode composition. The board will compare these keys within a team across every published letter, independent of filters or completion. The key itself makes no decision about which inputs are valid titles.

## Independent review

A normalized string stored in the envelope would lose original spelling. Borrow a key from the original instead. A global index would merge teams and a filtered index would miss collisions. Keep the key free of any index or session state. This component is reviewed for implementation. Verify Unicode edge whitespace, internal whitespace and case sensitivity in unit tests. Cross-client collision recovery requires A3 and remains pending.

## Pending product decisions

First-run setup, terminal layout and interaction, implementation dependencies and the write authorization matrix require confirmation. Further components require a reviewed implementation design before coding.

## Implementation comparison

TitleKey in src/mail/letter.rs borrows the trimmed title. Internal tests exercise case, whitespace and composition. The public integration test exercises lookup and the documentation example compiles. All three passed with cargo test. Nightly branch coverage reported 100% lines and zero measurable branches. A3 remains open until the publication path uses this key.
