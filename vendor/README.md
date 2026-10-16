# 手元に置いた依存クレート

`wayland-scanner` は、crates.io で公開されている 0.31.10 の写しである。
変更点は、依存する `quick-xml` を 0.41.0 に上げたことだけである。
公開版が依存する quick-xml 0.39 系には RustSec 2026 の XML パーサの勧告が出ている。
このクレートが読むのはコンパイル時に渡す Wayland のプロトコル定義（信頼できる XML）だけだが、勧告の出た版を依存に残さないために手元で直している。

quick-xml 0.41 系に依存する wayland-scanner が crates.io に公開されたら、このディレクトリとワークスペースの `[patch.crates-io]` を削除する。
