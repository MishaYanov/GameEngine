# Rust file ordering

Rust permits ordinary items to be defined before or after their uses. The Rust Style Guide gives placement and sorting
guidance for `extern crate`, imports, and out-of-line modules. Match the crate's style edition and rustfmt
configuration. The type-first layout below is this skill's readability default for ordinary
code. [Rust Reference item order](https://doc.rust-lang.org/reference/items.html#r-items.decl-order) · [Rust Style Guide on items](https://doc.rust-lang.org/style-guide/items.html#items) · [Rust style editions](https://doc.rust-lang.org/style-guide/editions.html)

## Type-first layout

- Give each substantial type its own sequence of declaration, inherent implementations, and trait implementations. Keep
  a type-specific constant, error type, or helper with the code that uses it. A cohesive group of related types can be
  introduced together when their relationship would otherwise be hard to follow. Multiple inherent impls make sense for
  different bounds, configurations, or
  responsibilities. [Rust Book on multiple impl blocks](https://doc.rust-lang.org/book/ch05-03-method-syntax.html#multiple-impl-blocks)
- Follow the style edition and existing groupings for `use`, `pub use`, and `mod child;`. In a facade like `lib.rs`,
  make the public surface easy to scan. Preserve deliberate pairings of a module declaration with a re-export and keep
  their feature gates aligned. In `main.rs` or `build.rs`, put the entry point before supporting private functions when
  that makes the flow
  clearer. [Rust Style Guide on import grouping](https://doc.rust-lang.org/style-guide/items.html#ordering-of-imports) · [Rust Book on public re-exports](https://doc.rust-lang.org/book/ch07-04-bringing-paths-into-scope-with-the-use-keyword.html#re-exporting-names-with-pub-use)
- Keep a trait declaration easy to read by grouping associated types, constants, required methods, and provided methods
  when that matches the contract. Use the trait's declaration order in its impl where that aids comparison. Keep inline
  unit tests with the module, after production items. When the crate splits out tests with `mod tests;`, follow its
  established
  placement. [Rust Book on test organization](https://doc.rust-lang.org/book/ch11-03-test-organization.html#unit-tests) · [Clippy on items after a test module](https://clippy.rust-lang.org/master/index.html#items_after_test_module)

The following example shows the default for an ordinary module. Omit groups a file does not need.

```rust
//! Validated asset names.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct AssetName(String);

impl AssetName {
    /// Returns `None` for an empty name or one with surrounding whitespace.
    pub fn new(name: String) -> Option<Self> {
        if name.is_empty() || name.trim() != name {
            return None;
        }

        Some(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::AssetName;

    #[test]
    fn rejects_surrounding_whitespace() {
        assert_eq!(AssetName::new(" player ".into()), None);
    }

    #[test]
    fn accepts_a_valid_name() {
        let name = AssetName::new("player".into()).expect("fixture is a valid name");
        assert_eq!(name.as_str(), "player");
    }
}
```

## Preserve behavior while moving items

- Preserve `macro_rules!` definitions relative to unqualified invocations and child modules that inherit their textual
  scope. A later definition can shadow an earlier macro, and moving a module can change which definition it sees.
  Preserve legacy `#[macro_use]` module or crate boundaries and inspect `include!` and generated item sequences before
  moving across
  them. [Rust Reference macro scoping](https://doc.rust-lang.org/reference/macros-by-example.html#scoping-exporting-and-importing) · [standard library include macro](https://doc.rust-lang.org/std/macro.include.html)
- Keep `//!` docs and `#![...]` attributes at their enclosing scope. Move `///` docs, `#[cfg]`, `#[cfg_attr]`, derives,
  and procedural macro attributes with the item, retaining their relative order. Keep mutually exclusive platform or
  feature implementations readable as a group. Verify affected configurations, because a default build can skip the
  moved
  code. [Rust Reference attributes](https://doc.rust-lang.org/reference/attributes.html) · [Conditional compilation](https://doc.rust-lang.org/reference/conditional-compilation.html)
- Preserve field, variant, and statement order in a layout-only pass. Field order controls destruction and can affect
  ABI under `repr(C)`. Variant order can change implicit discriminants, and field or variant order can change a derived
  `Ord`. Serialization and generated code may depend on declaration order as
  well. [Field destruction](https://doc.rust-lang.org/reference/destructors.html#r-destructors.operation) · [C struct layout](https://doc.rust-lang.org/reference/type-layout.html#reprc-structs) · [Enum discriminants](https://doc.rust-lang.org/reference/items/enumerations.html#implicit-discriminants) · [Derived ordering](https://doc.rust-lang.org/std/cmp/trait.Ord.html#derivable)
- Keep public paths, re-exports, and visibility intact. Moving an item within a file differs from moving it to a
  different module. Update generated files through their source or generator when regeneration is part of the
  task. [Cargo SemVer compatibility](https://doc.rust-lang.org/cargo/reference/semver.html)

## Formatter and lint scope

Run the repository's rustfmt configuration. Rustfmt can reorder import or module groups. Its `reorder_impl_items` option
addresses members inside impl blocks, and availability depends on the toolchain. Rustfmt does not enforce type-to-impl
adjacency. Clippy's `arbitrary_source_item_ordering` is an opt-in restriction lint for configurable item groups. It can
impose alphabetical order on fields and variants and is a poor fit for semantic grouping. Avoid enabling it as a side
effect of organizing a
file. [Rustfmt configuration](https://rust-lang.github.io/rustfmt/) · [Clippy source item ordering](https://clippy.rust-lang.org/master/index.html#arbitrary_source_item_ordering)
