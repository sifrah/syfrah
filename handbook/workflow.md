---
tags: [workflow, contributing, ci]
---
# Workflow

## Overview

Syfrah uses a GitHub Project board (Kanban) to track all work. Every contribution follows the same workflow: pick a task from the board, code it on a branch, open a PR, wait for CI, merge, and clean up.

This document describes the board structure, the contribution workflow, and the conventions that keep everything moving.

## GitHub Project board

The project board has five columns. Every issue moves left to right, never backward.

```
    Backlog → Ready → In Progress → In Review → Done
```

| Column | What belongs here | Who moves issues here |
|---|---|---|
| **Backlog** | Epics, ideas, issues that need decomposition or clarification. Not ready to code. | Anyone |
| **Ready** | Decomposed tasks with a clear scope, acceptance criteria, and size label. Ready to be picked up. | Maintainer |
| **In Progress** | Someone is actively working on this. Assigned to the person coding it. | The person who picks it up |
| **In Review** | A PR is open and CI is running. Waiting for green checks. | The person who opened the PR |
| **Done** | PR merged, branch deleted. | The person who merges |

## Issue hierarchy

Issues follow a four-level hierarchy. Only **tasks** are directly codable — everything above is organizational.

```
    Epic          Multi-feature initiative (label: epic)
    └── Feature   Deliverable user value (label: feature)
        └── Story Testable slice of a feature (label: story)
            └── Task  Technical sub-step, directly codable (label: task)
```

**Epics** live in the Backlog until decomposed. **Tasks** are the unit of work that moves through the board. A task should be sized XS (< 1 hour), S (half-day), or M (1-2 days). Anything larger needs further decomposition.

## Prioritization

Tasks in the Ready column are picked by priority:

| Priority | Label | Meaning |
|---|---|---|
| **P0** | `P0` | Blocking — drop everything |
| **P1** | `P1` | Must-have for current wave |
| **P2** | `P2` | Should-have, schedule when ready |
| **P3** | `P3` | Nice-to-have, backlog |

When multiple tasks share the same priority, prefer the smallest first (XS over S over M) to keep throughput high.

## Contribution workflow

### 1. Pick a task

- Look at the **Ready** column, sorted by priority (P0 first)
- Assign yourself to the issue
- Move the issue to **In Progress**

### 2. Create a branch

Branch from `main`. Use the naming convention:

```
{issue-number}-{short-slug}
```

Examples:
```bash
git checkout main && git pull
git checkout -b 94-fix-version-report
git checkout -b 51-cli-help-formatting
```

### 3. Code

Follow the project conventions:

- **Formatting**: `cargo fmt`
- **Linting**: `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
- **Tests**: `cargo test` — all tests must pass
- **Commits**: imperative mood, under 72 characters, reference the issue

```bash
# Good
git commit -m "Fix binary version to read from workspace Cargo.toml

Fix #94"

# Bad
git commit -m "fixed stuff"
```

See [CONTRIBUTING.md](../CONTRIBUTING.md) for full conventions.

### 4. Run checks locally

Before pushing, run the same checks CI will run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

Or use the shortcut:

```bash
just ci
```

### 5. Push and open a PR

```bash
git push -u origin {branch-name}
```

Open a PR against `main`. The PR should:

- Have a clear title (under 70 characters)
- Reference the issue in the body (`Closes #94`)
- Describe what changed and why

### 6. Wait for CI

CI runs automatically on every PR:

- `cargo fmt --check` (workspace-wide)
- `cargo clippy -p {crate} --all-targets -- -D warnings` (per crate)
- `cargo test -p {crate}` (per crate)

Move the issue to **In Review** while CI runs.

### 7. Merge or fix

**If CI is green:**
- Merge the PR
- Delete the branch
- Move the issue to **Done**

**If CI is red:**
- Read the failure logs
- Fix the issue on the same branch
- Push again — CI re-runs automatically
- Repeat until green

### 8. Cleanup

After merge:
- The branch is deleted (GitHub can do this automatically)
- The release workflow validates the version and creates a release if a tag is pushed
- The issue is closed automatically via `Closes #N` in the PR

## Moving tasks to Ready

A task is **Ready** when it has:

1. **A clear title** — what needs to be done, not what's wrong
2. **Acceptance criteria** — how to verify the task is complete
3. **A size label** — XS, S, or M (if larger, decompose further)
4. **A priority label** — P0, P1, P2, or P3
5. **A layer label** — which crate is affected (e.g., `layer/fabric`, `cross-cutting`)

Example of a well-defined task:

```
Title: [Task] Fix binary version to read from workspace Cargo.toml
Labels: task, cross-cutting, P0, XS, wave/0
Body:
  The binary reports 0.1.0 instead of the workspace version.

  Acceptance criteria:
  - `syfrah --version` prints the version from workspace Cargo.toml
  - CI passes
```

## Conventions summary

| Convention | Rule |
|---|---|
| Board columns | Backlog → Ready → In Progress → In Review → Done |
| Codable unit | `task` label only |
| Branch naming | `{issue-number}-{short-slug}` |
| PR merges | Only after CI green |
| Branch cleanup | Delete after merge |
| Commit messages | Imperative, <72 chars, reference issue |
| Max task size | M (1-2 days). Larger = decompose. |
| Pick order | P0 > P1 > P2 > P3, then smallest first (XS over S over M) |
