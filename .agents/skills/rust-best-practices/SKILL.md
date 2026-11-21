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

## Organize each file

For new files or an explicitly requested ordering pass, use this readability convention unless the crate documents a
different layout or the code needs a different order. Rust permits forward references for ordinary items.

1. Start with module or crate documentation and inner attributes, then any required `extern crate` declarations, imports
   and re-exports, and out-of-line module declarations. Follow the configured style for import and module groups.
2. Introduce the module's public concepts and related helpers together. Keep each `struct` or `enum` immediately before
   its inherent `impl` block(s), then the trait impls for that type. Group specialized impls by their bounds or
   configuration. Put a trait near its primary types or implementations when that improves discovery.
3. Within an inherent impl, lead with associated constants and constructors, then group public methods by use. Keep
   private helpers close to their callers or at the end of the impl. Place module-level functions and helpers where the
   module reads naturally. Put inline `#[cfg(test)] mod tests` last.
4. Move complete items with their docs, comments, and attributes. Preserve textual macro scope, conditional items, and
   the contents of declarations. For ordinary feature or bug edits, follow existing local order and keep movement
   limited to the requested change.

Read [File ordering](references/file-ordering.md) for a complete example, facade and entry-point layouts, and rules for
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
