# 海風 Production Checklist

公開前には、別リポジトリに保存した旧C#実装やPowerShell配布スクリプトではなく、以下の現行経路を通す。

## 必須のローカル/CIゲート

```sh
cargo fmt --all -- --check
cargo test -p aria-core
cargo test -p aria-cli --no-default-features --test umikaze_sample
cargo test --manifest-path examples/umikaze/ui/src-tauri/Cargo.toml

# 本文正本照合（Day 0–10を明示指定）
cargo run --release --no-default-features -p aria-cli -- import-novel /path/to/Novel/src \
  --out examples/umikaze/scripts/scenario/ja-JP \
  --chapter-select chapter_select_ja --locale ja-JP \
  --include 00_init.md,01_start.md,02_day2.md,03_day3.md,04_day4.md,05_day5.md,06_day6.md,07_day7.md,08_day8.md,09_day9.md,10_day10.md \
  --presentation umikaze --layout chapters --verify
```

通常版を作り、`dist/web`をローカル配信した状態でPlaywrightを実行する。

```sh
npm --prefix examples/umikaze/ui run prepare:desktop
python3 -m http.server 4173 --bind 127.0.0.1 --directory examples/umikaze/dist/web &
npm --prefix examples/umikaze/ui test
```

体験版は同じ出力先を意図的に入れ替えて、専用テストを通す。その後、開発者に渡す前は
通常版を再生成しておく。

```sh
npm --prefix examples/umikaze/ui run prepare:demo
UMIKAZE_DEMO=true npm --prefix examples/umikaze/ui test -- --grep 'opening arc'
npm --prefix examples/umikaze/ui run prepare:desktop
```

## 署名済み配布物

CIで次の環境変数を提供できることを確認する。値そのものをログ/リポジトリへ出してはならない。

- `ARIA_PAK_PROFILE=signed`
- `ARIA_PAK_SIGNING_KEY`
- `ARIA_PAK_VERIFICATION_KEY_ID`
- `ARIA_PAK_VERIFICATION_KEY_HEX`

```sh
npm --prefix examples/umikaze/ui run release:web
npm --prefix examples/umikaze/ui run release:demo:web

# 各OSで実行。既定はWindows=NSIS / Linux=deb / macOS=DMG。
npm --prefix examples/umikaze/ui run release:desktop
npm --prefix examples/umikaze/ui run release:demo:desktop
```

Windowsコード署名、macOS notarization、Linuxパッケージの署名、Steam Depot uploadは
配布プラットフォーム固有の追加ゲートである。PAK署名だけでそれらを代替しない。

## 公開直前の確認

- [ ] 通常版artifactは`umikaze-v4`、体験版artifactは`umikaze-demo-v1`である。
- [ ] 体験版の`game.ariac`/PAK/source mapにDAY 5–10が含まれない。
- [ ] 体験版はDAY 4後に`demo_end`へ到達し、再読/タイトル帰還だけを提示する。
- [ ] 署名済みPAKとchecksumの検証が成功する。
- [ ] NSIS/DEB/DMGは各対象OSで新規インストール・起動・削除できる。
- [ ] 手動保存、自動保存、破損世代回復、履歴復帰、設定、CG解放を実機で確認した。
- [ ] 右クリック/Escape/H/Enter/Space/スクロール/ゲームパッドの操作を確認した。
- [ ] 静止タイトルで継続描画がなく、長文が字幕帯を越えない。
- [ ] ストアの年齢・コンテンツ注意、返金/サポート窓口、ライセンス表記を承認済みである。

## 公開ブロッカー

- Canonical本文照合失敗、コンパイル失敗、保存破損、起動不能。
- DAY 5以降が体験版artifactへ混入。
- 完成版と体験版が保存またはアプリIDを共有。
- 署名が必要な公開候補が未署名、またはchecksumがない。
- 字幕帯外、見えない焦点、ブラウザ標準メニュー、連続アイドル描画など没入を壊すUI回帰。
