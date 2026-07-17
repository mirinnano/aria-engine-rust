# AriaEngine V3.1 runtime and file formats

V3.1 is the Rust native-first runtime. `aria-core` owns parsing, semantic
analysis, deterministic VM state, and value-only protocol messages. Winit,
wgpu, Kira, AccessKit, IndexedDB, and the filesystem live in adapters. A
Player executes `.ariac`; it never interprets source text at runtime.

The stable 1.0 surface is the `aria` CLI, the Native/Web Player, `aria.toml`,
`.aria`, `.ariac`, `.ariapak`, and `SaveEnvelopeV3`. Rust crate types are not
an embedding SDK contract.

## Project boundary

```toml
schema = 3

[game]
id = "jp.example.game"
version = "1.0.0"
title = "タイトル"

[runtime]
entry = "scripts/main.aria"
logical_width = 1280
logical_height = 720
asset_roots = ["assets"]
fonts = ["assets/fonts/NotoSansJP-Regular.ttf"]
save_namespace = "example-v3"
```

Unknown fields are errors. IDs use lowercase ASCII plus `.`, `-`, and `_`.
Logical paths are relative, NFC-normalized, slash-separated, and cannot escape
the project. Asset roots and font paths are checked for case-insensitive
collisions. A release requires at least one readable bundled font; no Player
discovers a Windows, Linux, or browser system font.

## CLI

```text
aria check <project> [--release]
aria run <project> [--headless|--replay <tape>]
aria build <project> --target windows-x64|macos-universal|linux-x64|steamdeck-x64|web \
  [--profile dev|signed|protected] [--signing-key <key>] [--encryption-key <key>] [--release]
aria migrate <project> [--game-id <id>]
```

`check --release` and `build --release` require structured `aria 3.1;`
source, a complete semantic compile, no `Host` opcode, exact asset/font
references, and a valid package runtime. Release builds must explicitly use
the `signed` or `protected` PAK profile; `dev` is for local/test data only.
`scripts/build-v3-web.sh` and `.ps1` build release WASM, run `wasm-bindgen`,
and pass that package to the same release builder.

## `.ariac` (ARIAC4)

All integer fields are little endian. The binary is deterministic and contains
no JSON, host pointer, platform path, or device type.

| Field | Size | Meaning |
|---|---:|---|
| magic | 8 | `ARIAC4\0\0` |
| checked header | 34 | format, language, VM ABI, flags, table counts, body size |
| game ID | variable | UTF-8 game ID |
| body | variable | constants, instructions, and source map |
| checksum | 32 | BLAKE3 of the checked header, game ID, and body |

Format version is 4 and the current VM ABI is 1. Language `3.0` is accepted
only by the migration bridge; release packages must carry language `3.1`.
The decoder validates all lengths, UTF-8 strings, finite numbers, operand
arity, constant indexes, jump addresses, register names, and source-map shape
before constructing a VM.

The 3.1 compiler lowers dialogue, explicit waits, scenes, choices, typed
state, backgrounds, sprites, transitions, audio, save/load, and terminal flow
to the fixed opcode table. `Host` is a diagnostic compatibility instruction;
3.1 source never emits it.

Native input is normalized before it reaches the VM. Winit handles keyboard,
pointer, touch, DPI, and focus transitions; gilrs polls standard gamepad
buttons/axes and hot-plug events on Windows WGI and Linux evdev. Steam Input's
standard-controller emulation is consumed through that same OS gamepad path
when Steam exposes it. All of these sources become the same `InputAction`
values, so the game script cannot observe the host OS or controller backend.

## `.ariapak` (PAK4)

PAK4 keeps a deterministic `ARIAPAK4` Core archive inside a variable-size
`.ariapak` role envelope. Packs use any non-empty combination of `boot`,
`hot`, `cold`, and `overlay`; dependencies, priority, locale/patch/DLC
subtype, content root, archive hash, key IDs, license policy, and format
version live in the authenticated manifest. Empty roles are omitted.

The three explicit profiles are:

| Profile | Integrity | Encryption | Use |
|---|---|---|---|
| `dev` | BLAKE3 | none | local development/tests |
| `signed` | BLAKE3 + Ed25519 | none | authenticated release |
| `protected` | BLAKE3 + Ed25519 | XChaCha20-Poly1305 per chunk | optional content protection |

Each envelope chunk is independently compressed, hashed, and optionally
encrypted. The Core may unwrap only `dev`; keys, signature verification,
encryption, and lease authorization live in Native/Web adapters. Protected
launch uses the narrow `LicenseProvider` entitlement/renewal contract and a
signed offline lease with an explicit expiry and grace period. Encrypted
package bytes need not be identical across platforms; the decrypted content
root, ARIAC checksum, and VM replay hash are the cross-platform identity.

The detailed manifest and cryptographic layout is specified in
[`pak4.md`](pak4.md).

## Bundle and Player wrapper

`bundle.aria.json` schema 5 records the game/language/VM metadata, ordered
`font_assets`, `.ariac` and PAK checksums/sizes, PAK content root, and a
metadata content root. `build-manifest.json` schema 5 records only target
wrapper facts: target, bundle checksum, native Player inclusion, and Web
runtime requirement. Launch validates those flags against the neighboring
files. A Windows/Linux release must contain a target-valid
`aria-player(.exe)`; packaging a debug executable or a target-mismatched file
fails closed. A `macos-universal` wrapper requires a genuine FAT Mach-O with
both x86_64 and arm64 slices.

## Saves

`SaveEnvelopeV3` contains `schema_version`, `game_id`, `engine_version`, host
timestamp, payload, and a BLAKE3 checksum. Native storage uses temporary write,
flush, atomic replace, and a previous generation. Web storage uses IndexedDB
transactions and at least two generations. A corrupt newest generation is
skipped in favor of the previous valid one; a save from another game or schema
is rejected.

## Extension policy

The 1.0 extension contract is `.aria`, themes, assets, and declarative data.
Native ABI, Rust plugins, and WASM plugins are intentionally not public.
