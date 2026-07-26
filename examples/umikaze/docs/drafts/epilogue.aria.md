aria;
module umikaze.scenario.ja.chapter_12;

state mut interlude_chapter_12_seen: Bool = false;
state mut interlude_chapter_12_end_seen: Bool = false;

// Source: ex.md
scene novel_chapter_12 {
  locale "ja-JP";
  persistent flag "canonical_chapter_12_seen" = true;
  unlock chapter "canonical_chapter_12" progress 1;
  background asset("#6d6b57") with fade(260ms);
  screen interlude;
  clear dialogue;
  narrate "春は、忘れたものの上にも同じ明るさで来る。";
  if interlude_chapter_12_seen {
    wait 220ms;
  } else {
    interlude_chapter_12_seen = true;
    wait 1200ms;
  }
  screen day_card;
  choice {
    "EPILOGUE\n春\n季節が巡ったあとにも、忘れられない風の匂いがある。" => novel_chapter_12_story;
  }
}

scene novel_chapter_12_story {
  screen dialogue;
  background asset("#3d4655") with fade(360ms);
  wait 240ms;
  narrate "春が来るたび、俺はあの不格好な雛菊を思い出す。";
  await advance;
  narrate "彼女が最期に遺した、ささやかな祈りのような花。";
  await advance;
  narrate "俺は時折、冷たい石碑の前に立ち尽くし、ただ風の音を聞く。";
  await advance;
  narrate "彼女がいなくなっても、世界はいつも通りに動いている。";
  await advance;
  narrate "大した事件なんてないし、誰も彼女の死なんて気にしていない。";
  await advance;
  narrate "……彼女だって、この世界で毎日死んでいく何千人の中の一人に過ぎないから。";
  await advance;
  effect tint "#e3d9cc" amount 18 over 320ms;
  wait 320ms;
  narrate "俺はこの平凡な日常の中で、もう新しく何かを激しく感じ取ることはないのかもしれない。";
  await advance;
  narrate "けれど、ふとした瞬間に思い出す。";
  await advance;
  narrate "いつまでも続く、単調なレールの音。";
  await advance;
  narrate "ひどく重たかった、海風の匂い。";
  await advance;
  clear dialogue;
  wait 360ms;
  narrate "あの秋の日、俺たちが確かに底辺を這いずり回っていたということ。";
  await advance;
  narrate "彼女が消えてしまっても、世界は何も変わらない。";
  await advance;
  narrate "俺の心も、案外平然と日常に適応している。";
  await advance;
  narrate "けれど、時折ひどく喉が渇くのだ。";
  await advance;
  narrate "あの日、彼女の隣で口にした、ひどくぬるいミネラルウォーターの味が忘れられなくて。";
  await advance;
  narrate "―――――だからこれは、決して戻ることのできない、あの頃の...思い出話。";
  await advance;
  clear dialogue;
  effect tint "#07090d" amount 58 over 560ms;
  wait 700ms;
  screen interlude;
  clear dialogue;
  narrate "幸福は、世界の外側にだけ、静かに残っていた。";
  if interlude_chapter_12_end_seen {
    wait 220ms;
  } else {
    interlude_chapter_12_end_seen = true;
    wait 1200ms;
  }
  screen dialogue;
  chapter "canonical_chapter_12" progress 100;
  clear dialogue;
  jump chapter_select_ja;
}
