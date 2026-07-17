# アセットコマンドリファレンス

## `asset_preload`

指定したWebアセットグループが利用可能になるまでスクリプト実行を停止します。

```aria
*scenario_01
asset_preload "scenario_01"
scenechange("prologue_rooftop", 2)
```

引数は英数字、`.`、`_`、`-` からなるグループ名です。Native版ではディスクまたはPakがすでに利用可能なため即時完了します。Raylib WASM版では `aria-web-assets.json` の同名グループをHTTP取得し、サイズとSHA-256を検証して仮想ファイルシステムへ配置してから実行を再開します。

取得に失敗した場合、VMは `WaitingForAssetGroup` のまま進みません。ランタイムのエラー画面でクリック、`R`、`Enter` のいずれかを入力すると同じグループを再試行します。

## `load_aria_asset`

単一アセットをバイト列として読み込み、アセットハンドルへ格納します。

```aria
owned asset @chapter_data
load_aria_asset "assets/data/chapter.bin", @chapter_data, owned
```

`asset_preload` は配布グループの可用性を保証する命令、`load_aria_asset` は利用可能な単一ファイルをVM内へ読み込む命令です。
