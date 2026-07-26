aria;

scene chapter_select_en {
  screen chapter_select;
  background asset("#102b38") with wipe(260ms);
  say Mio: "Which tide should open the record?";
  await advance;
  choice {
    "01 — Autumn Rooftop" => en_chapter_01;
    "02 — Station Platform" => en_chapter_02;
    "03 — Old Photograph" => en_chapter_03;
    "04 — Promise by the Sea" => en_chapter_04;
    "05 — Rain Sketch" => en_chapter_05;
    "06 — Lighthouse at Night" => en_chapter_06;
    "07 — Where the Wind Goes" => en_chapter_07;
    "08 — Brightest Autumn" => en_chapter_08;
  }
}

scene en_chapter_01 {
  locale "en-US";
  persistent flag "chapter_01_seen_en" = true;
  unlock chapter "chapter_01" progress 100;
  background asset("#284b59") with fade(260ms);
  say Me: "A memory of daisies loosens in the autumn wind.";
  await advance;
  jump chapter_select_en;
}

scene en_chapter_02 {
  locale "en-US";
  persistent flag "chapter_02_seen_en" = true;
  unlock chapter "chapter_02" progress 100;
  background asset("#1f3b4d") with fade(260ms);
  say Mio: "There is still time before the next station.";
  await advance;
  jump chapter_select_en;
}

scene en_chapter_03 {
  locale "en-US";
  persistent flag "chapter_03_seen_en" = true;
  unlock chapter "chapter_03" progress 100;
  background asset("#3d4655") with wipe(300ms);
  say Mio: "The people in the photograph do not know the future yet.";
  await advance;
  jump chapter_select_en;
}

scene en_chapter_04 {
  locale "en-US";
  persistent flag "chapter_04_seen_en" = true;
  unlock chapter "chapter_04" progress 100;
  background asset("#244e5a") with fade(300ms);
  say Me: "I chose to join her final journey.";
  await advance;
  jump chapter_select_en;
}

scene en_chapter_05 {
  locale "en-US";
  persistent flag "chapter_05_seen_en" = true;
  unlock chapter "chapter_05" progress 100;
  background asset("#394857") with wipe(300ms);
  say Mio: "Lines grow a little longer on rainy days.";
  await advance;
  jump chapter_select_en;
}

scene en_chapter_06 {
  locale "en-US";
  persistent flag "chapter_06_seen_en" = true;
  unlock chapter "chapter_06" progress 100;
  background asset("#17253b") with fade(300ms);
  say Me: "The lighthouse brushed the night sea once.";
  await advance;
  jump chapter_select_en;
}

scene en_chapter_07 {
  locale "en-US";
  persistent flag "chapter_07_seen_en" = true;
  unlock chapter "chapter_07" progress 100;
  background asset("#315565") with wipe(320ms);
  say Mio: "The wind chooses no destination, yet it keeps moving.";
  await advance;
  jump chapter_select_en;
}

scene en_chapter_08 {
  locale "en-US";
  persistent flag "chapter_08_seen_en" = true;
  unlock chapter "chapter_08" progress 100;
  unlock cg "cg_final_en";
  background asset("#6d6b57") with fade(420ms);
  say Me: "This was our unskilled affirmation of life.";
  await advance;
  jump chapter_select_en;
}
