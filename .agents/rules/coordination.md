# Project coordination

## Start or resume work

- Open [Project status](../../project-status/README.md) and the matching task note when the work spans chats,
  contributors, or repo areas. Start from the user's requested task and verify that the note matches it.
- Inspect the current branch, revision, index, and working tree. A task note describes its recorded checkout. Confirm
  which changes and evidence are available here, and preserve other work in progress.
- Define the outcome, scope, and acceptance checks. Use `$codebase-design` to compare ownership and interfaces when
  structure is at issue.

## Delegate and integrate

The primary agent coordinates work. Use named [specialists](../README.md#codex-subagents) when their scope matches an
independent slice. For refactors, use `editor_best_practices` for Angular, `rust_best_practices` for Rust source layout,
`rust_crates` for cross-crate changes, or `tauri` for desktop boundaries. Delegate read-only work or assign
non-overlapping write scopes with acceptance checks. When the same files need work, agree on order and one writer. Use
separate branches or worktrees when contributors need independent checkouts.

Ask each specialist to return affected paths, decisions or findings, checks and results, and blockers. Integrate the
results and check the combined change. Either update the handoff yourself or delegate the update to
`project_orchestrator` after integration and review its note. Use one writer per task note.

## Write the handoff

Keep one Markdown note per task in [`project-status/tasks/`](../../project-status/tasks/). Reuse its identifier across
chats and update the note before ending a material work session, including a blocked session. Keep the overview current
when the project-wide baseline, milestones, or shared blockers change. Separate tasks can update their own notes
independently.

Record the following in a short note.

- Goal, acceptance criteria, status, confirmed owner if known, and date
- Branch and base revision, where the work can be found, and affected paths
- Decisions, completed work, and remaining work
- Commands or other checks run, their results, and verification still pending
- Blockers and the next executable step

Distinguish observations from decisions, and local-only work from changes available on a shared branch or PR. Link to
existing ADRs, issues, or test evidence. Keep credentials and private logs out of the notes. These records are for
project coordination. The [engine documentation workflow](../../AGENTS.md#documentation-maintenance) still governs the
developer site and its checkpoint.
