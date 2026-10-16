# Aria Engine

Aria Engine は、ノベルゲームを書くためのスクリプト言語 Aria と、その実行環境である。
コンパイラは台本を構文・型・所有権の面から検査してバイトコードに変換し、VM がそれを Native と Web で同じように実行する。
同じ入力を与えれば、両者は同じ画面と同じ保存データを出す。

```aria
aria;
entry opening;

character mio { name "ミオ"; }

scene opening {
    background asset("assets/bg/shore.webp") with fade(300ms);
    say mio "海へ行こう。";
    await advance;
    choice {
        "堤防へ" => breakwater;
        "駅へ" => station;
    }
}

scene breakwater { narrate "波が足元でほどけた。"; await advance; end; }
scene station { narrate "発車ベルが遠くで鳴った。"; await advance; end; }
```

## 試す

リポジトリのルートで実行する。

```sh
# 検査（構文、型、所有権、制御フロー）
cargo run --locked -p aria-cli -- check examples/v3-minimal --release

# 画面を出さずに実行する
cargo run --locked -p aria-cli -- run examples/v3-minimal --headless

# Web 版を作る
./scripts/build-v3-web.sh examples/v3-minimal target/v3-web
```

## 何がどこにあるか

| 場所 | 中身 |
|---|---|
| `crates/aria-core` | 言語（構文解析、コンパイラ）、VM、保存、表示モデル |
| `crates/aria-cli` | `aria` コマンド |
| `crates/aria-native` | デスクトップ版のプレイヤー（Winit、wgpu、Kira） |
| `crates/aria-web` | Web 版のランタイム（Wasm） |
| `crates/aria-render` | 描画命令の実行 |
| `crates/aria-protection` | PAK の署名、暗号化、ライセンス |
| `crates/aria-plugin-sdk` | プラグインを書くための SDK |
| `crates/aria-plugin-host` | プラグインの読み込み、検証、実行 |
| `ui/packages/aria-ui-sdk` | 表示モデルの型（TypeScript） |
| `examples/` | 最小の作品と、プラグインの実例 |

`aria` コマンドは次の仕事を受け持つ。

- `check`：検査だけを行う。
- `run`：Native で実行する。`--headless` で画面なし、`--replay` で入力記録の再生。
- `build`：Windows、macOS、Linux、Web 向けの配布用データを作る。
- `package`：`build` の出力を ZIP やインストーラーにまとめる。
- `plugin pack` / `plugin inspect`：プラグインを署名する、中身を確かめる。
- `import-novel`：Markdown の原稿を Aria の台本に変換し、照合する。
- `bench`：VM の実行速度を測る。

## ドキュメント

- [Aria 言語仕様](docs/spec/aria.md)
- [台本と表示パッケージの分担](docs/spec/presentation.md)
- [プラグイン API](docs/spec/plugins.md)
- [PAK の配布形式](docs/spec/pak4.md)

## 版と配布

エンジンの版は `Cargo.toml` の `workspace.package.version` で、作品の版とは別に数える。
1.0 より前は、版が上がるたびに API が変わりうる。
`main` を更新するたびに、`aria` コマンドと Native プレイヤーの最新版を `engine-nightly` の Release に置く。

## ライセンス

MIT License
