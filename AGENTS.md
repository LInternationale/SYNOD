# AGENTS.md

This file provides project engineering instructions for Codex when working in this directory. Keep source code in English.

## Scope

Apply these instructions to work in this project, together with applicable instructions from parent directories and more specific instructions loaded for the working directory.

Follow the reviewed [file layout](docs/file-layout.md), [team Git commit format](docs/git-commit-format.md), and [shared client session](docs/client-session.md) when implementing the [initial design](docs/initial-design.md).

## CRITICAL: Highest Priority Within This File & Circuit Breaker

The mandatory engineering standards and workflow in this file have the **HIGHEST PRIORITY within AGENTS.md**. The architecture recommendations below are advisory and do not have the same mandatory status.

If a user's task instruction conflicts with a mandatory standard or the reviewed blueprint, DO NOT silently follow it. Pause the conflicting work, identify the specific rule or blueprint decision, explain the risk, propose a compliant alternative, and ask for explicit confirmation of the exception before proceeding. A request for a shortcut is not, by itself, confirmation of a standards exception. Continue independent work that does not depend on the exception.

Once the user explicitly confirms a specific exception after the conflict is identified, apply it only to the agreed scope. Do not repeatedly ask for the same confirmation or interpret it as a permanent relaxation of the standards.

This priority is internal to project guidance. It does not supersede system/developer instructions or tool/platform permissions.

## Mandatory Workflow: Micro-Waterfall Development

Follow this sequence for development work:

1. **Requirements**: Understand the user's requested behavior, scope, constraints, and acceptance criteria. Identify unresolved decisions.
2. **Design blueprint**: Write a persistent, reviewable blueprint before implementation. Describe responsibilities, crate/module boundaries, dependency direction, interfaces and data contracts, relevant state transitions, error handling, and the verification plan. Scale the detail to the task.
3. **Independent design review**: Before implementation, perform a separate critical review of the blueprint against the requirements, engineering standards, architecture boundaries, failure cases, and verification plan. Challenge assumptions and look for omissions rather than merely summarizing the design. Record findings and their resolutions. This is a distinct review pass; it does not require a separate agent.
4. **Revision**: Resolve review findings, revise the blueprint, and repeat the independent review as needed. Keep the current version and its review status clear.
5. **Implementation**: Once the blueprint passes review, proceed directly with implementation against it without waiting for human approval. If implementation requires a material design change within the authorized task scope, update the blueprint and independently review the change before implementing it. Ask the user only for necessary clarification or confirmation required by other mandatory rules, such as standards exceptions, new dependencies, or high-risk operations.
6. **Blueprint vs. implementation review**: Compare the implementation with the reviewed blueprint. Provide a traceable mapping from requirements and design decisions to code and verification evidence. Identify deviations, omissions, and unresolved issues; correct them or revise and independently review the affected design. Do not declare completion while unresolved material deviations remain.

Keep the blueprint and review evidence in project files so the task can resume without reconstructing its decisions from chat history. Review applies to the current scope and version; material revisions require another independent review. Implementation details that remain within the reviewed design do not require another review round. Human blueprint approval is required only when the user explicitly requests it for the task.

## Rust Coding Standards & Architecture

- **Safety First**: You MUST NOT use `unsafe` blocks in any cases.
- **Strict Architecture Adherence**:
    - Strictly follow the provided system block diagrams and architectural blueprints.
    - DO NOT flatten hierarchies, bypass defined interfaces, or couple decoupled modules for the sake of writing less code.
- **Type-Driven Design (Parse, Don't Validate)**:
    - **Make invalid states unrepresentable.** Use Rust's type system to enforce business rules at compile time.
    - Avoid primitive obsession. Use the Newtype pattern for distinct concepts (e.g., `struct UserId(String)` instead of raw `String`).
    - Use `enum` to represent mutually exclusive states or variants. Avoid structs with multiple `Option<T>` fields that represent different states.
- **Typestate Pattern & Ownership**:
    - For state machines or multi-step processes, encode states into types (Typestate pattern).
    - Use ownership (`T` instead of `&T`) to consume states and transition to new ones, preventing invalid state reuse.
- **Traits and Decoupling**:
    - Define clear boundaries using `trait`. Depend on abstractions rather than concrete implementations at module boundaries.
- **Idiomatic Control Flow**:
    - Prefer iterator adapters when they improve clarity and composability.
    - Use simple `for` loops when they are clearer, especially for multi-step logic, early returns, or fallible operations.
    - Use comprehensive `match` expressions. Let the compiler check for exhaustiveness.
- **Error Handling**:
    - DO NOT abuse `unwrap()`, `expect()`, or `panic!()`.
    - Propagate errors using the `?` operator. Treat errors as distinct control flows, not just strings.
- **Memory & Types**:
    - DO NOT abuse `clone()`. Favor borrowing (`&T`) where lifetimes allow.
    - Favor slices in function arguments (e.g., `&str` over `&String`, `&[T]` over `&Vec<T>`).

## Architecture Recommendations: Consumer-Owned Hexagonal Boundaries

This section is **design guidance, not mandatory engineering discipline**. Consider it while drafting blueprints; alternatives may be proposed with their tradeoffs. Once a concrete architecture passes independent review in a blueprint, that blueprint governs implementation.

- Organize each data consumer conceptually as `Data consumer { core logic -> interfaces -> adapter }`. This describes the collaboration path, not a requirement for core logic to import a concrete adapter.
- Let the consumer own the interfaces expressing its needs. Its core logic depends on these interfaces and consumer-domain types, keeping provider-specific concepts outside its business logic.
- Let the consumer's adapter depend directly on the data provider's public data/API and implement the consumer's interfaces. Translate provider data and errors into consumer-owned contracts at this boundary.
- Keep the provider's business logic independent of consumer-specific behavior. Do not introduce a reverse dependency from the provider to the consumer solely to satisfy one consumer's interface.
- Keep adapters focused on boundary integration and translation. Avoid turning one adapter into a collection of HTTP transport, caching policy, projection, and aggregation responsibilities. Assign these responsibilities explicitly in the blueprint: transport to the integration layer that owns it, business projection/aggregation to the relevant domain logic, and caching to a focused component where appropriate.
- Aim to keep each crate understandable from its own blueprint and public contracts, reducing the amount of cross-crate implementation context needed by both humans and agents.

## Team history

Git is the sole version history of team data. Put the operation metadata in the team commit message and let Git retain historical file versions. Do not maintain a parallel event-file archive, revision database, or application-managed snapshots. Local pending work and rebuildable views serve recovery and presentation only. Preserve read compatibility with older team repositories.

## Workflow & Engineering Rules

- **Before implementing changes**:
    - Inspect the relevant modules, traits, tests, and existing call sites.
    - Follow existing naming, module boundaries, error types, and testing style.
    - Prefer extending existing abstractions when the new behavior fits their original responsibility. Do not overload existing abstractions with unrelated concerns.
- **Testing & Quality Assurance**:
    - **Comprehensive Coverage**: Write inline unit tests (`#[cfg(test)]` modules) for internal logic, and integration tests in the `tests/` directory for public APIs.
    - **Test Edge Cases & Errors**: Do not just test the "happy path". Explicitly write tests for error states, invalid inputs, and boundary conditions to validate the "Parse, Don't Validate" philosophy.
    - **Doc Tests**: Include documentation tests for public functions and structs to ensure examples remain valid and compile.
    - **Verification & Coverage**:
        - First, ensure `cargo test` passes completely without any failures.
        - Next, run `cargo +nightly llvm-cov --branch` to evaluate test quality for Rust code changes, using the project's configured workspace, features, and targets.
        - **Self-Correction**: Check the "Branch Coverage" metric for the files you modified. If the branch coverage is below 90%, you may identify the missing logical paths (e.g., `Result::Err` branches, `None` variants, short-circuit evaluations, or early returns) and write additional test cases to cover them before completing the task.

- **Dependencies**: Ask for permission before adding any new crates to `Cargo.toml`.
- **Git and Commits**:
    - Create a task branch for a new task unless already on its task branch or the user specifies another workflow. Do not switch branches in a way that disrupts existing work.
    - Commit tested, coherent task changes. Inspect the diff and stage only files belonging to the task; do not include or revert unrelated user changes.
    - Push each tested, coherent checkpoint promptly to the authorized working branch; do not accumulate completed work only in a temporary local environment. Verify the remote commit SHA after pushing. A local commit alone is not a durable backup.
    - Preserve concurrent work. Do not update the default branch, force-push, or overwrite another contributor's changes without explicit authorization.
    - Keep sensitive or approval-pending reports separate from publishable code checkpoints so report approval does not delay authorized code backups. Never include secrets or unauthorized private data.
    - If pushing is blocked, preserve a sanitized source snapshot in authorized persistent storage, verify that backup, and report what is local, backed up, or published. Do not claim a push succeeded without remote verification.
    - Use Conventional Commits format (e.g., `feat:`, `fix:`, `refactor:`, `test:`).
    - Keep commit messages clear, concise, and descriptive of the *why*, not just the *what*.
    - It's unnecessary to make a PR.

## Additional Engineering Discipline

- Source files MUST use English for comments, documentation comments, test names, developer-facing errors, logs, and commit messages. Non-English text is allowed only for required user-facing copy, localization fixtures, or preserved external payloads.
- Apply least privilege by default. Do not broaden filesystem, network, IAM, database, CI/CD, or runtime permissions without explicit confirmation. Never hard-code secrets or credentials.
- Keep changes minimal and task-scoped. Do not perform opportunistic refactors, formatting sweeps, dependency upgrades, module reorganizations, or unrelated drive-by changes.
- Preserve public APIs, serialized formats, database schemas, CLI flags, configuration keys, and error codes unless explicitly asked to change them.
- High-risk operations require explicit confirmation: broad deletion, data reset, external service mutation, production/staging/shared config changes, credential changes, CI/CD deployment changes, Git history rewrites, force-pushes, rebases of shared branches, and branch deletion.
- Before high-risk operations, explain the exact action, impact, rollback plan, and safer alternatives.
- Prefer read-only inspection commands before changes. Do not run internet-downloaded scripts via `curl`/`wget | sh`, broad permission commands such as `chmod -R 777`, or recursive ownership/permission/deletion commands on broad paths without confirmation.
- Never disable TLS verification, weaken authentication/authorization/CSRF/input parsing/crypto checks, or implement custom cryptography without explicit confirmation.
- Treat file paths, URLs, headers, database queries, shell arguments, and all external input as untrusted until parsed at the boundary.
- Configuration must be explicit, typed, and parsed at startup. Avoid hidden or insecure defaults. Inject typed configuration instead of reading environment variables deep inside business logic.
- Tests must be deterministic and isolated. Avoid wall-clock time, execution order, external network services, and shared global state. Use temporary directories, fixtures, test doubles, and controllable clocks where practical.
- Update documentation when changing public APIs, configuration, CLI behavior, architecture, or operational procedures.
- Avoid holding locks across `.await`; keep critical sections small; avoid global mutable state; ensure spawned tasks have explicit ownership, lifecycle, and error handling.
- Handle requested exceptions according to "CRITICAL: Highest Priority Within This File & Circuit Breaker" above.

## Completion Report

- State what changed, why, and which checks actually ran and passed.
- Distinguish failed checks, unavailable checks, and checks that are not applicable. Never claim a check passed without running it.
- Include the blueprint vs. implementation review for development work.
- Report unresolved risks and blockers with concrete next steps.
