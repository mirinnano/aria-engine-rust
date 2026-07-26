# ARIAPAK4 distribution contract

Aria packages use the `.ariapak` extension. A project may contain any number
of packs; empty roles are omitted. Each pack carries a signed metadata
manifest with a `pack_id`, `role` (`boot`, `hot`, `cold`, or `overlay`),
optional dependencies, a metadata `subtype`, priority, the post-decryption
`content_root`, the inner archive hash, key IDs, license policy, and format
version.

The CLI exposes three explicit profiles:

| Profile | Integrity | Encryption | Intended use |
| --- | --- | --- | --- |
| `dev` | BLAKE3 | none | local development and tests |
| `signed` | BLAKE3 + Ed25519 | none | authenticated public release |
| `protected` | BLAKE3 + Ed25519 | XChaCha20-Poly1305 per chunk | optional content protection |

Chunks are independently compressed and hashed. Protected chunks use a
manifest/chunk associated-data binding and a content-derived XChaCha nonce.
Protected package bytes are not a cross-platform identity. Native and Web
instead require the same decrypted content root, ARIAC checksum, and VM
replay hash.

The cryptographic package reader is outside `aria-core`. Core can unwrap a
plaintext development envelope for existing asset providers, but it does not
hold keys or verify publisher signatures. Native and Web use the same
`aria-protection` package reader and the same narrow `LicenseProvider`
contract. A provider only confirms entitlement and renews an offline,
signed, time-limited lease; it does not receive VM, renderer, or archive
internals. After package authentication, a Player calls the package
authorization helper with the provider, a key lookup, and an explicit current
time. A valid cached lease is accepted locally; otherwise the helper asks the
provider for one renewal and validates its signature, pack/game IDs, declared
window, grace period, and revocation state before assets are exposed.

Protected packs may start offline while their lease is valid or within its
declared grace period. Expiry and revocation are surfaced as provider/Player
diagnostics. In the PWA, `globalThis.ariaPakKeyProvider(bundle)` is the host
hook for short-lived verification/encryption keys; a production host should
obtain those keys and its lease through the same provider semantics before
returning them. Web key delivery is intentionally short-lived and does not
claim that a browser/WASM process can keep content secret from its user.

Example:

```sh
aria build my-game --target linux-x64 --profile dev
aria build my-game --target windows-x64 --profile signed \
  --signing-key publisher:0123456789abcdef...
aria build my-game --target web --profile protected \
  --signing-key publisher:0123456789abcdef... \
  --encryption-key content:fedcba9876543210...
```

Keys may also be supplied through `ARIA_PAK_SIGNING_KEY` and
`ARIA_PAK_ENCRYPTION_KEY`. Values are 32-byte keys encoded as exactly 64
hexadecimal characters, optionally prefixed with `key-id:`.
