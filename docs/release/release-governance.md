# 海風 Release Governance

## Versioning

- 公開版は`vMAJOR.MINOR.PATCH`を使う。
- `PATCH`は本文・保存互換を壊さない修正。
- `MINOR`は後方互換な機能、章、配布経路の追加。
- `MAJOR`はAriaバイトコード、UIモデル、保存、本文経路の互換を意図的に切る変更。
- `aria.toml`のゲーム版とTauri製品版は同じ公開版番号へ揃える。

## Editions

| Edition | Entry | Save namespace | Desktop identity | 内容境界 |
| --- | --- | --- | --- | --- |
| 完成版 | `scripts/main.aria` | `umikaze-v4` | `jp.example.umikaze` | DAY 0–10 |
| 体験版 | `scripts/main-demo.aria` | `umikaze-demo-v1` | `jp.example.umikaze.demo` / `umikaze-demo` | DAY 0–4、`demo_end` |

体験版は完成版の停止フラグではない。ビルド時に別入口を選び、DAY 5以降を
import closureにもPAKにも含めない。体験版は完成版の保存を移行・消去・読込してはならない。

## Artifact policy

公開候補には次を添える。

- targetごとのインストーラーまたは静的Webアーカイブ
- `release-manifest.json`と`checksums.sha256`
- 署名済みPAKの検証鍵ID、ゲームID、版番号、edition
- 変更内容・既知の問題・保存互換/リセットの説明
- Windows Authenticode、macOS署名/notarizationが必要な場合はその証跡

PAK署名の秘密鍵、コード署名鍵、Steam資格情報はリポジトリへ置かず、CIの保護された
シークレットからのみ渡す。署名不能なローカル成果物はプレイテスト用であり、
公開候補と混同しない。

## Compatibility and rollback

- UI View Modelのschema変更、保存envelope変更、`save_namespace`変更はリリースノートに明記する。
- 旧名前空間の削除は`legacy_save_namespaces`で明示したものだけに限る。
- 重大障害時はまず直前の署名済みartifactへ戻し、保存の扱いを確認してから修正版を出す。
- Canonical MarkdownとAriaの本文照合が失敗した版は公開しない。

## Store and promotional policy

- 実在し、承認済みのURLだけを明示的なユーザー操作から開く。
- URLが未設定の段階でSteam/X/SNS/公式サイトの仮リンクをゲーム内へ置かない。
- レビュー、評価、SNS投稿の対価や誘導をゲーム進行へ結びつけない。
- Steam公開前にはApp ID、Depot構成、Steam Cloudの保存位置、年齢/コンテンツ表示を別途確定する。

WebとTauri desktop shellが現行の公式ターゲットである。Steamは追加の流通層であり、
上記の情報と各プラットフォーム審査を満たすまで「対応済み」と表記しない。
