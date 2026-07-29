# 海風 BGM assets

このディレクトリのOGGは、2026-07-30時点の初回試写用音源である。元MP3は変更・削除せず、リポジトリへは配布に使う論理Cue名だけを置く。

## 変換条件

- Ogg Vorbis
- 44.1 kHz / stereo
- FFmpeg native Vorbis encoder quality 5
- 元音源のダイナミクスを変えず、固定ゲインだけで約 -18 LUFSへ調整
- 元のタイトル等のメタデータを除去し、Cue IDだけを `title` として記録

生成MP3からOGGへの変換は試写段階だけの措置である。正式公開時は、採用テイクのWAVまたはFLACマスターから同じCue IDへ一度だけ書き出す。

## Cue対応

| Cue ID | 初回選定ソース | 実尺 | 使用形 | 章 |
| --- | --- | ---: | --- | --- |
| `umk.ward.first-light` | `Spring_in_Room_.mp3` | 168.57秒 | loop | DAY 0 |
| `umk.rail.departure` | `The_Platform_Slides_Away.mp3` | 159.43秒 | loop | DAY 1 |
| `umk.rain.room` | `Tea_On_The_Nightstand.mp3` | 178.52秒 | loop | DAY 5 |
| `umk.recording.trace` | `Unfinished_Answer.mp3` | 84.17秒 | non-loop | DAY 4 |
| `umk.clear-between` | `Platform_at_Noon.mp3` | 163.42秒 | loop | DAY 6 |
| `umk.island.distance` | `Crossing_the_Inland_Sea.mp3` | 179.88秒 | non-loop | DAY 7 |
| `umk.north.grey` | `Along_The_Grey_Rails.mp3` | 179.25秒 | non-loop | DAY 9 |
| `umk.everyday.table` | `Tea_Leaves_and_Keys.mp3` | 127.19秒 | loop | DAY 6 |
| `umk.waiting.window` | `A_Rest_In_Yokohama.mp3` | 81.11秒 | loop | DAY 7 |

`Waiting_by_the_inland_sea.mp3` と `Midday_Local.mp3` は `umk.island.distance` の未採用テイクとして配布物へ含めない。`A_Morning_Left_Unfinished.mp3` は `umk.spring.after` の候補として保管するが、後日談本文が未確定なので配布物へ含めない。終盤用の `umk.lighthouse.edge`、音源未生成の `umk.errand.steps` 以降も、このディレクトリとPAKへはまだ含めない。
