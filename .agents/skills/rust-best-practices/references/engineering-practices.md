# Rust engineering practices

## API and ownership

- Keep fields and helpers private by default. Use `pub(crate)` for intentional crate-wide APIs, and `pub` for the
  supported external surface. For public libraries, check visibility, signatures, trait impls, re-exports, feature
  availability, and changes to publicly constructible structs or exhaustively matched
  enums. [Rust Reference visibility](https://doc.rust-lang.org/reference/visibility-and-privacy.html) · [Cargo SemVer compatibility](https://doc.rust-lang.org/cargo/reference/semver.html)
- Accept `&str` and `&[T]` when a function only needs a borrowed view. Take `&mut T` to mutate the caller's value and
  take ownership when consuming or retaining data. Reach for `Clone`, `Arc`, interior mutability, or more generic bounds
  when the ownership model or measured cost warrants it. Prefer clear names and conventions like `as_`, `to_`, and
  `into_` for
  conversions. [Rust Book on slices](https://doc.rust-lang.org/book/ch04-03-slices.html) · [API Guidelines on flexibility](https://rust-lang.github.io/api-guidelines/flexibility.html) · [Naming conventions](https://rust-lang.github.io/api-guidelines/naming.html)
- Use enums or validated newtypes when they rule out an invalid state or distinguish values callers could otherwise mix
  up. Derive common traits when the generated behavior matches the type's contract. Prefer `From` for infallible,
  lossless, obvious conversions and `TryFrom` when conversion may fail. Preserve consistent equality, hashing, and
  ordering. [API Guidelines on type safety](https://rust-lang.github.io/api-guidelines/type-safety.html) · [Interoperability](https://rust-lang.github.io/api-guidelines/interoperability.html) · [std conversion guidance](https://doc.rust-lang.org/std/convert/trait.From.html) · [Ord contract](https://doc.rust-lang.org/std/cmp/trait.Ord.html)
- Choose loops, iterators, and borrowing patterns for readability and the ownership needs of the code. In measured hot
  paths, check allocation, growth, copies, I/O, and contention against the actual workload. Preserve cleanup and
  cancellation semantics when changing asynchronous or concurrent
  code. [Rust Book on loops and iterators](https://doc.rust-lang.org/book/ch13-04-performance.html) · [Cargo benchmarks](https://doc.rust-lang.org/cargo/commands/cargo-bench.html)

## Failure, safety, and documentation

- Use `Option` for expected absence and `Result` with `?` for recoverable errors. Preserve actionable error variants and
  their sources when callers need to respond. Add context at boundaries where it explains the failed operation. Use
  assertions or `expect` where an invariant makes failure a bug or in a test with a known fixture, and explain the
  assumption when it is
  non-obvious. [Rust Book on when to panic](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html) · [std Error](https://doc.rust-lang.org/std/error/trait.Error.html)
- Use a safe abstraction when it can express the operation. For necessary unsafe code, use a small explicit
  `unsafe { ... }` even inside an `unsafe fn`. Document caller obligations in `# Safety`. Place a `// SAFETY:`
  explanation at the operation or unsafe impl and connect it to the actual preconditions. Review safe calls, lifetimes,
  aliasing, initialization, and relevant `Send` or `Sync` guarantees. For FFI, check the ABI, layout, ownership, and
  unwind boundary against the external
  contract. [Rust Reference unsafe](https://doc.rust-lang.org/reference/unsafe-keyword.html) · [Undefined behavior](https://doc.rust-lang.org/reference/behavior-considered-undefined.html) · [Rust 2024 explicit unsafe blocks](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-op-in-unsafe-fn.html) · [Rustonomicon FFI guidance](https://doc.rust-lang.org/nomicon/ffi.html)
- Write rustdoc that describes the caller-visible behavior. Add `# Errors`, `# Panics`, and `# Safety` where applicable.
  Use a doctest for practical public examples, module unit tests for internal behavior, and integration tests for public
  contracts. Exercise meaningful boundary cases and the failure path that would
  regress. [API Guidelines on documentation](https://rust-lang.github.io/api-guidelines/documentation.html#c-failure) · [Rust Book on test organization](https://doc.rust-lang.org/book/ch11-03-test-organization.html) · [Rustdoc tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html)

## Cargo verification

Use the pinned toolchain, crate `edition` and `rust-version`, formatter configuration, and lint policy. Prefer existing
CI or project commands. Typical starting points are `cargo fmt --all -- --check` for a workspace,
`cargo check -p PACKAGE` or `cargo clippy -p PACKAGE` for affected packages, and `cargo test -p PACKAGE` for behavior.
Replace `PACKAGE` with the actual package name. For a standalone file, configure rustfmt with the intended Rust edition
and style edition and check which child modules it would touch before
writing. [Rustfmt usage](https://github.com/rust-lang/rustfmt#usage) · [Cargo MSRV](https://doc.rust-lang.org/cargo/reference/rust-version.html) · [Clippy usage](https://doc.rust-lang.org/clippy/usage.html) · [Cargo manifest lints](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section)

- Match the supported feature and target matrix where the change can affect it. Use `--no-default-features`, explicit
  `--features`, or `--target` when relevant. Use `--all-features` if the combination is supported. Check the actual MSRV
  when the change uses new syntax, new library APIs, or newer dependencies. A pinned development toolchain does not by
  itself validate the
  MSRV. [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html#command-line-feature-options) · [Cargo Rust-version CI](https://doc.rust-lang.org/cargo/guide/continuous-integration.html#verifying-rust-version)
- Ordinary `cargo test -p PACKAGE` includes library doctests when enabled. With a targeted command such as
  `cargo test --all-targets`, use a separate `cargo test -p PACKAGE --doc` if doctest coverage is needed. Match the
  existing warning policy and the project's chosen Clippy groups. Use Miri for useful additional evidence on affected
  unsafe paths when the toolchain and code support it. A successful run covers the exercised
  paths. [Cargo test target selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection) · [Clippy lint groups](https://doc.rust-lang.org/clippy/lints.html) · [Miri limitations](https://github.com/rust-lang/miri#limitations)
