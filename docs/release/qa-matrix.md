# 海風 QA Matrix

この表は、現行の Aria v3 Core、React/Web presentation、Tauri desktop shell
にだけ適用する。旧NScripter/Raylib/C#経路や、存在しない外部リンク画面を
公開ゲートに含めない。

## 自動ゲート

| 区分 | 実行 | 合格条件 |
| --- | --- | --- |
| 整形 | `cargo fmt --all -- --check` | Rust差分が標準整形済み |
| Core | `cargo test -p aria-core` | 字幕分割、保存、履歴、設定、CG、断章を含む全テスト成功 |
| Scenario/CLI | `cargo test -p aria-cli --no-default-features --test umikaze_sample` | 通常版の章導線と体験版のDAY 0–4境界が成功 |
| 正本照合 | `aria import-novel … --verify` | `Desktop/Novel/src` とDay 0–10の本文・話者・順序が一致 |
| 通常Web | `npm run prepare:desktop` → `npm test` | タイトル、章扉、読書、RMenu、設定、保存、履歴、ギャラリーの25件以上が成功 |
| 体験版Web | `npm run prepare:demo` → `UMIKAZE_DEMO=true npm test -- --grep 'opening arc'` | DAY 4終端、DAY 5以降の不在、体験版保存名が確認できる |
| 静止性能 | 通常/体験版のPlaywright性能テスト | タイトルが継続的な`requestAnimationFrame`を要求しない |
| Native save | `cargo test --manifest-path examples/umikaze/ui/src-tauri/Cargo.toml` | 自動保存が手動スロットと別ディレクトリである |

Playwrightには公開済みの`dist/web`を配信するローカルHTTPサーバーが必要である。
CIでは`python3 -m http.server 4173 --bind 127.0.0.1 --directory examples/umikaze/dist/web`
をバックグラウンドで開始する。

## 手動リリース候補確認

| Area | 確認内容 |
| --- | --- |
| Windows | NSISの新規インストール・アンインストール・日本語パス・DPI 100/150%・WebView2未導入時の導入経路 |
| Linux | `.deb`または選択したAppImageの新規インストール・削除・Wayland/X11・WebKitGTKあり/なしの診断 |
| macOS | DMG起動・署名・notarization・Retina・アクセシビリティ権限なしでの読み上げ/キーボード操作 |
| Web | Chrome/Edge/Safari/iOS/Androidでの初回ロード、オフライン再起動、IndexedDB無効時の安全な保存失敗 |
| Input | クリック、Enter、Space、下スクロール、H、Escape、右クリック、ゲームパッドA/B/Y/D-pad、接続解除/再接続 |
| Reading | 120字超の日本語・英語・中国語、句読点境界、二行帯外なし、ページ完了後にだけ次入力で送る |
| Save | 手動1–10、自動保存、破損世代からの復旧、履歴OK/NG、既読/章/CG/設定の保持 |
| Editions | 完成版`umikaze-v4`と体験版`umikaze-demo-v1`を同一端末に共存させ、互いの保存を見ないこと |
| Performance | 低電力端末で静止タイトル/メニューがアイドル、文字送りP95、画像切替時の入力欠落なし |

## 画面キャプチャの基準点

- 通常版タイトル
- 章選択のDAY 1とDAY 7
- 日付カード
- 二行字幕（通常・長文・断章）
- 透明RMenu
- CONFIG（TEXT / SOUND / DISPLAY）
- SAVE / LOAD / LOG / EXTRA
- 体験版の`DEMO COMPLETE`

画面比較では文字のアンチエイリアス差を許容するが、操作対象の欠落、字幕帯外、
焦点の不可視、無意味なカード/ブラウザ標準UIの露出は不合格とする。
