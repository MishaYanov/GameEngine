# The Hollow Maze and verified asset reuse

**Status** · Observed local work. Owner and acceptance criteria unconfirmed.

**Checkout** · Local `main` index and working tree at base `cc0e6b9`, inspected 2026-11-25.

The index contains the game project under `projects/the-hollow-maze/`, the maze implementation and tests under
`crates/gameplay-catalog/src/maze/`, and `history/adr/0015-verified-asset-reuse.md`. The gameplay catalog entry point
also has working-tree edits. Build, asset provenance, and play acceptance are unverified in this handoff.

**Next step** · Confirm the project and catalog changes are present in the checkout. In the originating worktree, review
`git diff --cached --stat -- projects/the-hollow-maze crates/gameplay-catalog history/adr/0015-verified-asset-reuse.md`
and the unstaged catalog changes. If the changes have since been committed, review the branch diff. Read the project's
README and asset-reuse manifest before running `cargo test --offline --locked -p gameplay-catalog`, then record the
result here.
