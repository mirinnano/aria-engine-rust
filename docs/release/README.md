# 海風 Release Documentation

現行の公開・体験版・CI判断には、次だけを運用文書として使う。

- [production-checklist.md](production-checklist.md) — 公開前の実行可能なゲート
- [qa-matrix.md](qa-matrix.md) — 自動/手動QAの範囲
- [release-governance.md](release-governance.md) — 版、保存、artifact、ストア方針
- [umikaze-distribution.md](umikaze-distribution.md) — Web/Tauri/installerの作り方

このディレクトリに残る`v1.0.0`、`AriaEngine`、`data.pak`、PowerShell、
旧Steam構成を参照する文書は、旧C#配布線の履歴・監査記録である。現行の
Aria v3 React/Tauri成果物の手順として実行してはならない。履歴を削除せず
残しているのは、過去の配布物を追跡できるようにするためである。
