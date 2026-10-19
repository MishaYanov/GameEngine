# Repository tests

The maintained [testing and validation guide](../docs/testing.md) documents current commands, runtime/public-API coverage, native smoke checks, and verification limits.

From the repository root, `python test_all.py` runs default automated suites. `python test_all.py --native` additionally exercises Vulkan runtime initialization, generated-project validation, and the hidden scene-rendering example. The native path requires GPU/window support and cached dependencies.

Tests now live with shared assets/scenes in `runtime`; earlier descriptions of the game and runtime as compile-only are superseded. User-confirmed native and visible-game checks completed the renderer/runtime extraction sequence. They do not imply every future change or installed distribution has been verified.
