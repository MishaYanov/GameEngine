# Resolved architecture decisions

This section records the **accepted core** of decisions agreed by the user on 2026-10-29. Resolved means the
architectural choice is settled within its stated scope. It does not mean all related features, UI work or verification
are complete.

| Accepted core                                                                                         | Decision status      | Implementation scope                                                                                                                   | Verification status                                                                                               |
|-------------------------------------------------------------------------------------------------------|----------------------|----------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------|
| [ADR-0004 — State lifetimes and prepared activation](0004-state-lifetimes-and-prepared-activation.md) | Accepted, 2026-10-29 | Authored/editor/run/scene ownership, typed in-memory progress and prepared scene/New Game activation are present in the inspected tree | Project5 compile passed; attributed external behavioral evidence has snapshot limits; no interactive acceptance   |
| [ADR-0005 — Serial scheduling, authority and rollback](0005-serial-scheduling-and-rollback.md)        | Accepted, 2026-10-29 | Serial bounded actions, aggregate supported world/progress commit and failure handling are present                                     | Compile and prior reported tests are distinct from proof of every failure path or arbitrary native-state rollback |
| [ADR-0008 — Native modules and typed progress](0008-native-modules-and-typed-progress.md)             | Accepted, 2026-10-29 | Native prepared factories, read-only progress/context, bounded typed domain actions and game-defined CampaignProgress are present      | No scripting, sandbox, plugin ABI or interactive demo acceptance follows from these checks                        |

Each page separates the decision, inspected implementation and verification. The authoritative accepted core lives here;
the original [ADR register](../README.md) keeps the proposal URLs, rationale, alternatives and remaining work. Broader
component lifecycle, animation, scripting/reload, apply-back and persistence proposals remain open. ADR-0001 is accepted
direction with substantial remaining UI work; it is not marked fully delivered or added here as a completed feature.

The inspection baseline is HEAD `b6ec4ba5a88c5ef66eff82fa61eb5747b940400d`, plus staged, unstaged and untracked content
inspected on 2026-10-29. Uncommitted source is part of this record; HEAD alone does not identify it.
`docs-site/docs-state.json` preserves the scoped source hashes, publication checks and prior attributed test evidence.
All content belongs to Engine Developer; Game App Developer remains empty.
