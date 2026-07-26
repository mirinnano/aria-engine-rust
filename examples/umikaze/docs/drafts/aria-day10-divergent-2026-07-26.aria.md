// Non-canonical Aria adaptation, archived before canonical text sync.
// Canonical source: /home/mirin/Desktop/Novel/src/10_day10.md
// This file is deliberately .aria.md so the project builder cannot compile it.
aria;
module umikaze.scenario.ja.chapter_10;

state mut interlude_chapter_10_seen: Bool = false;
state mut interlude_chapter_10_turn_seen: Bool = false;

// Source: 10_day10.md
scene novel_chapter_10 {
  locale "ja-JP";
  persistent flag "canonical_chapter_10_seen" = true;
  unlock chapter "canonical_chapter_10" progress 1;
  background asset("#17253b") with fade(260ms);
  screen interlude;
  clear dialogue;
  narrate "終点の先にも、線路の音だけは残る。";
  if interlude_chapter_10_seen {
    wait 220ms;
  } else {
    interlude_chapter_10_seen = true;
    wait 1200ms;
  }
  screen day_card;
  choice {
    "DAY 10\n9月30日・終点の先\n何も決めないまま、線路だけが先へ伸びていく。" => novel_chapter_10_story;
  }
}

scene novel_chapter_10_story {
  screen dialogue;
  background asset("#17253b") with fade(360ms);
  wait 180ms;
  narrate "硬いベンチで目を覚ますと、待合室に朝の光が差し込んでいた。";
  await advance;
  narrate "体中が痛い。隣のミオは、俺の上着を抱えたまま、まだ眠っている。";
  await advance;
  narrate "シャッターは半分だけ開いていて、ホームから箒の音が聞こえた。";
  await advance;
  narrate "肩を軽く呼ぶと、ミオは一度だけ眉を寄せてから目を開けた。";
  await advance;
  narrate "「行けるか」";
  await advance;
  narrate "「行ける。たぶん」";
  await advance;
  narrate "始発列車に乗った。行き先表示は見たけれど、覚えなかった。";
  await advance;
  narrate "乗り換えの途中、ホームの端にある立ち食いの店へ入った。";
  await advance;
  narrate "湯気を上げるかけうどんを一つ頼み、割り箸を割る。";
  await advance;
  narrate "先にミオへ渡すと、湯気が眼鏡のない顔を一瞬だけ隠した。";
  await advance;
  narrate "ミオは三口ほど啜って、箸を置いた。";
  await advance;
  narrate "「もういい。残り、食べていいよ」";
  await advance;
  narrate "俺はミオの丼を引き寄せ、黙って食った。";
  await advance;
  narrate "少しふやけた麺は、塩気だけがやけに残った。";
  await advance;
  narrate "食べ終えるまで、ミオは窓の外の線路を見ていた。";
  await advance;
  narrate "また列車に乗った。";
  await advance;
  narrate "窓の外は、ずっと同じ色をしていた。";
  await advance;
  narrate "鉛色の空と、灰色の海。";
  await advance;
  clear dialogue;
  background asset("#17253b") with fade(380ms);
  wait 380ms;
  narrate "しばらくすると、ミオは眠った。";
  await advance;
  narrate "俺も目を閉じた。";
  await advance;
  narrate "起きたら、強い喉の渇きを覚えた。";
  await advance;
  narrate "列車はトンネルの中を走っていた。窓に映った自分の顔が、知らないやつみたいに見えた。";
  await advance;
  narrate "どこで降りるのかも、どこが終点なのかも、俺はまだ知らない。";
  await advance;
  narrate "喉の渇きだけが、身体がまだここにいることを教えてくる。";
  await advance;
  narrate "「次、どこで降りる？」";
  await advance;
  narrate "眠っていたはずのミオが、目を閉じたまま言った。";
  await advance;
  narrate "「決めてない」";
  await advance;
  narrate "「じゃあ、決めようよ」";
  await advance;
  narrate "ミオは窓の外を見たまま、薄く笑った。";
  await advance;
  clear dialogue;
  screen interlude;
  clear dialogue;
  narrate "終点は、行き先の表示にあるものじゃなかった。";
  if interlude_chapter_10_turn_seen {
    wait 220ms;
  } else {
    interlude_chapter_10_turn_seen = true;
    wait 1200ms;
  }
  screen dialogue;
  wait 420ms;
  narrate "「終わった顔で、ずっと乗ってるのは嫌だ」";
  await advance;
  narrate "その言葉は、俺を責めるためではなく、ミオ自身の席を確かめるために聞こえた。";
  await advance;
  narrate "俺は路線図を探した。指で追った先に、次の小さな駅名があった。";
  await advance;
  narrate "「次で降りる」";
  await advance;
  narrate "「うん。そこで水、買おう」";
  await advance;
  narrate "列車が減速した。海の色が、窓の外で少しだけ近づいた。";
  await advance;
  clear dialogue;
  wait 420ms;
  chapter "canonical_chapter_10" progress 100;
  clear dialogue;
  jump chapter_select_ja;
}
