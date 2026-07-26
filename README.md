# Aria Engine

Aria Engine は、物語のための Rust 風・所有権対応スクリプト言語 **Aria** と、
Native/Web で同じバイトコードを実行する決定論的ランタイムです。

新規作品の作者言語は一つだけです。

```aria
aria;
```

互換モード、`strict` モード、言語バージョン選択、NScripter 命令の実行経路はありません。
仕様、サンプル、CLI、Native Player、Web runtime はこの構文だけを扱います。

## はじめる

```bash
# 構文・型・所有権・制御フローを検査
cargo run --locked -p aria-cli -- check examples/umikaze --release

# ヘッドレス実行
cargo run --locked -p aria-cli -- run examples/umikaze --headless

# Web bundle を作成
cargo run --locked -p aria-cli -- build examples/umikaze --target web --out target/umikaze-web
```

実行可能なプレゼンテーション例は [`examples/umikaze`](examples/umikaze/README.md) にあります。

## Aria の例

```aria
aria;

entry opening;
state mut route: Int = 0;

scene opening {
    background asset("assets/bg/shore.webp") with fade(300ms);
    let mut mio = show image(asset("assets/ch/mio.webp")) at (760px, 86px) z 20;

    say Mio: "海へ行こう。";
    await advance;

    borrow mut mio as portrait {
        move &mut portrait to (720px, 86px);
    }

    choice {
        "堤防へ" => breakwater;
        "駅へ" => station;
    }
}

scene breakwater { narrate "波が足元でほどけた。"; end; }
scene station { narrate "発車ベルが遠くで鳴った。"; end; }
```

`Node` は GC 任せではありません。 `show` で作られた Node は一つの所有者を持ち、
`drop`、所有権移動、または字句スコープ終了で決定的に一度だけ解放されます。Node を
変形する操作には `&mut`、借用には `borrow` が必要です。

## 仕様とツール

- [Aria 言語仕様](docs/spec/aria.md) — 構文、型、所有権、借用、診断、非互換
- [ランタイム・ファイル形式](docs/spec/aria-v3-runtime.md) — ARIAC7、pak、bundle、保存
- [Umikaze の実行方法](examples/umikaze/README.md#run) — React/WASM と Tauri shell
- [ドキュメント一覧](docs/README.md)

CLI は以下を提供します。

- `aria check` — 解析、型検査、所有権検査、制御フロー検査
- `aria run` — Native runtime または headless replay
- `aria build` — Windows/Linux/macOS/Web 用の player data bundle
- `aria import-novel` — Markdown 章を `aria;` ライブラリソースへ変換
- `aria bench` — VM hot loop の計測

## 設計上の境界

- Aria source は物語状態、演出、Node 資源、意味的な `screen` 遷移を所有します。
- React/Tauri/Web の presentation package はレイアウト、配色、フォント、アクセシビリティ、
  入力表示を所有します。
- コンパイラは ARIAC7 を生成します。旧 `.ariac`、互換 opcode、実行時言語モードは受理しません。

歴史的な C# 実装と旧仕様書は
[`aria-engine`](https://github.com/mirinnano/aria-engine) に保存されています。
このリポジトリは Rust 実装だけを公開・配布します。新規コードとドキュメントは必ず
[Aria 言語仕様](docs/spec/aria.md) を正としてください。

## ライセンス

MIT License
