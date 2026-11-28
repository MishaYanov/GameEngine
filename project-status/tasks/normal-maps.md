# Normal-map support

**Status** · Implemented and verified on Windows for the Castle Offense pilot, November 28,
2026. [Milestones 246–256](../../history/roadmaps/normal-maps-246-256.md) · [Verification](../../history/verification/normal-maps.md)

**Delivered** · Static, instanced, reflected, and skinned tangent-space normal maps. Embedded and contained external
glTF images. Independent normal UV coordinates and bounded MikkTSpace generation. Blender export and stable texture-only
reimport. Prop Workspace inspection and a per-preview comparison control. Castle Offense brick and flagstone use the
verified Fantasy Castle textures.

**Builds
** · [Portable Windows Editor](<D:/Builds/editor/Normal Maps/Editor/game-engine-editor.exe>) · [Castle Offense project](<D:/Projects/Castle Offense/project.json>) · [Standalone Castle Offense](<D:/Builds/games/Normal Maps/Castle Offense/game.exe>)

**Verified** · Packaged Editor Play, Save/Reopen, native export and copied Player startup, 30 Castle collision and route
checks, Blender 4.0 and 5.3 static and character exports, and sampled A/B frame and resource cost on an RTX 4070. The
report records the base-level sampler scope and benchmark coverage.
