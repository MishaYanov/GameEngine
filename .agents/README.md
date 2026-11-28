# Game Engine agent guidance

Open Codex at the repository root to use the project skills and custom subagents.

| Location                                                                                   | Purpose                                                             |
|--------------------------------------------------------------------------------------------|---------------------------------------------------------------------|
| [Root instructions](../AGENTS.md)                                                          | Project entry point and documentation maintenance                   |
| [Editor instructions](../apps/editor/AGENTS.md)                                            | Angular, TypeScript, and accessibility rules                        |
| [Project workflow](rules/project.md)                                                       | Source map, development commands, and verification                  |
| [Coordination rules](rules/coordination.md)                                                | Delegation, integration, and handoffs across chats and contributors |
| [Project status](../project-status/README.md)                                              | Current baseline, open verification, and task notes                 |
| [.agents/skills/](skills/)                                                                 | Reusable workflows for the current agent                            |
| [.codex/agents/](../.codex/agents/)                                                        | Codex custom subagent definitions                                   |
| [Editor runtime profile](../crates/authoring-agent/profiles/game-development-support.json) | Built-in authoring agent configuration packaged with the editor     |

## Skills

| Skill                                                                | Use for                                                                                     |
|----------------------------------------------------------------------|---------------------------------------------------------------------------------------------|
| [game-development-support](skills/game-development-support/SKILL.md) | Authoring workflows, viewport interactions, gameplay configuration, and playable milestones |
| [logger-agent](skills/logger-agent/SKILL.md)                         | Logging instrumentation, runtime logging modes, and dedicated error persistence             |
| [rust-best-practices](skills/rust-best-practices/SKILL.md)           | Rust file ordering and idiomatic implementation or review                                   |
| [skill-creator](skills/skill-creator/SKILL.md)                       | Creating and maintaining reusable skills with the bundled tooling                           |
| [grill-me](skills/grill-me/SKILL.md)                                 | A design interview when a feature or structure decision is stuck                            |
| [codebase-design](skills/codebase-design/SKILL.md)                   | Compare interfaces, ownership, dependencies, and test seams                                 |

Invoke a skill by name, such as `$rust-best-practices`, or read its `SKILL.md` directly.

`grill-me` combines the upstream entry point and interview procedure for Codex. `codebase-design` retains the upstream
design references and uses the engine's existing terms for components, services, and crates. Both are based on
[Matt Pocock's skills at `d653911`](https://github.com/mattpocock/skills/tree/d65391149dbb5fe76478e76fa8d5122b2aa5a70e)
and include the upstream MIT license.

## Codex subagents

| Name                                                                 | Use for                                                                      |
|----------------------------------------------------------------------|------------------------------------------------------------------------------|
| [editor_best_practices](../.codex/agents/editor-best-practices.toml) | Angular structure, accessors, state ownership, and responsibility boundaries |
| [graphics_design](../.codex/agents/graphics-design.toml)             | Editor visuals, design-system components, themes, icons, and assets          |
| [rust_best_practices](../.codex/agents/rust-best-practices.toml)     | Rust source layout and behavior-preserving changes                           |
| [rust_crates](../.codex/agents/rust-crates.toml)                     | Crate boundaries, dependencies, and cross-crate APIs                         |
| [tauri](../.codex/agents/tauri.toml)                                 | Editor IPC, native lifecycles, and desktop configuration                     |
| [project_orchestrator](../.codex/agents/project-orchestrator.toml)   | Reconcile project status and write task handoffs when delegated              |
| [documentation](../.codex/agents/documentation.toml)                 | Check docs coverage and maintain authorized engine docs                      |
| [verification](../.codex/agents/verification.toml)                   | Run acceptance checks and report the evidence                                |
| [performance](../.codex/agents/performance.toml)                     | Measure performance and verify targeted optimizations                        |
| [security_review](../.codex/agents/security-review.toml)             | Defensive review of trust, data, and release boundaries                      |

## Maintenance

Keep project-wide workflow guidance in `rules/`, skills in `skills/`, and each Codex subagent in its own
`.codex/agents/*.toml` file. Update the root or editor `AGENTS.md` for rules owned by those entry points.

Validate a changed skill from the repository root with
`python .agents/skills/skill-creator/scripts/quick_validate.py .agents/skills/<skill-name>`.
The bundled validator requires Python and PyYAML. Check referenced paths and workflow behavior as well.
