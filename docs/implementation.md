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

## Confirmed application flow

First run offers create-team and join-team paths. Either identity may be skipped. Preferences can change setup later. The board opens compose, preferences and workspace. Escape returns from each to the board. Compose edits in place and saves a draft. Sending acts on a draft. Workspace previews directory structure and handles updates, publishing and per-space mode selection. The operating system file manager handles browsing files. Draft discovery, unsaved changes, button activation and remote creation are pending interaction decisions.

## Team history message reader

Implement the v1 wire contract in the Git adapter before connecting it to a process. Deserialize the common metadata and dispatch the target table into one of five typed variants. Require an offset datetime and LF-terminated input. This reader consumes team history, never software development commits. The operations layer will later verify identifiers against paths and events before accepting history.

Independent review: target shape must depend on kind even when common metadata is valid. An absent target is legal only for team configuration. Test every fixture message and reject malformed metadata, target mismatches and unsupported versions. Scope here is decoding. Encoding and actual Git object verification remain outstanding.

Implementation comparison: commit_message::decode reads the complete message and maps each target to its operation variant. The fixture integration test covers all five variants, missing and surplus target data, invalid metadata, line endings, timestamp offsets and unsupported versions. Seven workspace tests including documentation passed. The reader covered 8/8 measured branches. Actual history replay is still pending.

## Draft rows and board actions

The board presents the active identity's draft copies first, then published letters, preserving session order inside each group. Prefix every draft title with 草稿. Distinguish draft copies by their complete draft reference and published records by letter ID. The runtime owns source data. The board component forwards selected drafts and the three board buttons as typed actions. Tab traverses the list and buttons. Enter and a primary-button click use the same button action. Derive hit regions from the render layout. Saving stays in compose; Escape on dirty input asks 是否丢弃更改. Repository creation follows confirmation of account, repository name and visibility. Preferences includes 设置主题色; its input and persistence interaction await confirmation.

Independent review: a draft and a publication can share a letter ID, and concurrent draft copies can share a title. Use separate UI keys and accept the complete copy reference. Keep drafts out of published title indexes. Verify draft ordering, button interaction equivalence and empty boards via the public component.

## Theme preference transaction

Only custom RGB is supported. Preferences edits three channels independently of the active theme. Apply Settings validates all three before requesting local persistence and only publishes the new color on success. Inject the initial active color from settings. The default RGB and which widgets it colors remain user decisions. Independent review: a failed save must preserve both the previous active color and the user's edits. Exercise invalid, failed and successful applications.

Bulletin verification: the public Ratatui test rendered the draft before the published letter, checked its label, activated all three buttons through both inputs, and opened the intended draft reference. Eight tests passed; bulletin branch coverage was 22/24.

ThemeEditor takes the current color explicitly. Its apply operation preserves edits on invalid input or storage failure. Ten tests passed including the public apply example. Overall measured branch coverage is 38/40. The preference form, local storage adapter and live terminal integration remain outstanding.

## Preference form and compose lifecycle

The initial color is RGB(70,93,112). A settings view opens with the active channels, stages keyboard edits, and exposes Apply Settings by mouse or keyboard. Route the resulting persistence request through the client. Escape returns to the board. The form retains invalid or unsaved input. The compose lifecycle compares current inputs with the last locally saved snapshot, remains open on save, and returns directly or asks for discard on Escape. Retain the last saved draft when discarding later edits. A failed save preserves the editor and the previous snapshot. Frontend functions never call Git.

Independent review: treating any keystroke as dirty prompts even after undo. Compare content. Capture success only after the client accepts persistence. Render and hit-test from the same geometry. Initial color is an explicit preference default and does not imply which other widgets to style. Test RGB defaults, both apply routes, failures, undo and discard.

Only selected items and keyboard focus use the applied RGB. Pass it from the client into board rendering and the theme form, preserving the default renderer as a compatibility entry point. Both roots are user specified in setup. Repository provisioning may use an already authenticated gh executable after explicit confirmation.

## Configuration location revision

Put client.json in the operating system configuration directory. It contains both chosen root paths and preferences. Keep runtime locks, team caches, outbox, recovery and local draft data in the chosen state root. The shared host will own configuration writes after setup. Local settings storage preserves unrelated JSON fields and atomically installs a replacement after syncing the new file. A missing setting uses the agreed theme default. Reject malformed existing settings rather than replacing user data. Use create_new for staging and remove only the temporary file owned by this write. Test reopening, unrelated fields, malformed input and default lookup. This revision supersedes the old client.json location.

## Repository provisioning adapter

Use a typed pending repository request (owner, name, public/private choice). The setup controller displays these exact values for confirmation and consumes the request to produce a confirmed request. Only the confirmed type can invoke provisioning. Execute gh directly with an argument vector and no shell or token handling. Failure returns the process status and diagnostics. No remote call in tests. Test invalid operands and exact argument construction for both visibility choices.

## Team setup inputs

The creator enters three human names, three assistant names and the workspace names in setup. Preserve names as display data; generate stable path-safe member and space identifiers by position within the new manifest. Existing teams retain their supplied identifiers. Serialize the initial manifest using the same v1 field names and member kinds as the repository fixture. No membership identity needs to be selected to enter these fields. The setup controller will pass this manifest to the team history adapter after provisioning. Verify multilingual display data, all six identities and empty display-name errors.

Implementation comparison: ThemeForm handles both apply routes; ThemeEditor keeps failed changes; board and form rendering use the applied color for highlights. ClientConfig resolves the OS path, decodes the default and atomically replaces settings. Editor snapshots implement escape and save semantics. The provisioning adapter accepts only confirmed values; the manifest builder accepts the creator's six display names and workspace names. Fifteen tests passed; measured branch coverage 80/96. A live TUI entry, session ownership, team initialization and all end-to-end publishing remain outstanding. No GitHub team repository was created during verification.

## Deferred initial publication

When both identities are skipped, persist the initial manifest in client.json as pending team configuration. Publishing requires a later explicitly bound member of that manifest. Never infer authorship from repository ownership. Do not consume the durable pending record when preparing a publication. Clear only after later remote verification. Independent review: sign-in and publication may be separated by a restart; exercise serialization roundtrip and check the member against the pending roster before constructing a signed intent.

PendingTeam is serialized in ClientConfig and exposes an initial publication only when supplied a matching bound actor. The restart test exercises skipped, unknown and known actors. Sixteen tests passed. The confirmation and publication worker still needs to be connected to this stored intent.
