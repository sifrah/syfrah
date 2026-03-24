# Contribution Workflow

This document describes how the Syfrah team tracks work and how contributors move a task from idea to merged code.

## Project board

We use a GitHub Project board with a Kanban layout. Every issue lives in exactly one column:

| Column | Meaning |
|---|---|
| **Backlog** | Captured but not yet refined. May lack acceptance criteria or sizing. |
| **Ready** | Refined, prioritized, and ready to be picked up. |
| **In Progress** | Someone is actively working on it. |
| **In Review** | A pull request is open and waiting for review or CI. |
| **Done** | Merged (or closed). |

## Issue hierarchy

Issues are organized into four levels:

| Level | Purpose | Example |
|---|---|---|
| **Epic** | A large initiative spanning multiple features | "Compute layer" |
| **Feature** | A user-visible capability within an epic | "VM lifecycle API" |
| **Story** | A slice of a feature that delivers value | "Create VM command" |
| **Task** | A concrete, small unit of work | "Add `vm create` CLI handler" |

Contributors typically pick up **tasks**. Each task links to its parent story (and transitively to a feature and epic) so you can always see the bigger picture.

## Prioritization

Every issue in Ready has a priority label:

| Label | Meaning |
|---|---|
| **P0** | Critical — drop everything |
| **P1** | High — do this sprint |
| **P2** | Medium — next sprint |
| **P3** | Low — when capacity allows |

When choosing what to work on, follow two rules:

1. **Higher priority first.** P0 before P1, P1 before P2, and so on.
2. **Smallest task first** (among equal priority). Smaller tasks merge faster and unblock others sooner.

## Task readiness criteria

A task is considered **Ready** when it has all of the following:

- A clear, imperative title (e.g., "Add `fabric leave` timeout flag")
- Acceptance criteria that describe what "done" looks like
- A **size label** (`XS`, `S`, `M`, `L`, `XL`)
- A **priority label** (`P0`, `P1`, `P2`, `P3`)
- A **layer label** indicating which crate the work belongs to (e.g., `layer/core`, `layer/fabric`)

If a task in Ready is missing any of these, flag it in the issue comments rather than guessing.

## Step-by-step contribution workflow

### 1. Pick a task

Go to the **Ready** column. Choose the highest-priority, smallest task that matches your skills.

### 2. Assign yourself and move to In Progress

Assign the issue to yourself on GitHub and drag it to **In Progress**. This signals to the rest of the team that the work is underway.

### 3. Create a branch

Branch from `main` using the naming convention:

```
{issue-number}-{short-slug}
```

For example, issue #42 about adding a leave timeout becomes:

```bash
git checkout -B 42-leave-timeout origin/main
```

### 4. Write code

Follow the conventions in [CONTRIBUTING.md](../CONTRIBUTING.md):

- Put code in the correct layer (`layers/{layer}/src/`)
- Put CLI commands in `layers/{layer}/src/cli/`
- Use `thiserror` for library errors, `anyhow` for the binary
- Add `serde` Serialize/Deserialize on public types

### 5. Run local checks

Before pushing, make sure everything passes locally:

```bash
cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test
```

All three must succeed. CI enforces the same checks, so catching problems locally saves time.

### 6. Push and open a pull request

Push your branch and open a pull request that references the issue:

```bash
git push -u origin 42-leave-timeout
```

In the PR description, include `Closes #42` (replacing `42` with your issue number). This automatically moves the issue to **Done** when the PR merges.

Follow the [pull request checklist](../CONTRIBUTING.md) in CONTRIBUTING.md.

### 7. Wait for CI

CI runs formatting, linting, and tests across all supported targets. The PR moves to **In Review** while this happens.

### 8. If CI is green

- Request a review (if not auto-assigned).
- Once approved, merge the PR.
- Delete the branch.
- The issue moves to **Done**.

### 9. If CI is red

- Read the CI logs to identify the failure.
- Fix the issue locally, commit, and push again.
- Repeat until CI is green, then go to step 8.

## Conventions summary

| Convention | Value |
|---|---|
| Branch naming | `{issue-number}-{short-slug}` |
| PR body | Must contain `Closes #N` |
| Commit style | Imperative mood, under 72 chars, reference issues |
| Checks before push | `cargo fmt && cargo clippy && cargo test` |
| Error handling | `thiserror` (libraries), `anyhow` (binary) |
| Serialization | `serde` on all public types |
| Async runtime | tokio |

## Releasing

Releases are automated. Every merge to `main` triggers the Release workflow, which calculates the next semantic version from commit messages, builds all targets, and publishes a GitHub Release. The workflow **validates** that the version in `Cargo.toml` matches the computed release version -- it does **not** auto-bump `Cargo.toml`. If they differ, the build fails.

Before merging a version-bumping PR, you must manually update `version` in `[workspace.package]` in the root `Cargo.toml` to match the version the release workflow will compute.

See [handbook/releasing.md](releasing.md) for the full release process, target matrix, and artifact details.
