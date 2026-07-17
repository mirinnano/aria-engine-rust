# V3 compatibility corpus

This directory contains platform-neutral fixtures for the Rust-native Aria V3
runtime.

- `vertical-slice-inputs.json` is a green replay tape. The Rust test suite runs
  it through the Native and Web wrappers and requires identical VM, render,
  audio, UI, and runtime-command hashes.
- `umikaze-legacy/` is the unchanged source snapshot used as migration input;
  it includes the original `main.aria`, all UI helpers, and every localized
  scenario file.
- A behavior becomes a V3 contract only after it is represented by a
  platform-neutral source/replay fixture and the cross-runtime test is green.

Legacy projects are migrated into this repository with `aria migrate`; the
legacy runtime and its test inventory are intentionally kept outside this
Rust-only repository.
