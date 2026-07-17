# AriaEngine V3.1 native-first architecture

V3.1 is a Rust visual-novel runtime, not a general-purpose engine. Native is
the quality reference; Web compiles the same `aria-core` VM to WASM.

```text
.aria + aria.toml
       |
       v
aria-core: lossless syntax -> typed compiler -> ARIAC4 -> deterministic VM
       |                         |
       v                         v
aria-native: Winit/wgpu/Kira   aria-web: WASM/PWA/WebGPU/WebGL2/IDB
       \_________________________|
                    aria-render: protocol renderer, Taffy, cosmic-text/glyphon
                         |
                    aria-cli: check/run/build/migrate
```

`aria-core` has no OS, GPU, audio, browser, clock, or filesystem dependency.
Its boundaries are value-only `InputSnapshot`, `RenderFrame`, `AudioCommand`,
`UiTree`, `RuntimeCommand`, and `SaveEnvelopeV3`. An architecture test rejects
platform/device types from Core. A replay tape is the cross-runtime oracle:
Native and Web must produce the same per-frame and final-state BLAKE3 hashes.

Native translates keyboard, mouse, touch, accessibility, and controller
events into the same `InputAction` vocabulary. The desktop Player polls gilrs
on every frame; gilrs gives Windows WGI and Linux evdev the same standard
button/axis names and hot-plug handling. Steam Input's standard-controller
emulation is consumed through that same OS gamepad path when Steam exposes it;
direct vendor SDK types never cross the adapter boundary. The deterministic
Core never polls a device or depends on a vendor SDK: the adapter emits only
`RawInputEvent` values, so a Steam Input host can still feed the same seam
without changing the Player or game bytecode. Winit owns window, DPI,
resize, focus-loss, and letterbox conversion. wgpu uses the host's
Direct3D 12, Metal, Vulkan, or GLES-compatible backend through the adapter;
presentation mode is FIFO and frame delta is clamped to 250 ms. AccessKit
receives the same logical UI tree used for hit testing. Kira audio receives
only validated PAK asset bytes and follows the Core play/stop/loop/fade/volume
contract.

Text uses only the ordered font bytes named by `runtime.fonts`. Native parses
them into a bundled-only cosmic-text/fontdb database and glyphon draws the
result. Web loads the same bytes through `FontFace` before the first frame and
uses generated family names. Neither side performs host font discovery.
Aria text helpers operate on grapheme clusters and implement VN typewriter
and basic Japanese line-break rules; full visual goldens remain release QA.

Web is a thin PWA shell: service-worker precaching, update notification,
keyboard/pointer focus, standard Gamepad API polling, audio unlock, bundled
fonts, and IndexedDB generation storage. WebGPU is preferred and WebGL2 is a
complete fallback. Device/context
loss clears GPU resources and reloads them from PAK bytes. React and a second
game runtime are not included.

PAK4 and ARIAC4 are target-independent. Native release wrappers are validated
against PE/ELF/Mach-O headers, while Web release packaging requires real
wasm-bindgen glue and a WebAssembly header. A package containing a debug Player,
fake Web glue, a missing bundled font, a mismatched checksum, or a stale
manifest is rejected before launch.

The 1.0 cutover gate is: Windows/Linux release Players with the same portable
bundle, macOS universal artifact evidence, Chromium Web smoke, deterministic
replay, save corruption recovery, migrated game playthrough, and supply-chain
checks. The legacy C#/Raylib runtime is retained until those artifacts and
behavior comparisons are independently green.
