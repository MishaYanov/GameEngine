# Native viewport implementation verification

The Courtyard Run update passed 317 editor tests, including Game status, Restart Run,
and the startup scene chooser regression. The production bundle and custom-protocol
host builds passed. On Windows, a generic-project smoke displayed the base props and
exercised Game status, Restart Run and Stop. Campaign objectives and completion in
Editor Game were verified in host and Angular tests. The full desktop walkthrough
below lists the broader spatial and campaign checks to record.

The following results describe the earlier viewport implementation.

The new math/interaction checks exercise projection conventions, rays, selection,
constraints, transaction cancellation, parent transforms and stale/lifecycle cases.
Renderer checks exercise editor data validation, clipping, capacity growth and
failure recovery. Fake-driver execution is not a physical-GPU image comparison.

Frontend tests cover camera/document notification separation, action coalescing,
queue limits, stale native state, normalized aperture coordinates, pointer capture,
drop/Escape/blur cancellation, async placement status, Play guards and toolbar AXE
semantics. Their mocked DOM cannot prove cross-HWND mouse delivery.

The final aggregate Angular run passed 277 tests across 38 files, including all
13 viewport service checks. After the narrow generic placement-message change,
the eight drag checks and exact default optimized production check passed again.
The production exclusion check, design build, five design-registry checks and
three schematic checks passed; production optimization was not weakened.

The final Rust app run passed 108 checks: 62 library plus 46 integration checks,
including the publication-deadline regression. The 47 math/interaction checks are
included in that result. Bridge/runtime/renderer/compiler passed 255 checks with
one intentional Vulkan-hardware check ignored. Workspace-local TEMP/TMP were
required for native layout-preference fixtures under the filesystem sandbox.
The final embedded host build with `--features custom-protocol` also passed.

Desktop smoke checked authoring creation/opening, Hierarchy-to-Inspector
selection, orthographic projection, and Play/Game/Stop returning to Scene with
the projection retained. Save/Undo stayed disabled after view-only changes.
The isolated fixture lives under `target/viewport-native-smoke`; sample sources
were not edited. An RTX 4070 renderer reported 1,515 frames and a final frame of
three draws, two instances and 100,085 triangles with an empty error log. These
are smoke counters, not a performance benchmark. The captured child was black, so this
does not establish visible grid/handle quality. Full spatial interaction, actual
cross-HWND drag delivery, desktop DPI, exported-player execution and another-machine
export remain separately recorded acceptance tasks.
