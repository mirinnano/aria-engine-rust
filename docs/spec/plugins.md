# Rust/Wasm プラグイン API v1

Aria は、Rust で書いて `wasm32-unknown-unknown` 向けにビルドしたプラグインを、エンジンの再ビルドなしで読み込めます。
API v1 の拡張対象は、独自画面の入力、待機、文字表示ポリシーです。
描画、任意 opcode、本文の生成、実行中の hot reload、状態付き callback は対象外です。

ゲストの Rust コードは登録時に一度、ポリシーを記述する JSON を生成します。
Native と Web は同じ `aria-plugin-host` / wasmi ローダーでそのコードを実行し、検証済みデータだけを Core へ渡します。
Wasm instance は登録後に破棄し、物語の実行や履歴の再構成では不変のポリシーを使用します。
UI の DOM、配色、画像、フォントは作品側で実装します。
プラグインだけを置いても、独自画面の見た目は自動生成されません。

## 実例をビルドして検査する

リポジトリのルートで実行します。

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --locked -p aria-lecture-plugin --target wasm32-unknown-unknown
cargo run --locked -p aria-cli --no-default-features -- plugin inspect \
  target/wasm32-unknown-unknown/release/aria_lecture_plugin.wasm
```

[講義スライドのソース](../../examples/plugins/lecture/src/lib.rs)は `lecture_slide` と `lecture.next` を登録します。
[海風のソース](../../examples/umikaze/plugin/src/lib.rs)は `day_card`、`interlude`、`statement` を登録します。
これらは Core の標準画面ではありません。
サンプルにはビルド済みの Wasm も同梱しているため、通常の作品検査でゲストの再ビルドは必要ありません。
ソースを編集した場合は Wasm を再ビルドし、同梱ファイルも更新します。

```sh
cargo build --release --locked -p umikaze-story-plugin --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/umikaze_story_plugin.wasm \
  examples/umikaze/assets/plugins/umikaze-story.wasm
cargo run --locked -p aria-cli --no-default-features -- check examples/umikaze --release
```

## Rust で独自プラグインを書く

ライブラリを `cdylib` にし、`aria-plugin-sdk` を依存に追加します。
以下はワークスペース外の最小パッケージです。
SDK のパスは実際の checkout に合わせます。

```toml
[package]
name = "my-story-plugin"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
aria-plugin-sdk = { path = "/path/to/aria-engine-rust/crates/aria-plugin-sdk" }
```

```rust
use aria_plugin_sdk::{PLUGIN_API_VERSION, PluginManifest, RoutePolicy};

fn manifest() -> PluginManifest {
    PluginManifest {
        api_version: PLUGIN_API_VERSION,
        id: "org.example.my-story".into(),
        version: "1.0.0".into(),
        routes: vec![RoutePolicy {
            atomic_text: true,
            advance_releases_hold: true,
            advance_action: Some("slide.next".into()),
            ..RoutePolicy::reading("my_slide")
        }],
    }
}

aria_plugin_sdk::export_plugin!(manifest);
```

```sh
cargo build --release --target wasm32-unknown-unknown
```

Rust の trait object や ABI に依存せず、3 つの整数関数とゲストの線形メモリを境界に使います。
SDK の macro が次の exports を生成します。

| Export | 署名 | 用途 |
| --- | --- | --- |
| `aria_plugin_api_version` | `() -> i32` | API の互換性検査 |
| `aria_plugin_manifest_ptr` | `() -> i32` | ゲスト内 JSON の先頭位置 |
| `aria_plugin_manifest_len` | `() -> i32` | JSON のバイト数 |
| `memory` | Wasm memory | ホストが範囲を検査して読み取る領域 |

この API はポリシーの登録用です。
実行中に Rust の関数を毎フレーム呼び出す API ではありません。

## 作品へ登録する

Wasm を既存の `runtime.asset_roots` 配下へコピーします。
独自パッケージの例では成果物を次のように配置します。

```sh
mkdir -p /path/to/game/assets/plugins
cp target/wasm32-unknown-unknown/release/my_story_plugin.wasm \
  /path/to/game/assets/plugins/my-story.wasm
```

`PluginManifest` は SDK の macro が Wasm 内の JSON に埋め込みます。
別の manifest ファイルは不要で、プラグイン ID と登録ファイル名も一致する必要はありません。
プロジェクトは schema 5 にし、既存の `[runtime]` に `plugins` を加えます。
下記は追加する値を示す抜粋であり、プロジェクト全体ではありません。

```toml
schema = 5

[runtime]
plugins = ["assets/plugins/my-story.wasm"]
```

その後は通常の `aria check`、`aria run`、`aria build` を使用します。
CLI は、ABI、JSON、予約名、重複、実行量、メモリ制限を検査します。
Wasm は他の素材と同じ PAK に格納し、体験版などで参照素材を絞る場合も必須素材として残します。
plugin inventory の順序とパスは bundle の content root に含めます。
登録済みの独自画面をシナリオから指定できます。

```aria
aria;
entry opening;
scene opening {
    screen my_slide;
    narrate "この一枚を、読者の入力で送る。";
    wait 2000ms;
    screen dialogue;
    narrate "通常の本文へ戻る。";
    await advance;
    end;
}
```

### ホストを独自実装する場合

Native の CLI、Player、ReplayRunner は同じ登録を初期化時に引き継ぎます。
Web/Tauri の作品 bootstrap は、PAK から取り出したゲストの bytes を最初の `step` より前に渡します。
保存の検査用に一時 VM を作る場合も、同じゲストを登録します。
[海風の bootstrap](../../examples/umikaze/ui/src/runtime.ts)が実例です。

```javascript
const runtime = new WebRuntime(bytecode, 1280, 720);
// pluginBytes は検証済み PAK から取り出した Uint8Array。
runtime.register_plugin(pluginBytes);
const output = runtime.step(inputJson);
```

Rust の組み込みホストでは次の境界を使います。

```rust
let plugin = aria_plugin_host::load_plugin(&plugin_bytes)?;
let mut vm = aria_core::Vm::new(program, logical_size)?;
vm.register_plugin(plugin)?;
```

Core 自体は Wasm を解釈せず、ファイルを開きません。

## RoutePolicy

| フィールド | 既定値 | 作用 |
| --- | --- | --- |
| `route` | 必須 | 独自画面の識別子 |
| `reading_surface` | `true` | 待機を物語時間として進め、Cancel はメニューを開く |
| `atomic_text` | `false` | 現在の字幕ページをタイプライタなしで表示・履歴へ記録する |
| `playback_controls` | `false` | オートとスキップの action を公開する |
| `advance_releases_hold` | `false` | 通常の決定入力で現在の待機を解放する |
| `skip_releases_hold` | `false` | VM のスキップモードで現在の待機を解放する |
| `advance_action` | `null` | 独自の待機解放 action。上記の advance 許可が必要 |
| `auto_hold_cap_ms` | `null` | オート時の残待機時間の上限 |
| `first_visit_min_hold_ms` | `null` | この長さ以上の待機を first-visit として表示モデルへ渡す |

時間値を指定する場合、`auto_hold_cap_ms` と `first_visit_min_hold_ms` は `1..=60000` ms に限定します。
`reading_surface=false` の場合は、ほかの bool をすべて `false`、optional な時間値と action をすべて `null` にします。
読書ではない画面に文字表示・待機・再生ポリシーを混ぜる設定は拒否します。

first-visit は訪問履歴を自動的に数える仕組みではありません。
シナリオが初回と再訪で異なる待機を指定し、プラグインがその閾値を宣言します。
`atomic_text=true` で通常 advance を許可しない画面では、明示した待機を通常入力で短縮しません。
物理的に保持した Skip は既存の共通入力として待機を解放します。

標準の `dialogue`、`title`、`save` 等や、その別名は上書きできません。
独自 action は標準の `menu.*`、`chrome.*` 等を横取りできません。
route と action は小文字 ASCII の英数字、`.`、`_`、`-` に限定します。
複数プラグインの同じ ID、route、action の重複は、後勝ちにせず登録を拒否します。

表示モデル schema 9 の `story_surface` は、`reading_surface`、`atomic_text`、`advance_action`、`first_visit` を公開します。
作品 UI はこの値と通常の choices を使って、画面名を列挙せず入力を振り分けられます。

## 制限と保存互換

- ゲストの imports はすべて拒否します。WASI、ファイル、通信、時計、環境変数を提供しません。
- Wasm binary は最大 4 MiB、線形メモリは最大 8 MiB、JSON は最大 64 KiB、実行 fuel は登録全体で 5,000,000 です。
- fuel の対象外である検証・コンパイルにも wasmi の strict 構造制限を適用します。関数 10,000、globals 1,000、data/element segments 各 1,000、引数・戻り値各 32 が上限で、大量の極小関数も平均サイズ検査で拒否します。
- 同時登録は最大 16 plugins、全体で最大 64 routes です。table は 1 個・4096 elements、memory は 1 個に制限します。
- 不正な pointer、export の型、JSON、API version、無限ループ、メモリ拡張超過はエラーとして返します。
- 登録は最初の実行前だけです。起動後にポリシーを差し替えません。
- Snapshot schema 10 は、ゲスト bytes と検証済みポリシーの fingerprint を保存します。欠落・差替え・更新したプラグインで旧保存を開くと、明示的に拒否します。
- Schema 9 以前の保存も明示的に拒否します。今回の移行で保存ファイルを自動削除しません。
- Project schema 4 はプラグインなしで読み込めます。プラグインには schema 5 が必要です。旧 bundle schema 5 はプラグインなしに限って読み込み、新規 bundle は schema 6 です。

署名済み PAK は、従来の配布物と同じ署名検証をプラグイン bytes に適用します。
plugin inventory の選択と順序を含む bundle JSON は content-root のハッシュで結び付けますが、JSON 自体の発行者署名ではありません。
署名済みゲスト bytes の認証と、inventory 選択の発行者認証を同一視しないでください。
開発用の unsigned bundle のハッシュは破損検出であり、配布元の認証ではありません。
sandbox と配布元の信頼性も別の条件です。
信頼できない配布元のプラグインを、登録しただけで正しい作品ポリシーとみなすことはできません。

## 検証

[実ゲストの統合テスト](../../crates/aria-cli/tests/plugins.rs)は、独立した Rust/Wasm のスライドを Core と PortableWebRuntime に登録し、出力、snapshot、Native replay、保存互換、履歴の再構成を比較します。
[ローダーのテスト](../../crates/aria-plugin-host/src/lib.rs)は、imports、不正 JSON、範囲外メモリ、巨大出力、無限ループ、メモリ上限を検査します。
[海風の回帰テスト](../../crates/aria-cli/tests/umikaze_sample.rs)も、同梱したゲストを読み込んで実行します。

```sh
cargo test --locked -p aria-plugin-sdk -p aria-plugin-host
cargo test --locked -p aria-cli --no-default-features --test plugins --test umikaze_sample
cargo check --workspace --target wasm32-unknown-unknown --no-default-features --locked
```
