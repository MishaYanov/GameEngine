---
name: rust-best-practices
description: Write, organize, refactor, or review Rust .rs files. Use for struct and impl placement, module layout, and idiomatic Rust changes involving APIs, ownership, errors, unsafe code, or Rust verification.
---

# Rust Best Practices

Work in the Cargo workspace that owns the affected files. Read its local instructions, nearby modules, `Cargo.toml`,
toolchain and style configuration, and relevant CI jobs. Identify the crate, edition, minimum supported Rust version (
MSRV), features, and targets before recommending version-sensitive changes. For a standalone file or pasted snippet,
review the supplied code directly. If it is self-contained, a scratch compile or test can verify the example under a
stated edition and relevant `cfg` settings.

## Rules per crate

- Give each module a clear owner and a reason to change. Split independent behaviors into named modules, such as
  `game_camera/follow.rs` and `game_camera/orbit.rs`. Keep each behavior's settings, validation, state, and tests
  with its implementation. Keep shared invariants and transactions under one owner.
- Use `lib.rs` as the public entry point and `main.rs` for startup and wiring. Put substantial behavior in private
  modules and preserve existing public paths with re-exports when extracting code.
- Give each constant a home with the policy it controls. Use a module constant for a module-wide rule and an associated
  constant for a rule intrinsic to a type. When several modules enforce the same limit, use one domain-owned definition,
  such as `scene/limits.rs`. Use the narrowest visibility that serves those callers.
- Name constants with their unit or scope, such as `MAX_REPLAY_BYTES` or `MAX_ACTIONS_PER_TICK`, and record why a
  non-obvious limit exists. Use typed settings with documented defaults for values callers or game authors should tune.

## Rules per file

For new files or an explicitly requested ordering pass, use this readability convention. Respect the crate's documented
layout and preserve any order that affects behavior. Rust permits forward references for ordinary items.

1. Start with module docs and inner attributes, then required `extern crate` declarations, imports and re-exports,
   and out-of-line module declarations. Follow the configured style for import and module groups.
2. Put constants and aliases shared within the file near the top. Keep a constant or helper used by one type in or near
   that type's implementation.
3. Group by type. The default order is `struct A` → `impl A` → trait impls for `A` → `struct B` → `impl B` → trait
   impls for `B`. Apply the same pattern to enums. Co-locate closely related types when they form one readable concept.
4. Inside an inherent impl, lead with associated constants and constructors, then group public methods by purpose.
   Keep private helpers near their callers. Place shared private helpers after the main production items and put
   inline `#[cfg(test)] mod tests` last.
5. Move complete items with their docs, comments, and attributes. Preserve textual macro scope, conditional compilation,
   and field and variant order. In ordinary feature or bug edits, follow local order and keep moves within scope.

Read [File ordering](references/file-ordering.md) for examples, facade and entry-point layouts, and rules for
behavior-preserving moves.

## Implement and review

- Keep the API narrow and preserve caller-visible contracts. Borrow data for immediate use, take ownership when the API
  consumes or stores it, and use types to express meaningful invariants.
- Represent expected absence with `Option` and recoverable failure with `Result`. Preserve error causes and check
  untrusted, boundary, and failure inputs without relying on panics.
- Prefer safe Rust. Keep necessary unsafe operations small and document the caller's obligations and the evidence that
  each operation satisfies them.
- Document changed public behavior and use focused tests where the change introduces a regression risk. Review for
  concrete behavior, soundness, and compatibility issues before style. Cite files and lines when reviewing, and edit
  only when the task calls for changes.

Read [Engineering practices](references/engineering-practices.md) for public API design, ownership, errors, unsafe code,
and verification examples. Verify disputed or version-sensitive details against the applicable Rust toolchain docs.

## Verify the requested scope

Run the repository's checks from the owning workspace. Use its formatter configuration, targeted build or Clippy checks,
and tests for the affected behavior. For ordering changes, compare the diff for moved items and run checks for the
features and targets that can expose changed macro scope or configuration. Report the commands that ran and any relevant
configurations you could not exercise.
