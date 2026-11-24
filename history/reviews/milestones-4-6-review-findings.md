# Milestones 4–6 independent review findings

The implementation was committed as `b835718` before four independent reviews, as requested.
The reviewers compared it with `c34555e` and inspected the subsequent fixes, committed as `d5f34d2`. This document records
confirmed findings and how they were resolved; it is not a claim of exhaustive verification.

## Findings and resolutions

| Review | Finding | Resolution and evidence |
| --- | --- | --- |
| Security | Link and Update could apply source bytes changed after the review dialog opened | Both operations require the captured draft token. The host validates workspace generation, authoring revision, source asset ID, resolved path and the fingerprint of the parsed bytes. Native regressions reject changed, closed or foreign drafts without changing the scene; reopening permits a reviewed update and Undo restores the prior scene. |
| Security | Generic source command DTOs bypassed native source-review guards | The host rejects generic InstantiateLinkedPrefab and UpdateLinkedPrefab requests. Dedicated guarded commands remain available; trusted Rust core APIs retain their existing typed commands. Both rejection paths have regressions. |
| Optimization | Tick snapshots copied every instance's owned keys; inspection and spawners repeatedly scanned ownership | Ownership records and indexes share an Arc until a structural change. Entity and owner/component lookups are indexed; descendant traversal follows ownership edges. Clone-isolation and deep-cascade regressions verify rollback and cleanup. |
| Optimization | Visibility admission searched every prepared entity for each capability | It builds one membership set, then validates each capability. Rejected foreign capabilities preserve the previous accepted set. |
| Tests | Same-owner spawner independence and simultaneous parent/child expiry lacked combined public-API coverage | Two new tests exercise distinct attachment deadlines/alive limits and cascading expiry with duplicate despawn actions, including runtime handles, topology revision, pending requests and authored-data isolation. |
| Documentation | Current reference pages retained obsolete scene-schema and capability claims; section-link checks covered only one page at a time | Corrected current schema/component/prefab/lifecycle references, labeled historical walkthroughs, documented the source guards and ownership tradeoffs, and extended fragment validation across pages. All 62 pages and 8 site tests passed. |

For a 64-instance fixture with 16 entities each, inspecting 128 authored entities previously performed
131,072 ownership-key comparisons; the new path performs 128 indexed lookups. Validating a complete
4,096-entity visibility set previously required 8,390,656 equality checks; it now performs 4,096 set
inserts and 4,096 membership lookups. These are source-derived operation counts, not measured FPS
or latency. The additional indexes consume memory, and structural copy-on-write still has a cost.

## Verification

- The first committed implementation passed a fresh Rust workspace run: **850 passed, 0 failed,
  9 ignored**, across 76 result blocks. The Rust documentation command also passed, with no actual doctests.
- Native source-draft tests passed **8/8**, including the new security regressions. The Windows directory
  junction containment test passed separately with authorized native setup.
- Runtime review checks passed: **4 private ownership tests**, **10 lifecycle tests**, **13 boundary tests**,
  **2 added public API tests**, and **3 headless replay tests**. These overlap the aggregate suite.
- The editor passed **456 tests across 49 files** after adding draft-token transport checks, followed by
  production bundle verification and a successful native custom-protocol editor build.
- The final combined source passed **864 Rust tests, 0 failed, 9 ignored**, across 77 test targets.
  The fresh export outside the checkout matched the authored 1,245-tick headless route, and its hidden native
  lifecycle acceptance passed. Exact commands, export fingerprints and final checks are in the
  [verification ledger](../verification/milestones-4-6-tests-2026-11-20.json).

Reviewer reports and raw logs are kept under ignored `target/m46-review-*`; the tracked ledger records
reproduction commands, outcomes and hashes. The [implementation review](milestones-4-6-review.md)
explains how to use and inspect the completed features.

## Practical limits

GPU idle waits and full-scene preparation at spawn admission remain conservative. This patch does not
implement deferred GPU retirement or claim a frame-time improvement. No visible editor/player walkthrough
was performed in this turn; native acceptance uses hidden windows and real Vulkan resources. The security
review was targeted source inspection and regression testing, not exhaustive fuzzing or hostile same-user
filesystem race testing. Four unrelated backup files were preserved.
