# Umikaze V3 distribution

The release line has one content bundle and several thin delivery shells. The
compiled program and split PAKs are produced once; Windows, Linux, macOS, and
Web only wrap that same bundle. This keeps save compatibility and download
deduplication stable across targets.

## Fast local build

`prepare-desktop.mjs` fingerprints the Aria VM/runtime and the presentation.
Unchanged runs reuse `target/aria-web-runtime-tauri` and
`target/aria-presentation-tauri`; set `ARIA_FORCE_REBUILD=true` when diagnosing
toolchain changes. The normal development command remains:

```sh
npm --prefix examples/umikaze/ui run prepare:desktop
```

It uses an unsigned `dev` PAK. A WebView release uses a `signed` PAK (the
public verification key is safe to ship; the private key stays in CI):

```sh
ARIA_PAK_PROFILE=signed \
ARIA_PAK_SIGNING_KEY='publisher:<64-byte-hex-key>' \
ARIA_PAK_VERIFICATION_KEY_ID=publisher \
ARIA_PAK_VERIFICATION_KEY_HEX='<32-byte-public-key-hex>' \
npm --prefix examples/umikaze/ui run release:desktop
```

The script selects `deb` on Linux, `dmg` on macOS, and `nsis` on Windows. Set
`ARIA_TAURI_BUNDLES` to override the platform default (for example,
`deb,appimage` when `appimagetool` is installed). Platform signing and macOS
notarization happen after the unsigned Tauri bundle is created.

## CLI installers

For a native `aria build` bundle, the CLI can produce the portable archive and
installers without a GUI:

```sh
cargo run --release -p aria-cli -- build examples/umikaze \
  --target linux-x64 --profile signed --release --out dist/umikaze-linux
cargo run --release -p aria-cli -- package dist/umikaze-linux \
  --format auto --out dist/releases/linux
```

`auto` emits a deterministic ZIP, a self-contained user-level Linux `.run`,
and, when the host tools exist, `.deb` and AppImage. On macOS it emits an
`.app.tar.gz` and a `.dmg` when `hdiutil` is available. `--format installer`
turns missing native tools into an error; `--format zip` is portable-only.
Every package writes `release-manifest.json` and `checksums.sha256`.

The Linux `.run` installs under `~/.local/share/<game-id>` by default and
accepts `--install-dir DIR`. It does not require root. `.deb` remains the
system-integrated option for distributions that ship `dpkg-deb`.

## Web release

```sh
ARIA_PAK_PROFILE=signed \
ARIA_PAK_SIGNING_KEY='publisher:<64-byte-hex-key>' \
ARIA_PAK_VERIFICATION_KEY_ID=publisher \
ARIA_PAK_VERIFICATION_KEY_HEX='<32-byte-public-key-hex>' \
npm --prefix examples/umikaze/ui run release:web
```

The result is a static PWA archive plus `web-release.json`, `_headers`,
`release-manifest.json`, and `checksums.sha256`. Deploy the archive contents to
any static host. Immutable runtime/PAK files receive a one-year immutable
cache policy; `index.html`, the manifest, and the service worker are always
revalidated. The service worker keeps the first-load path small by fetching
the hot pack only when the reader needs it.

The GitHub Actions workflow `.github/workflows/umikaze-release.yml` builds the
Web artifact and the three native installer families from the same tag. It
expects the production PAK signing key in the `ARIA_PAK_SIGNING_KEY` secret;
the matching public verification key is supplied through
`ARIA_PAK_VERIFICATION_KEY_ID` and `ARIA_PAK_VERIFICATION_KEY_HEX`. No private
key is stored in the repository.

## Demo edition

The demo is a separately compiled DAY 0–4 edition, not a full bundle that
checks a runtime stop flag. Its logical entry is `scripts/main-demo.aria`; the
compiler imports only the opening-arc chapter modules and reaches a local
`demo_end` route after DAY 4. The route offers replay and title return only.
The presentation resolves a separate demo-only scene catalogue and chapter
preview catalogue too, so later text and photographs are not merely hidden in
the player bundle. No store URL is embedded until a real store page is
configured.

The demo owns its save namespace (`umikaze-demo-v1`) and desktop identity
(`jp.example.umikaze.demo`). It neither reads nor clears the complete game's
`umikaze-v4` records. The desktop build uses
`src-tauri/tauri.demo.conf.json`, so an installed demo and full game can stay
side by side during playtesting or commercial release.

```sh
# Fast unsigned local build
npm --prefix examples/umikaze/ui run prepare:demo

# Signed static archive: dist/releases/demo-web
ARIA_PAK_PROFILE=signed \
ARIA_PAK_SIGNING_KEY='publisher:<64-byte-hex-key>' \
ARIA_PAK_VERIFICATION_KEY_ID=publisher \
ARIA_PAK_VERIFICATION_KEY_HEX='<32-byte-public-key-hex>' \
npm --prefix examples/umikaze/ui run release:demo:web

# Signed native installer. Default: NSIS on Windows, .deb on Linux, DMG on macOS.
npm --prefix examples/umikaze/ui run release:demo:desktop
```

Before publishing, CI runs the demo-only integration test with
`UMIKAZE_DEMO=true` against `prepare:demo`. That build also rejects known
post-DAY-4 preview text and scene filenames from the generated presentation
artifact. The normal presentation suite runs against the full bundle and
remains the regression gate for the complete game.

### GitHub Pages demo host

[`aria-web-pages.yml`](../../.github/workflows/aria-web-pages.yml) deploys the
signed demo's `site/` directory through GitHub Pages. It does not publish the
full game, source Markdown, or a faux store page. The frontend uses relative
paths, so the standard project URL
`https://mirinnano.github.io/aria-engine-rust/` is supported without a Vite base
override.

Enable **Settings → Pages → Source: GitHub Actions** once, and provide the
three PAK signing secrets used by the release workflow. GitHub Pages is an
appropriate free static host while the repository's visibility/plan permits
it; use a dedicated static host when custom cache headers, domain control, or
commercial analytics become necessary.
