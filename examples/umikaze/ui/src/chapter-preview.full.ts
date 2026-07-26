import { chapterPreviewByLabel as openingChapterPreviews } from "./chapter-preview.demo";
import type { ChapterPreviewRecord } from "./chapter-preview.types";

export const chapterPreviewByLabel: Record<string, ChapterPreviewRecord> = {
  ...openingChapterPreviews,
  "DAY 5": {
    date: "9月25日・益田",
    synopsis: "強い雨が、進む理由を足止めする。",
    scene: "rain-city",
  },
  "DAY 6": {
    date: "晴れた移動の途中",
    synopsis: "夜の駅を越え、海の気配へ向かう。",
    scene: "platform",
  },
  "DAY 7": {
    date: "始発前の待合室",
    synopsis: "海を渡るあいだ、記録の外側が近づいてくる。",
    scene: "mist",
  },
  "DAY 8": {
    date: "山あいの居間",
    synopsis: "遠い場所の映像が、静かな朝を占めていく。",
    scene: "understructure",
  },
  "DAY 9": {
    date: "北へ向かう列車",
    synopsis: "足元の地図を離れ、線路だけが先へ続いている。",
    scene: "night",
  },
  "DAY 10": {
    date: "終点を知らない列車",
    synopsis: "灰色の海のそばを、降りる理由のないまま進む。",
    scene: "blue",
  },
};
