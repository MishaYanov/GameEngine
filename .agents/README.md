# Game Engine agent rules and skills

This bundle belongs to the full Rust, Angular, and Tauri project at `D:\Coding\Game Engine`. Start with the
repository-root `AGENTS.md` and the [project workflow](rules/project.md).

## Rules

- [Project workflow and source map](rules/project.md): ownership boundaries, source paths, development commands, and
  verification.
- [Angular and TypeScript rules](rules/angular-typescript.md): a copy of the existing `apps/editor/AGENTS.md`, including
  accessibility requirements. The existing editor file remains authoritative; refresh this copy when it changes.
- [Documentation maintenance rules](rules/documentation.md): the existing root documentation rules, captured before
  adding the bundle entry point. The repository-root `AGENTS.md` remains authoritative.

## Skills

| Skill                                                                | Use for                                                                                     | Origin                                                                                                                                                                        |
|----------------------------------------------------------------------|---------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| [game-development-support](skills/game-development-support/SKILL.md) | Authoring workflows, viewport interactions, gameplay configuration, and playable milestones | Copied from the existing `agent-profiles/game-development-support/`; corrected the shared gameplay source path and made feature-support wording conditional on current source |
| [logger-agent](skills/logger-agent/SKILL.md)                         | Logging instrumentation, runtime logging modes, and dedicated error persistence             | Copied from `~/.codex/skills/logger-agent/`; the shared Rust logger and its runtime modes already exist in this repository                                                    |
| [rust-best-practices](skills/rust-best-practices/SKILL.md)           | Rust file ordering and idiomatic Rust implementation or review                              | Adapted from the Game Engine 2 agent bundle using primary Rust language and tooling sources                                                                                   |
| [skill-creator](skills/skill-creator/SKILL.md)                       | Creating and maintaining reusable skills                                                    | Copied from `~/.codex/skills/.system/skill-creator/`, including scripts, references, UI metadata, assets, and license                                                         |

Read only the skill relevant to the current task. Example requests can invoke `$game-development-support`,
`$logger-agent`, or `$skill-creator`; if a client does not expose a skill, point it directly at that skill's local
`SKILL.md`.

From a Codex session opened at the repository root, invoke `$rust-best-practices` or ask the [
`rust_best_practices` subagent](../.codex/agents/rust-best-practices.toml) to organize or review a Rust change.

## Maintenance

Keep project-wide guidance in `rules/` and task-specific workflows in `skills/`. Use `.agents/skills/` for new project
skills. Update source maps and commands when the implementation moves. The older `agent-profiles/` files are preserved;
this bundle's game skill includes the source-path correction described above.

Validate a changed skill from the repository root with
`python .agents/skills/skill-creator/scripts/quick_validate.py .agents/skills/<skill-name>`. The copied validator
requires Python and PyYAML. Also check links, source paths, and behavior; structural validation does not establish that
instructions or feature claims are correct.

This bundle packages project guidance and relevant local skills. It does not export application configuration,
credentials, conversation history, or unrelated plugin skills. Packaging it does not authorize application
instrumentation or an engine documentation/site update.
