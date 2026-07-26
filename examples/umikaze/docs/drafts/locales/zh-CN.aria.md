aria;

scene chapter_select_zh_cn {
  screen chapter_select;
  background asset("#102b38") with wipe(260ms);
  say 美绪: "要从哪一段潮汐开始阅读？";
  await advance;
  choice {
    "01 — 秋日屋顶" => zh_cn_chapter_01;
    "02 — 车站月台" => zh_cn_chapter_02;
    "03 — 老照片" => zh_cn_chapter_03;
    "04 — 海边的约定" => zh_cn_chapter_04;
    "05 — 雨中的素描" => zh_cn_chapter_05;
    "06 — 夜晚的灯塔" => zh_cn_chapter_06;
    "07 — 风的去向" => zh_cn_chapter_07;
    "08 — 最耀眼的秋天" => zh_cn_chapter_08;
  }
}

scene zh_cn_chapter_01 {
  locale "zh-CN";
  persistent flag "chapter_01_seen_cn" = true;
  unlock chapter "chapter_01" progress 100;
  background asset("#284b59") with fade(260ms);
  say 我: "不太整齐的雏菊记忆，在秋风中慢慢散开。";
  await advance;
  jump chapter_select_zh_cn;
}

scene zh_cn_chapter_02 {
  locale "zh-CN";
  persistent flag "chapter_02_seen_cn" = true;
  unlock chapter "chapter_02" progress 100;
  background asset("#1f3b4d") with fade(260ms);
  say 美绪: "到下一站之前，还有一点时间。";
  await advance;
  jump chapter_select_zh_cn;
}

scene zh_cn_chapter_03 {
  locale "zh-CN";
  persistent flag "chapter_03_seen_cn" = true;
  unlock chapter "chapter_03" progress 100;
  background asset("#3d4655") with wipe(300ms);
  say 美绪: "照片里的人，还不知道未来会发生什么。";
  await advance;
  jump chapter_select_zh_cn;
}

scene zh_cn_chapter_04 {
  locale "zh-CN";
  persistent flag "chapter_04_seen_cn" = true;
  unlock chapter "chapter_04" progress 100;
  background asset("#244e5a") with fade(300ms);
  say 我: "我选择陪她走完最后的旅程。";
  await advance;
  jump chapter_select_zh_cn;
}

scene zh_cn_chapter_05 {
  locale "zh-CN";
  persistent flag "chapter_05_seen_cn" = true;
  unlock chapter "chapter_05" progress 100;
  background asset("#394857") with wipe(300ms);
  say 美绪: "下雨的时候，线条总会变得长一点。";
  await advance;
  jump chapter_select_zh_cn;
}

scene zh_cn_chapter_06 {
  locale "zh-CN";
  persistent flag "chapter_06_seen_cn" = true;
  unlock chapter "chapter_06" progress 100;
  background asset("#17253b") with fade(300ms);
  say 我: "灯塔的光，轻轻掠过夜里的海。";
  await advance;
  jump chapter_select_zh_cn;
}

scene zh_cn_chapter_07 {
  locale "zh-CN";
  persistent flag "chapter_07_seen_cn" = true;
  unlock chapter "chapter_07" progress 100;
  background asset("#315565") with wipe(320ms);
  say 美绪: "风不会决定目的地，却仍然向前。";
  await advance;
  jump chapter_select_zh_cn;
}

scene zh_cn_chapter_08 {
  locale "zh-CN";
  persistent flag "chapter_08_seen_cn" = true;
  unlock chapter "chapter_08" progress 100;
  unlock cg "cg_final_cn";
  background asset("#6d6b57") with fade(420ms);
  say 我: "这就是我们找到的、不太熟练的生命肯定。";
  await advance;
  jump chapter_select_zh_cn;
}
