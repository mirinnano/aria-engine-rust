# AriaEngine Rust

AriaEngine V3.1 is a Rust-native visual-novel runtime. The same deterministic
Core, VM state, render/audio protocol, and semantic input vocabulary are used
by the Native and Web adapters.

## Quick start

```bash
cargo run --locked -p aria-cli -- check examples/v3-minimal --release
cargo run --locked -p aria-cli -- run examples/v3-minimal --headless
cargo run --locked -p aria-cli -- build examples/v3-minimal \
  --target linux-x64 --profile dev --out dist/linux-x64
```

Release packaging uses an explicit signed or protected PAK profile:

```bash
export ARIA_PAK_SIGNING_KEY=publisher:<64-hex-bytes>
cargo run --locked --release -p aria-cli -- build examples/v3-minimal \
  --target linux-x64 --profile signed --release --out dist/linux-x64
scripts/build-v3-web.sh examples/v3-minimal dist/web
```

The `protected` profile additionally requires
`ARIA_PAK_ENCRYPTION_KEY=content:<64-hex-bytes>`.

## Formats and boundaries

- `.aria` is authoring source for Aria 3.1.
- `.ariac` is the validated ARIAC4 bytecode container.
- `.ariapak` is the PAK4 role/chunk envelope with `dev`, `signed`, and
  `protected` profiles.
- `aria-core` has no OS, filesystem, clock, network, key, or device access.
- Native and Web own rendering, audio, storage, package protection, and the
  narrow two-operation `LicenseProvider` contract.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo check -p aria-web --target wasm32-unknown-unknown --locked
```

The release gate also checks deterministic Native/Web replay hashes, bundled
fonts, target Player wrappers, migration diagnostics, and PAK integrity.

## Documentation

- [Aria 3.1 author language](docs/spec/aria-3.1.md)
- [V3 runtime and file formats](docs/spec/aria-v3-runtime.md)
- [PAK4 and license contract](docs/spec/pak4.md)
- [Native-first architecture](docs/architecture/v3-native-first.md)
- [V1/V2 migration](docs/development/v3-migration.md)
