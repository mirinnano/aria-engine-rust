# Aria Engine ドキュメント

## 現行の入口

- [Aria 作者言語](spec/aria.md)：単一の `aria;` 構文と意味的な画面契約。
- [Rust/Wasm プラグイン API v1](spec/plugins.md)：SDK、登録、制限、保存互換、実例。
- [海風の実行方法](../examples/umikaze/README.md)：作品側の React/WASM/Tauri presentation。
- [PAK4 の配布契約](spec/pak4.md)：署名・保護・素材パッケージ。

## 歴史資料

次の資料は以前の設計を含みます。
ARIAC4、旧作者構文、visual UI DSL、移行コマンドなどを、現行実装の操作手順として使わないでください。
現行のプラグイン・表示・保存 schema は上記の現行資料を参照します。

- [Aria 3.1](spec/aria-3.1.md)
- [V3 ランタイム初期仕様](spec/aria-v3-runtime.md)
- [Native-first 初期設計](architecture/v3-native-first.md)
- [V1/V2 移行記録](development/v3-migration.md)
- [Asset opcode の資料](reference/opcodes/asset.md)
