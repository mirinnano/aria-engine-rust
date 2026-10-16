# プラグイン API

プラグインは、Rust で書いて `wasm32-unknown-unknown` 向けにビルドした拡張で、署名した一つの `.ariaplugin` ファイルとして配る。
エンジンや PAK を作り直さずに、プラグインのファイルを置き換えるだけで更新できる。
プラグインにできることは次の五つで、どれを使うかをマニフェストの権限（capability）で宣言する。

| 権限 | できること |
|---|---|
| `routes` | 独自画面を登録し、その入力と待ちの方針を決める |
| `commands` | 台本から `名前空間::命令(引数)` で呼べる命令を提供する |
| `hooks` | 本文の表示、選択肢の提示、場面への進入に介入する |
| `state` | 台本の変数とフラグを読み書きする |
| `render` | 自分の描画層を持つ。Web では UI モジュールも同梱できる |

プラグインには、ファイル、通信、時計、環境変数を渡さない。
プラグインが受け取るのは、エンジンが呼ぶたびに渡す要求（JSON）だけである。

## 作品への組み込み

### 置き場所と読み込み

ゲームは起動時に、次のフォルダにある `.ariaplugin` をすべて読み、署名を検証する。

1. 作品（または配布物）の `plugins/`
2. ユーザーデータの `plugins/`（Native プレイヤーでは `<保存先>/plugins/<ゲーム ID>/`）

ファイル名は `aria.toml` に書かない。
同じ ID のプラグインが複数あれば、署名が正しく `require` の範囲に入るもののうち、版がいちばん新しいものを使う。
更新したプラグインをユーザーデータに置くのは、macOS ではアプリの中身を書き換えるとコード署名が壊れて起動できなくなるからである。
Web 版はフォルダの中身を一覧できないので、ビルドが `plugins/index.json` にファイル名を書き出し、Service Worker は `plugins/` をネットワークから先に取得する。

### aria.toml

```toml
schema = 6

[plugins]
trusted_keys = { publisher = "<Ed25519 公開鍵を 16 進 64 文字で>" }
require = { "org.example.story" = "^1" }
```

`trusted_keys` は、信頼する署名鍵の ID と公開鍵である。
これ以外の鍵で署名されたファイルや、署名のあとに書き換えられたファイルは、理由を表示して読み込まない。
`require` は、作品が動くのに必要なプラグインと、その版の範囲（semver）である。
範囲を満たすプラグインが一つもなければ、ゲームは起動しない。

### 署名とパッケージ

```sh
cargo build --release --target wasm32-unknown-unknown
ARIA_PAK_SIGNING_KEY='publisher:<秘密鍵を 16 進 64 文字で>' \
  aria plugin pack target/wasm32-unknown-unknown/release/my_story_plugin.wasm \
  --out plugins/org.example.story.ariaplugin
aria plugin inspect plugins/org.example.story.ariaplugin --project .
```

`aria plugin pack` は、Wasm と、省略できる Web 用 UI モジュール（`--ui`、`ui.js` として同梱）と素材（`--assets`、`assets/` 以下に同梱）をまとめ、Ed25519 で署名する。
署名鍵は PAK と同じ発行者鍵を使える。
`aria plugin inspect` は `.wasm` と `.ariaplugin` のどちらも読み、宣言している権限、命令、画面を表示する。

## プラグインを書く

ライブラリを `cdylib` にし、`aria-plugin-sdk` を依存に加える。
`export_plugin!(manifest)` は登録だけをするプラグインを、`export_plugin!(manifest, handle)` は実行中に呼ばれるプラグインを書き出す。

```rust
use aria_plugin_sdk::{
    Capability, CommandDecl, PLUGIN_API_VERSION, PluginManifest, PluginRequest, PluginResponse,
    PluginValue, ValueType,
};

pub fn manifest() -> PluginManifest {
    PluginManifest {
        api_version: PLUGIN_API_VERSION,
        id: "org.example.tally".into(),
        version: "1.0.0".into(),
        state_version: 1,
        migrates_from: None,
        capabilities: [Capability::Commands].into(),
        namespace: Some("tally".into()),
        commands: vec![CommandDecl {
            name: "add".into(),
            params: vec![ValueType::Int],
            returns: Some(ValueType::Int),
        }],
        routes: Vec::new(),
        hooks: Default::default(),
    }
}

pub fn handle(request: PluginRequest) -> Result<PluginResponse, String> {
    match request {
        PluginRequest::Command { command, args, state, .. } if command == "add" => {
            let total = state.get("total").and_then(|v| v.as_i64()).unwrap_or(0);
            let [PluginValue::Int(amount)] = args.as_slice() else {
                return Err("add takes one Int".into());
            };
            Ok(PluginResponse {
                result: Some(PluginValue::Int(total + amount)),
                state: Some(serde_json::json!({ "total": total + amount })),
                ..PluginResponse::default()
            })
        }
        _ => Err("unsupported request".into()),
    }
}

aria_plugin_sdk::export_plugin!(manifest, handle);
```

完全な例は [`examples/plugins/tally`](../../examples/plugins/tally/src/lib.rs)（命令、状態、移行）と [`examples/plugins/annotate`](../../examples/plugins/annotate/src/lib.rs)（三種類のフック、描画層）にある。

### マニフェスト

| 項目 | 意味 |
|---|---|
| `id` | プラグインの識別子。保存データに記録されるので、公開後は変えない |
| `version` | semver の版。作品は `require` でこの範囲を指定する |
| `state_version` | 保存に入るプラグインの状態の版。状態の形を変えたときだけ上げる |
| `migrates_from` | 移行できる最も古い `state_version` |
| `capabilities` | 使う権限 |
| `namespace`、`commands` | 台本から呼ぶときの名前空間と、命令の宣言 |
| `hooks` | 介入する出来事（`text`、`choices`、`scene_enter`） |
| `routes` | 独自画面の方針（後述） |

宣言と権限は一致していなければならない。
たとえば `commands` を持つのに `commands` 権限がない、あるいは権限があるのに命令がないマニフェストは、読み込みの段階で拒否される。

## 呼び出しの仕組み

エンジンは、命令やフックを実行するたびに、プラグインの Wasm を新しく実体化し、要求の JSON を一つ渡して応答の JSON を一つ受け取る。
一回の呼び出しには実行量（fuel）とメモリの上限がある。
前の呼び出しの値はプラグインのメモリに残らないので、同じ要求には同じ応答が返り、Native と Web で同じ結果になる。
プラグインが覚えておきたい値は、応答の `state` で返す。
エンジンはそれを保存データに入れ、次の要求の `state` で渡し直す。

要求の種類と、応答で返せる項目は次のとおりである。

| 要求 | 届くとき | 応答で返せるもの |
|---|---|---|
| `command` | 台本が命令を呼んだ | 戻り値、状態、物語の変更、描画層 |
| `text` | 本文を一行表示する直前 | 差し替える本文と話者、状態、物語の変更、描画層 |
| `choices` | 選択肢を提示する直前 | 選択肢の並び（残す、除く、並べ替える、ラベルを変える、既存の場面への選択肢を足す）、状態、物語の変更、描画層 |
| `scene_enter` | 場面の先頭に来た | 状態、物語の変更、描画層 |
| `migrate` | 古い `state_version` の保存を開く | 新しい形の状態 |

`state` 権限を持つプラグインは、要求の `story` で台本の `state` 変数とフラグを受け取り、応答の `story` で書き換えられる。
台本が宣言していない変数や、型の違う値を書こうとすると、その応答は何も反映されずにエラーになる。

`text` フックで本文を書き換えても、既読の判定と改稿時の位置合わせには、台本に書かれた行を使う。
同じ出来事に複数のプラグインが介入する場合は、プラグイン ID の辞書順に適用する。

## 描画層

`render` 権限を持つプラグインは、どの応答でも `draw` に自分の描画層を丸ごと返せる。
描画層は矩形、画像（作品の素材のパス）、文字の並びで、最大 256 個である。
エンジンは描画層を保存データに入れ、毎フレームの描画に重ねるので、プラグインを毎フレーム呼ぶ必要はない。
空の並びを返すと描画層は消える。

Web 版とデスクトップ版（WebView）では、プラグインに同梱した `ui.js` を表示パッケージが読み込める。
表示モデルの `route_plugin` が、いまの画面を登録したプラグインを示すので、表示パッケージはそのプラグインの UI モジュールに画面を任せられる。
UI モジュールは表示パッケージと同じ権限で動くので、署名の検証を通ったものだけが読み込まれる。

## 独自画面の方針（RoutePolicy）

| 項目 | 既定値 | 作用 |
|---|---|---|
| `route` | 必須 | 独自画面の名前 |
| `reading_surface` | `true` | 待ちを物語の時間として進め、戻る操作でメニューを開く |
| `atomic_text` | `false` | 本文をタイプライタなしで一度に表示し、履歴に残す |
| `playback_controls` | `false` | オートとスキップの操作を出す |
| `advance_releases_hold` | `false` | 通常の決定入力で、いまの待ちを切り上げる |
| `skip_releases_hold` | `false` | スキップ中は、いまの待ちを切り上げる |
| `advance_action` | なし | 待ちを切り上げる独自の操作名。`advance_releases_hold` が要る |
| `auto_hold_cap_ms` | なし | オート時の待ちの上限 |
| `first_visit_min_hold_ms` | なし | この長さ以上の待ちを、表示モデルで初回の訪問として示す |

時間の値は 1 から 60000 ミリ秒の範囲で指定する。
`reading_surface` が `false` の画面には、文字表示、待ち、再生の方針を混ぜられない。
`dialogue`、`title`、`save` などの標準の画面名と、`menu.`、`chrome.` などで始まる標準の操作名は上書きできない。
複数のプラグインで画面名、操作名、名前空間が重なると、後から読んだ側を採用せずに登録を拒否する。

## 保存の互換

保存データには、プラグインごとに ID、版、`state_version` を記録する。

- `state_version` が同じなら、プラグインの中身が変わっていても保存を開ける。
- 保存の `state_version` が古く、新しいプラグインの `migrates_from` 以上なら、`migrate` 要求で状態を変換してから開く。
- 保存に記録されたプラグインがない、または移行できない場合だけ、開くのを拒否する。

保存のあとに加わったプラグインは、状態を持たない状態で始まる。

## 制限

- Wasm は最大 4 MiB、`.ariaplugin` は最大 16 MiB、線形メモリは最大 8 MiB、要求と応答はそれぞれ最大 1 MiB である。
- プラグインは import を一つも持てない。
- 同時に読み込めるのは 16 個まで、独自画面は全体で 64 個まで、命令はプラグインごとに 64 個までである。
- 署名は、プラグインのファイルが改ざんされていないことと、信頼する発行者が作ったことを保証する。プラグインの振る舞いが作品にとって正しいことまでは保証しないので、信頼する鍵は作品の発行者自身のものに限る。

## 検証

```sh
cargo test --locked -p aria-plugin-sdk -p aria-plugin-host
cargo test --locked -p aria-cli --test plugins --test plugin_commands --test plugin_hooks
```

統合テストは、実例のプラグインを Native と Web の両方で動かし、出力、保存、移行が一致することを確かめる。
