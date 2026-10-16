# Aria Engine の作業メモ

このリポジトリはエンジンだけを持つ。作品（HesPhoros）は別のリポジトリにあり、このリポジトリをサブモジュールとして使う。

## 変更したら通すもの

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check --workspace --target wasm32-unknown-unknown --no-default-features --locked
cargo run --locked -p aria-cli -- check examples/v3-minimal --release
```

Web 版のビルドを含むテストには、`Cargo.lock` と同じ版の `wasm-bindgen` CLI（0.2.126）が要る。

## 境界

- `aria-core` は言語、VM、保存、表示モデルの意味を決める。描画や入出力の具体は持たない。
- `aria-native` と `aria-web` は、同じ VM を各環境で動かす薄い層である。両者は同じ入力に同じ出力を返す。`cross_runtime_determinism` のテストがこれを確かめる。
- プラグインは Wasm で動き、Core はプラグインを呼ぶ口（`PluginExecutor`）だけを持つ。
- 作品の資料（本文、画像、作品固有の表）をこのリポジトリに置かない。

## 保存データの形を変えるとき

`VM_SNAPSHOT_SCHEMA`、`UI_VIEW_MODEL_SCHEMA`、`ARIAC_VM_ABI_VERSION` のいずれかを変えたら、`cross_runtime_determinism` の固定値も変わる。
固定値は、同じテストが Native と Web の一致を確かめたあとでだけ更新する。
