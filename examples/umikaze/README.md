# 海風 Aria sample

This is the single-`aria;` semantic-runtime sample for the umikaze vertical
slice. The historical NScripter/C# sources are preserved in the separate
[`aria-engine` legacy repository](https://github.com/mirinnano/aria-engine);
they are not an executable compatibility path: `aria migrate` and the legacy
parser were removed. New scenario sources are compiled directly by the current
Aria front end.

The sample keeps the umikaze visual language (deep indigo, sea fog, pale
paper, restrained gold, quiet panels, chapter cards, and non-colour-only
focus states) and exposes the shared runtime features: a Japanese-first public
release flow; persistent chapter/CG progress; textbox and text-speed settings;
tween and screen effects; choices; save/load; menu; backlog; auto; and skip
input. The Japanese route is a hand-directed Aria scenario rather than a
one-line demo or an opaque prose import.

Its React presentation package at [`ui`](ui) separates story data from the
title rail, reading panel, right/bottom sheets, settings sliders, backlog,
chapter cards, and gallery. The package receives only the semantic view model
from the shared WASM runtime and is also embedded by the Tauri desktop shell;
there is no second native layout implementation. The text-free sea-fog,
paper-grain, wave-divider, and chapter-ornament raster assets complement the
existing seaside scene without baking any player-facing words into images.
It deliberately uses color backgrounds and vector-like rectangles, so the
project can be compiled and replayed without shipping the original artwork.
The bundled Noto Sans CJK JP face carries the reader and operational text,
while M PLUS 1 Code is reserved for compact record metadata. This keeps the
same project runnable on desktop and web without relying on a host font.
Both fonts are distributed under the SIL Open Font License; see
[`licenses/NotoSansCJKJP-OFL.txt`](licenses/NotoSansCJKJP-OFL.txt) and
[`licenses/MPLUS1Code-OFL.txt`](licenses/MPLUS1Code-OFL.txt).

## 日本語シナリオ

日本語版は [`scripts/scenario/ja-JP/index.aria`](scripts/scenario/ja-JP/index.aria)
を入口として、章ごとの `chapter-00.aria` から `chapter-10.aria` を実行する。各ファイルは
正典Markdownの対応する一章だけを埋め込んだ自己完結した生成物であり、ランタイムや
配布pakが `Desktop/Novel` を読むことはない。本文は合計2,419のプレイヤー可視ビートとして
そのまま移され、Day 0の視点区切り1箇所とDay 5の既存演出指示4件だけを、非言語の
画面転換へ翻訳している。

演出規則は意図的に小さい。各日の開始はプレイヤーが進める日付カード、原稿内の太字
日時は短い断章、単独の `...`／`……` は字幕を消した420msの沈黙になる。Day 5では
原稿の暗転・待機・フェードインを暗い保持と病室色への遷移として保ち、Day 10末尾だけは
暗い色をゆっくり残す。新しい地の文・台詞・事実は生成していない。

原文の更新を反映するときは、必ず本文レビュー後にDay 0〜10を明示指定する。まず
`--verify` で既存Ariaを**書き換えずに**検証する。この検証はMarkdownとAria ASTの
本文テキスト・話者・順序・件数を章ごとに比較し、日付カード、見出し断章、沈黙、Day 0／5／10の
演出規則も確認する。JSONの各章ビート数と `aria check` も併せて確認する。

```sh
cargo run -p aria-cli -- import-novel /path/to/Novel/src \
  --out examples/umikaze/scripts/scenario/ja-JP \
  --chapter-select chapter_select_ja --locale ja-JP \
  --include 00_init.md,01_start.md,02_day2.md,03_day3.md,04_day4.md,05_day5.md,06_day6.md,07_day7.md,08_day8.md,09_day9.md,10_day10.md \
  --presentation umikaze --layout chapters --verify
```

検証が通ったあとに原文更新を反映するときだけ、同じコマンドから `--verify` を外して
再生成する。日付カードの短い紹介文はナビゲーション用メタデータであり、本文比較の対象外。
地の文・台詞・見出しは比較対象に含まれる。原稿の章境界とAriaファイル境界を一致させるため、
Day 11以降の草稿をこのコマンドの `--include` に加えてはいけない。

DAY 11–13は[`docs/story-map.md`](docs/story-map.md)で設計中であり、旧DAY 14と
後日談は公開経路から外した草稿として[`docs/drafts/`](docs/drafts)に保管する。
本文レビューの判断は[`docs/manuscript-review.md`](docs/manuscript-review.md)に残す。
公開時のコンテンツ注意はゲーム内には表示せず、ストアページで成人向け心理劇・
病気・家族関係・逃走を扱う旨を明示する。

この改稿版は保存名を`umikaze-v4`へ更新する。初回起動時には同作の
`umikaze-v3`だけを消去するため、旧版の進行・設定・自動保存は引き継がれない。
ほかのゲームや保存名前空間には影響しない。

## 体験版

体験版は、完成版に停止フラグを足したものではない。専用入口
[`scripts/main-demo.aria`](scripts/main-demo.aria)から、DAY 0–4の章モジュールだけを
コンパイルする独立した読書経路である。DAY 4の読後は静かな終端画面へ入り、
「もう一度読む」か「タイトルへ戻る」だけを選べる。DAY 5以降の本文・選択肢・画像を
体験版PAKへ含めない。さらに表示用フロントエンドもDAY 0–4用の章プレビューと場面写真
だけを解決するため、先の内容がブラウザやデスクトップの展開物から読める形では残らない。

保存は完成版の`umikaze-v4`と分離した`umikaze-demo-v1`を使う。体験版の初回起動は
完成版の保存を削除も移行もしない。Tauri版もアプリIDを`jp.example.umikaze.demo`として
分け、Linuxの実行ファイル名も`umikaze-demo`として、インストール済みの完成版を
上書きしない。

開発用の体験版を作るには次を実行する。

```sh
npm --prefix examples/umikaze/ui run prepare:demo
```

デスクトップ版を体験版のまま試すときは、専用のTauri設定を使う。

```sh
npm --prefix examples/umikaze/ui run tauri:demo
```

署名済みの配布物は、通常版と同じCI上のPAK署名鍵を使い、明示的に体験版モードを選ぶ。

```sh
ARIA_PAK_PROFILE=signed \
ARIA_PAK_SIGNING_KEY='publisher:<32-byte-secret-as-64-hex-characters>' \
ARIA_PAK_VERIFICATION_KEY_ID=publisher \
ARIA_PAK_VERIFICATION_KEY_HEX='<32-byte-public-key-as-64-hex-characters>' \
npm --prefix examples/umikaze/ui run release:demo:web

# 各ホストでの既定形式: Windows=NSIS / Linux=AppImage / macOS=dmg
npm --prefix examples/umikaze/ui run release:demo:desktop
```

署名用の秘密値と検証用公開値は、ともに32バイトを16進64文字で表したものを使う。
秘密値は将来の更新署名にも必要になるため、復旧可能な秘密管理へ保管し、リポジトリや
CIログへは出さない。

### GitHub Pages

無料の静的体験版ホストにはGitHub Pagesを使える。このリポジトリでは
[`aria-web-pages.yml`](../../.github/workflows/aria-web-pages.yml)が`main`への対象変更時と
手動実行時に、署名済みの体験版だけを`dist/releases/demo-web/site`から公開する。
Vite/PWAは相対URLで生成されるため、標準の
`https://mirinnano.github.io/aria-engine-rust/`のようなプロジェクト配下URLでも動作する。

最初の一回だけ、GitHubリポジトリの **Settings → Pages → Source** を **GitHub Actions** に
設定する。公開ワークフローには`ARIA_PAK_SIGNING_KEY`、
`ARIA_PAK_VERIFICATION_KEY_ID`、`ARIA_PAK_VERIFICATION_KEY_HEX`のRepository Secretsが必要である。
秘密鍵を置けないforkやローカルからは公開せず、`prepare:demo`での未署名プレイテストを使う。

GitHub Pagesは静的配信だけを担う。ダウンロード販売、年齢認証、サーバー側分析、Steam連携、
独自のキャッシュヘッダが必要になった時点で、体験版artifactをCloudflare Pages/R2や販売先へ
そのまま移せるようにしてある。

## Run

Build the browser package from the repository root:

```sh
cargo build -p aria-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir target/aria-web-runtime-local --out-name aria_web \
  target/wasm32-unknown-unknown/debug/aria_web.wasm
ARIA_WEB_RUNTIME_DIR=target/aria-web-runtime-local \
  cargo run -p aria-cli -- build examples/umikaze --target web --out target/umikaze-web
```

To run the desktop shell, install the operating system dependencies required
by Tauri/WebKit first, then run:

```sh
cd examples/umikaze/ui
npm install
npm run tauri -- dev
```

On this Gentoo-based development host, the missing packages are provided by
`net-libs/webkit-gtk:4.1` (which supplies `webkit2gtk-4.1.pc` and
`javascriptcoregtk-4.1.pc`). The package may bring a substantial dependency
set, so it is intentionally not installed by the project build scripts.
