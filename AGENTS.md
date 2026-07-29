# Aria V3 Rust

This repository is the active Rust-native Aria runtime and the Umikaze
distribution project. The historical C# / Raylib implementation has been
retired and is not an authoring or release target here.

## Verify

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run --locked -p aria-cli -- check examples/umikaze --release
```

For Web portability, verify without the native desktop-player default feature:

```bash
cargo check --workspace --target wasm32-unknown-unknown --no-default-features --locked
```

The Umikaze presentation is game-owned under
`examples/umikaze/ui/`; keep the reading surface, VM protocol, save format,
and release package behavior consistent across Web and Tauri builds.

## Boundaries

- `crates/aria-core` owns language, VM, save, and package semantics.
- `crates/aria-native` and `crates/aria-web` are platform adapters.
- `examples/umikaze` owns game assets, scenario, and presentation.
- `compatibility/v3/umikaze-legacy` is migration input and historical
  evidence, not a runnable C# implementation.
