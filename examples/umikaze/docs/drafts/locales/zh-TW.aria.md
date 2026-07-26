aria;

scene chapter_select_zh_tw {
  screen chapter_select;
  background asset("#102b38") with wipe(260ms);
  say 美緒: "要從哪一段潮汐開始閱讀？";
  await advance;
  choice {
    "01 — 秋日屋頂" => zh_tw_chapter_01;
    "02 — 車站月台" => zh_tw_chapter_02;
    "03 — 老照片" => zh_tw_chapter_03;
    "04 — 海邊的約定" => zh_tw_chapter_04;
    "05 — 雨中的素描" => zh_tw_chapter_05;
    "06 — 夜晚的燈塔" => zh_tw_chapter_06;
    "07 — 風的去向" => zh_tw_chapter_07;
    "08 — 最耀眼的秋天" => zh_tw_chapter_08;
  }
}

scene zh_tw_chapter_01 {
  locale "zh-TW";
  persistent flag "chapter_01_seen_tw" = true;
  unlock chapter "chapter_01" progress 100;
  background asset("#284b59") with fade(260ms);
  say 我: "不太整齊的雛菊記憶，在秋風中慢慢散開。";
  await advance;
  jump chapter_select_zh_tw;
}

scene zh_tw_chapter_02 {
  locale "zh-TW";
  persistent flag "chapter_02_seen_tw" = true;
  unlock chapter "chapter_02" progress 100;
  background asset("#1f3b4d") with fade(260ms);
  say 美緒: "到下一站之前，還有一點時間。";
  await advance;
  jump chapter_select_zh_tw;
}

scene zh_tw_chapter_03 {
  locale "zh-TW";
  persistent flag "chapter_03_seen_tw" = true;
  unlock chapter "chapter_03" progress 100;
  background asset("#3d4655") with wipe(300ms);
  say 美緒: "照片裡的人，還不知道未來會發生什麼。";
  await advance;
  jump chapter_select_zh_tw;
}

scene zh_tw_chapter_04 {
  locale "zh-TW";
  persistent flag "chapter_04_seen_tw" = true;
  unlock chapter "chapter_04" progress 100;
  background asset("#244e5a") with fade(300ms);
  say 我: "我選擇陪她走完最後的旅程。";
  await advance;
  jump chapter_select_zh_tw;
}

scene zh_tw_chapter_05 {
  locale "zh-TW";
  persistent flag "chapter_05_seen_tw" = true;
  unlock chapter "chapter_05" progress 100;
  background asset("#394857") with wipe(300ms);
  say 美緒: "下雨的時候，線條總會變得長一點。";
  await advance;
  jump chapter_select_zh_tw;
}

scene zh_tw_chapter_06 {
  locale "zh-TW";
  persistent flag "chapter_06_seen_tw" = true;
  unlock chapter "chapter_06" progress 100;
  background asset("#17253b") with fade(300ms);
  say 我: "燈塔的光，輕輕掠過夜裡的海。";
  await advance;
  jump chapter_select_zh_tw;
}

scene zh_tw_chapter_07 {
  locale "zh-TW";
  persistent flag "chapter_07_seen_tw" = true;
  unlock chapter "chapter_07" progress 100;
  background asset("#315565") with wipe(320ms);
  say 美緒: "風不會決定目的地，卻仍然向前。";
  await advance;
  jump chapter_select_zh_tw;
}

scene zh_tw_chapter_08 {
  locale "zh-TW";
  persistent flag "chapter_08_seen_tw" = true;
  unlock chapter "chapter_08" progress 100;
  unlock cg "cg_final_tw";
  background asset("#6d6b57") with fade(420ms);
  say 我: "這就是我們找到的、不太熟練的生命肯定。";
  await advance;
  jump chapter_select_zh_tw;
}
