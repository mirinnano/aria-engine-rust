import type { ChapterPreviewRecord } from "./chapter-preview.types";

// These lines are the authored day-card inscriptions for the playable demo,
// not a second synopsis.  Keeping this module closed at DAY 4 means the web
// bundle cannot disclose a later chapter through its chapter-selection UI.
export const chapterPreviewByLabel: Record<string, ChapterPreviewRecord> = {
  PROLOGUE: {
    date: "春から九月",
    synopsis: "季節だけが先に進む窓辺で、まだ名もない願いが揺れている。",
    scene: "ward",
  },
  "DAY 1": {
    date: "9月21日・横浜駅",
    synopsis: "西へ向かう最初の列車が、朝のホームを離れる。",
    scene: "station",
  },
  "DAY 2": {
    date: "9月22日・三ノ宮",
    synopsis: "雨の気配が近づく街で、二人は次の行き先を探している。",
    scene: "rain",
  },
  "DAY 3": {
    date: "9月23日・岡山",
    synopsis: "遠ざかる景色の先で、言葉にできないものと向き合う。",
    scene: "rail-sunset",
  },
  "DAY 4": {
    date: "9月24日・松江",
    synopsis: "残そうとする音が、静かな海辺へ続く道を指している。",
    scene: "shore",
  },
};
