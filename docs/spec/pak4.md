# PAK の配布形式（ARIAPAK4）

作品の素材（画像、音声、フォント）は、拡張子 `.ariapak` のパッケージに入れて配る。
一つの作品は複数のパッケージを持てる。
各パッケージは役割（`boot`、`hot`、`cold`、`overlay`）を持ち、起動に要るものだけを先に読み、残りは必要になってから読む。

パッケージは、署名付きのマニフェストと、個別に圧縮してハッシュを取った塊（チャンク）からなる。
マニフェストには、パッケージ ID、役割、依存先、優先度、復号後の内容のハッシュ（content root）、鍵 ID、ライセンスの方針、形式の版が入る。

## 三つのプロファイル

| プロファイル | 改ざんの検出 | 暗号化 | 用途 |
|---|---|---|---|
| `dev` | BLAKE3 | なし | 手元の開発とテスト |
| `signed` | BLAKE3 と Ed25519 署名 | なし | 公開する配布物 |
| `protected` | BLAKE3 と Ed25519 署名 | チャンクごとに XChaCha20-Poly1305 | 内容を保護したい配布物 |

`dev` のハッシュは壊れたファイルを見つけるためのもので、誰が作ったかは保証しない。
`signed` と `protected` は、発行者の署名で、配布物が発行者のものであることも確かめる。
`protected` のパッケージは暗号化のためにバイト列が環境ごとに異なるので、Native と Web の同一性は、復号後の content root、バイトコードのチェックサム、VM の再生結果が一致することで確かめる。

```sh
aria build my-game --target linux-x64 --profile dev
aria build my-game --target windows-x64 --profile signed \
  --signing-key publisher:<秘密鍵を 16 進 64 文字で>
aria build my-game --target web --profile protected \
  --signing-key publisher:<秘密鍵> \
  --encryption-key content:<暗号鍵>
```

鍵は環境変数 `ARIA_PAK_SIGNING_KEY` と `ARIA_PAK_ENCRYPTION_KEY` でも渡せる。
値は 32 バイトの鍵を 16 進 64 文字で書いたもので、先頭に `鍵ID:` を付けられる。
署名の秘密鍵は、以後のすべての更新に要る発行者の身元なので、復旧できる秘密管理に保管する。
検証用の公開鍵は、作品の表示パッケージに埋め込んでよい。

## 読み込みとライセンス

パッケージを読む処理は `aria-protection` にあり、Native と Web で同じものを使う。
Core は鍵を持たず、発行者の署名も検証しない。

`protected` のパッケージは、ライセンスを提供する部品（`LicenseProvider`）から、署名付きで期限のある利用許可（lease）を受け取ってから開く。
手元に有効な許可があれば、そのまま開く。
なければ一度だけ更新を求め、署名、パッケージとゲームの ID、期間、猶予、失効を確かめてから素材を渡す。
許可が有効な間と、宣言した猶予の間は、オフラインでも起動できる。

Web 版では、`globalThis.ariaPakKeyProvider(bundle)` が、短い期限の検証鍵と暗号鍵を渡す入り口になる。
ブラウザと Wasm の中にある内容を、そのブラウザの利用者から完全に隠すことはできないので、Web 版の暗号化は手軽な複製を防ぐためのものと考える。
