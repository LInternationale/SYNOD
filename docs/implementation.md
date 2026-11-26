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

## Executable Unix client checkpoint

Start the terminal with Crossterm raw mode, alternate screen and mouse reporting, restoring the terminal on all exits. Bootstrap collects the two root paths before connecting to a separate Unix session process. Resolve existing directory aliases and reject overlapping roots. Persist accepted paths in client settings. A single host locks the state root, serves local requests and serializes settings writes. The front end sends requests over IPC. Present board and settings using the existing components. A live board must never claim remote publication for local-only state.

Independent review: two first launches may race during bootstrap. Serialize the bootstrap write and re-read client settings before accepting roots. Use a private socket directory, detect obsolete socket files only after obtaining the owner lock, and keep the same lock inode across restarts. Restore terminal state even when rendering or input fails. Exercise the real binary in an isolated PTY and check persistence separately.

The setup UI sends the team manifest and repository choice to the session. The host persists a selected team profile and either a pending manifest or an existing team's fetched manifest. The Git adapter inspects an existing HTTPS GitHub repository in a new isolated bare cache, reads team.json from main, and leaves verification of published mail history to the history reader. Only local draft saves may report local persistence. Keep local draft snapshots scoped by team and TUI identity. Limit workspace preview to names in the selected working directory. Exercise local actions without mutating an external repository.

## Initial team publication

On first identity selection freeze the repository URL, chosen actor, timestamp and stable operation ID in local settings before any Git write. Reuse the frozen operation after interruption. Publish a parentless team.configured commit containing only the frozen manifest; inspect the committed message and tree, and push without force. Verify the remote main SHA before clearing the pending record. If main has moved, inspect its root for the same initial operation and manifest. Never overwrite unrelated remote history. Verify against a temporary filesystem bare remote.

Executable checkpoint review: main.rs starts the interactive client or separate session owner. Setup, board, editor, preferences and workspace tree are reachable in tui/live.rs. Unix transport now uses a private short socket path, atomic endpoint.json and a generation handshake. An isolated PTY run caught a macOS inherited nonblocking socket bug; the accepted socket now explicitly uses blocking I/O. The PTY flow then saved and read back a local draft and applied a theme through the host. The bare-remote test retried an already published team configuration and verified exactly one commit on main. Cargo tests and nightly coverage ran; measured workspace branch coverage 36.57%, excluding the external PTY run. Real GitHub provisioning was not executed. Remote draft/letter publication, verified bulletin replay, worktree synchronization, MCP and Windows transport remain open.

## Workspace directory viewer

The workspace view offers the confirmed system-folder action for the configured editable root. Use the platform opener with a single absolute path argument and no shell. Escape returns to the board; arrows scroll the preview. A focusable button and its rendered rectangle share keyboard and mouse activation. Review: a file name from the preview must never become an executable or shell input. Inject only the validated workspace root; keep opener failures visible without closing the workspace view.

Workspace comparison: the live board opens the directory preview and the viewer dispatches both activations to the platform command. Cargo tests passed and nightly measured 98/278 branches across the workspace. The graphical system file manager was not launched during automated tests.

## Reply composition and confirmed send

The board offers quick reply on a selected published letter. New replies preselect the original author, cite the selected letter ID, and prepend Re: even when already present. Use the full board title index and the first free numbered suffix starting at 2. Compose chooses a single recipient and multiple CC members from the roster, and references from published letters independently of task dependencies. A native file picker supplies a path; read regular-file bytes into the draft so a moved original does not break it. New local fields default empty when loading older drafts. A send action asks once before submitting.

Independent review: do not infer task edges from references; keep separate fields. Avoid interpolating filenames into shell or AppleScript. Use stable letter IDs for selection and attachment IDs for repository paths. Local drafts must survive failed publication. Check prefilled recipients, repeated prefixes, suffix collisions, multi-selection cancellation, legacy draft decoding and frozen attachments.

## Git send path

Before publication freeze unique operation/letter IDs, actor, team, immutable draft bytes and offset timestamp on disk. Create the cloud draft commit from main, and publish the sealed original and LetterSent via a two-parent fast-forward candidate. Preserve references separately from initial_dependencies. Validate addresses, reference/dependency targets and title on refreshed main. Retry from current main on a competing push. Confirm the same frozen object from remote main before hiding the local draft. Failure leaves the editing snapshot available. Workspace conflict choices are 推迟提交 and 解决冲突; the resolution interaction is pending clarification.

Mail verification: live_client exercised real draft and two-parent send commits, idempotent retry, attachment byte equality, repeated Re: and collision suffixes, and separate envelope references. A PTY test selected a recipient and confirmed a send, then read letter.sent from the temporary remote main. Cargo tests passed. Nightly workspace branch coverage was 33.42%; this excludes external PTY interaction. No actual native graphical file dialog was opened by the tests.

## Saving and submitting workspace edits

Approved protocol extension: workspace.saved is a single-space commit on work/<actor>/<space>; workspace.published merges the saved version into main. Workspace selector uses the manifest's IDs and display names. Copy ordinary user files into an isolated worktree, compare the entire staged subtree to the last personal save and reject symlinks and nested Git metadata. Submit rechecks local contents. Unsaved edits require explicit Cancel or Save and submit; even the latter merges only a successfully pushed personal commit. Inspect candidate paths relative to main. Run the configured Git mergetool in the isolated conflicted checkout; defer leaves main untouched. Recheck main through normal fast-forward push. Independent review: using current main as the parent of every save would silently discard a genuine three-way conflict. Base later saves on the actor’s personal ref. A conflict must survive long enough for the user decision, and failed tool execution must never publish unresolved files.

Workspace comparison: the live workspace menu selects a manifest space before Save or Submit. The dirty result returns only Cancel or Save and submit. A conflicted merge is isolated from the editable tree; Defer abandons the pending merge and leaves personal and main refs intact. Resolve dispatches the configured mergetool; the Unix TUI releases the terminal for interactive tools. An isolated bare-remote test verified personal save, main publication, two authors editing the same file, defer preserving both versions, and a configured fixture mergetool completing the merge. The user's own GUI/tool was not invoked. Full offline tests passed; nightly measured 173/478 workspace branches (36.19%). Worktree cleanup, automatic fetching and per-space update/recovery policies still require further work.

## First-run directory picker revision

Replace the two typed path fields in first-run setup with selectable directory rows. Up/Down move between the state directory, workspace directory and bottom-left completion button. Enter or a primary click activates the focused control. A native folder dialog returns an optional path. Commit both choices through bootstrap::initialize only on completion. Existing configuration bypasses the screen.

Independent review: cancelling or failing the picker must preserve earlier selections. Keep status above the bottom button; share layout rectangles for drawing and mouse input. Exercise the focus order, picker cancellation and completion using injected local choices; verify left alignment in a Ratatui buffer.

Implementation comparison: live startup now drives DirectorySetup, which calls the native directory picker on activation and invokes bootstrap initialization only on completion. Buffer and input tests cover bottom-left placement, keyboard traversal, clicks, picker failure and cancellation, and incomplete choices. Full offline tests passed. Nightly reported 9/14 measured branches for the new screen. The graphical chooser itself was not opened during automation.

## Keyboard and startup revision

Use one previous/next focus rule for arrows, Tab and BackTab. Preserve arrow traversal of board rows and workspace preview scrolling; at the boundary move focus to actions. Enter activates the focused control. Escape remains local to the current navigation level. Root screens cannot end the process with Escape. Center the directory completion label across the entire interior row and preserve its mouse target.

Independent review: board actions are conditional on the current letter, so reverse traversal must omit unavailable actions. Test rendered centered text and focus transitions in both directions. Keep Ctrl+C as the existing explicit quit action.

## Extensible team roster

Create the repository and persist an empty team first. The next screen optionally adds one human and one assistant and activates the supplied identities locally. Persist the pending wizard step so reopening cannot recreate the repository. A skipped empty roster remains editable later. For subsequent additions use stable operation-derived member IDs and reapply the append to fresh main before each ordinary push. Keep existing IDs and display names and allow arbitrary roster size. Reuse the v1 team.configured operation for roster updates. Verify independent skips, more than six members, and concurrent or retried updates against a bare remote. Separate initial publication from later roster amendments so a retry cannot rewrite published history.

Local activation revision: represent no activation, human only, or human plus assistant as an exhaustive choice. Reject an assistant-only activation before any member creation or remote write. Ordinary roster additions may contain only an assistant because they do not change activation. Existing assistant-only settings are interpreted as inactive; retain the member in the team roster. A skipped unpublished team's later roster-only edits remain local until an identity can sign the initial publication. Check profile, initial registration, preferences and startup resume paths.

## Borderless TUI pages

Full-screen pages use a standalone title row with the content occupying the remaining width. Prompts may use a border if their title sits on a separate line; avoid embedding a title in any border. Apply this rule to future pages as well. Independent review: changing only the paint would leave an invisible border in hit testing and would misplace the full-width completion button. Derive page body and drawing from the same borderless title block. Verify the terminal buffer and bottom button hit area.

Implementation comparison: all current full-page render paths use the shared borderless title block. Directory setup tests check the title and empty right edge, centered text, full-width focus color, and mouse target. Board tests exercise entering controls with all four arrows. The session test created an empty team in an isolated bare repository, registered the initial pair, rejected AI-only activation, grew the remote roster to nine, and confirmed additions preserved both active identities. Offline cargo tests passed; nightly branch coverage was 247/610 (40.49%). Real GitHub provisioning was not invoked for this change.
