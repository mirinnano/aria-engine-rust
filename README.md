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
- [Rust/Wasm プラグイン API v1](docs/spec/plugins.md) — 独自画面ポリシー、SDK、後読み込み、保存互換
- [PAK4 配布契約](docs/spec/pak4.md) — 署名・保護・素材パッケージ
- [旧 V3 ランタイム資料](docs/spec/aria-v3-runtime.md) — 歴史資料（現行の操作手順ではありません）
- [Umikaze の実行方法](examples/umikaze/README.md#run) — React/WASM と Tauri shell
- [ドキュメント一覧](docs/README.md)

CLI は以下を提供します。

- `aria check` — 解析、型検査、所有権検査、制御フロー検査
- `aria run` — Native runtime または headless replay
- `aria build` — Windows/Linux/macOS/Web 用の player data bundle
- `aria import-novel` — Markdown 章を `aria;` ライブラリソースへ変換
- `aria bench` — VM hot loop の計測
- `aria plugin inspect` — Rust/Wasm プラグインの制限付き実行と契約検査

## 設計上の境界

- Aria source は物語状態、演出、Node 資源、意味的な `screen` 遷移を所有します。
- React/Tauri/Web の presentation package はレイアウト、配色、フォント、アクセシビリティ、
  入力表示を所有します。
- コンパイラは ARIAC7 を生成します。旧 `.ariac`、互換 opcode、実行時言語モードは受理しません。
- 独自画面の入力・待機ポリシーは Rust/Wasm plugin として登録できます。Native/Web 共通の sandbox で起動時に検証し、Core は検証済みデータだけを実行します。任意 opcode や実行中 hot reload は API v1 の対象外です。
- 新規 project は schema 5、bundle は schema 6、UI view model は schema 9、snapshot は schema 10 です。プラグインを変更した保存と旧 snapshot は明示的に拒否し、今回の移行で保存を自動削除しません。

歴史的な C# 実装と旧仕様は廃止済みです。このリポジトリが公開・配布する
Rust 実装だけを正本とします。新規コードとドキュメントは必ず
[Aria 言語仕様](docs/spec/aria.md) を正としてください。

## ライセンス

MIT License
