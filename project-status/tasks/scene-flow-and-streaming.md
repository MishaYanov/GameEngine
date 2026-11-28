# Scene flow, world regions, and sequences

**Status** · Observed local work. Owner and acceptance criteria unconfirmed.

**Checkout** · Local `main` working tree at base `cc0e6b9`, inspected 2026-11-25.

The checkout contains `designs/scene-flow-and-streaming.md`, an editor scene-flow panel and contract, and related Rust
changes in `crates/engine-core/`, `crates/runtime/`, `apps/game/`, and the native editor host. Focused Rust tests and an
Angular scene-flow spec are also present. Their results were not checked for this handoff.

**Next step** · Locate the design draft and related changes in the checkout before running tests. If they are available,
inspect the diffs and confirm the intended first slice with the owner if needed. For the shared command contract, start
with
`cargo test --offline --locked -p engine-core --test scene_flow_api --test region_navigation_api --test sequence_api`,
then record the result here.
