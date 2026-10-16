# Aria 言語仕様

Aria の台本は、先頭の `aria;` から始まる。
コンパイラは、構文、型、表示資源の所有権、制御フローを検査してから、バイトコード（ARIAC7）に変換する。
この文書は、作者が書く構文とその意味を説明する。
演算子と全命令の網羅的な一覧ではないので、細部は[構文解析](../../crates/aria-core/src/modern.rs)と[コンパイラ](../../crates/aria-core/src/modern_compiler.rs)、および `crates/aria-core/tests/` の例で確かめる。

## 場面と流れ

```aria
aria;
module example.story;
entry opening;

scene opening {
    background asset("#07131f") with fade(240ms);
    narrate "朝のホームに、風が通った。";
    await advance;
    choice {
        "海へ行く" => shore;
        "ここで待つ" => stay;
    }
}

scene shore { narrate "波の音が近づいた。"; await advance; end; }
scene stay { narrate "次の列車を待つことにした。"; await advance; end; }
```

台本は場面（`scene`）の集まりで、`entry` が最初の場面を指す。
場面は隣の場面へ自然に流れないので、`end`、`jump`、`choice` のいずれかで行き先を書く。
`call` は別の場面を呼び、その場面の `return` で呼んだ位置へ戻る。

`narrate "…";` は話者のない本文、`say 話者 "…";` は話者付きの本文を表示し、`await advance;` は読者の入力を待つ。
`choice` は選択肢の文字列と移動先の場面を並べる。
`cancel => 場面;` を書くと、読者が戻る操作（Escape や閉じる）をしたときの移動先になり、`cancel` 自体は選択肢として表示されない。
`cancel` のない選択肢を出している画面では、戻る操作をしても画面は閉じない。

`use "scenario/chapter.aria";` は、コンパイル時に別の台本を取り込む。
実行時にファイルを読む命令ではない。

## 本文

### prose

```aria
prose breath 220ms {
    "病室の窓から見える景色は、毎日少しずつ変わっていく。";
    "小学校の時に初めて倒れて入院して以来、" +;
    "私の世界は、この一部屋に収まるほどのものになってしまった。";
    pause 480ms;
    mio "行こう。";
    play bgm asset("assets/audio/bgm/first-light.ogg") loop fade 700ms;
    "窓のむこうで、桜が舞い散る。";
}
```

`prose` の中では、文字列だけの行が本文の一行になる。
一行ごとに、表示して読者の入力を待ち、本文を消して `breath` の時間だけ間を置く。
`識別子 "…";` は話者付きの行、行末の `+` は本文を消さずに次の行へ続ける印である。
直後の `pause <n>ms;` はその行の後の間だけを変え、`pause 0ms;` は本文を消して待たずに進む。
それ以外の文（`play`、`background`、`stage` など）は書いた位置で実行する。
`prose` は構文解析の段階で通常の文の並びに展開されるので、`narrate` と `await advance` を並べて書いた場合と同じバイトコードになる。

### 文中の記法

| 書き方 | 意味 |
|---|---|
| `\|宍道湖《しんじこ》` | ルビ。`｜` も使える |
| `病室《びょうしつ》` | ルビ。直前に続く漢字が親文字になる |
| `《《ここ》》` | 傍点 |
| `{wait:300}` | 文中で 300ms 止める |
| `{days}` | 変数 `days` の値を差し込む |

記法はウェブ小説の投稿サイトで使われているものと同じである。
形が完成していない `《`、`|`、`{` は文字としてそのまま表示されるので、普通の文章を書き換える必要はない。
親文字のないルビ、知らないコマンド、存在しない変数名はコンパイルエラーになる。
表示モデルには、記号を取り除いた本文と、ルビ・傍点の範囲が渡る。

## 宣言

台本の最上位には、場面のほかに次の宣言を書ける。
どれもプログラムの資料として読み込み時に読まれ、保存データには入らない。
そのため、宣言の文言を直すと、既存の保存にもそのまま反映される。

### 登場人物

```aria
character mio { name "ミオ"; }
```

`say mio "…";` は宣言した名前「ミオ」で表示され、表示モデルには話者 ID `mio` も渡る。
登場人物を一人でも宣言した作品では、宣言のない話者を警告する。

### 章

```aria
chapter day01 {
    label "DAY 1";
    numeral "I";
    title "西へ";
    date "9月21日・横浜駅";
    synopsis "西へ向かう最初の列車が、朝のホームを離れる。";
    image asset("assets/bg/scenes/platform-sea-dawn-v1.webp");
    start novel_chapter_01;
}

scene chapter_select {
    screen chapter_select;
    choice {
        chapter day01;
        cancel => title_screen;
    }
}
```

`chapter` は、章の一覧に出す情報を一か所にまとめる。
選択肢の `chapter day01;` は、ラベルに `title` を、移動先に `start` を使う。
日付カードのように別の場面へ進むときは `chapter day01 => day01_story;` と書く。
`unlock chapter "day01" progress 1;` の ID が宣言の ID と一致すると、解放状態と進み具合が章に結び付く。

### 資料のまとまり

```aria
collection title_quotes {
    item tractatus_world {
        text "世界は、起こっていることの総体である。";
        source "Ludwig Wittgenstein, Tractatus Logico-Philosophicus, 1";
    }
}
```

`collection` は、表示パッケージが画面に出す資料（引用、注記、クレジットなど）を台本の側で宣言する。
各 `item` は ID と、文字列の項目を持つ。
表示モデルの `collections` に、宣言した順のまま渡る。
同じ名前の `collection` を複数の台本に分けて書いてもよいが、`item` の ID は重複できない。

### CG

```aria
cg shore_dawn { title "夜明けの浜"; image asset("assets/bg/shore.webp"); }
```

`unlock cg "shore_dawn";` で解放した CG は、ギャラリーの表示モデルに `title` と `image` を持って現れる。
`image` は台本が参照する素材として数えられるので、範囲を絞った体験版のビルドにも含まれる。

## 状態と式

状態の型は `Int`、`Bool`、`String` で、可変の状態は `state mut` で宣言する。
局所変数は `let`、可変の局所変数は `let mut` である。

```aria
state mut trust: Int = 0;

scene talk {
    trust = trust + 4;
    flag "met_mio" = on;
    if trust >= 5 && flag("met_mio") { narrate "{trust}"; }
    end;
}
```

代入の右辺には `+` と `-` の式を書け、`+=` と `-=` もある。
`flag "名前" = on;` はその周回のフラグを立て、`persistent flag "名前" = on;` は周回をまたいで残るフラグを立てる。
`flag("名前")` はどちらのフラグも Bool として読む。
どの台本も立てていないフラグを読むと、綴りの誤りとして警告する。

```aria
choice {
    "海へ行く" => shore;
    "あの話をする" if trust >= 5 => confession;
}
```

`if` の付いた選択肢は、条件が成り立つときだけ表示される。
選択肢の番号（`choice:0` など）は、表示されている選択肢の並びに対して振られる。

## 表示資源の所有権

```aria
scene opening {
    let mut portrait = show image(asset("assets/portrait.webp")) at (760px, 86px) z 20;
    borrow mut portrait as image {
        move &mut image to (720px, 86px);
    }
    drop portrait;
    end;
}
```

`show` で作った表示資源は、所有者を一つだけ持つ。
`drop`、所有権の移動、スコープの終わりのどれかで一度だけ解放され、変形には可変の借用（`&mut`、`borrow mut`）が要る。
コンパイラは、移動済みや借用中の資源への操作を拒否する。

## 時間と画面

```aria
wait 500ms;
wait breath 300ms;
clear dialogue;
screen dialogue;
```

`wait` は指定した時間を待つ。
`wait breath` は、短い最低時間が過ぎたあと、読者の通常の入力で切り上げられる待ちである。
`clear dialogue` は本文を消す。
`screen` は意味としての画面（`setup`、`title`、`dialogue`、`chapter_select`、`gallery` など）へ移る命令で、座標や配色は指定しない。
作品固有の画面は[プラグイン](plugins.md)が登録し、登録されていない画面名はコンパイルエラーになる。

### 背景の意味（stage）

```aria
background asset("#6d706f") with fade(260ms);
stage stillness;
```

`stage` は、直前の背景が物語のうえで何を表すかを名前で示す。
名前は英小文字で始まり、英小文字・数字・`-`・`_` だけからなる 48 文字以内の語で、色や座標は受け付けない。
見た目は、表示パッケージが名前から決める。
写真の背景は素材のパスが場所を表すので、`stage` が要るのは単色の背景や日付カードのように素材だけでは意味が決まらない場面である。
`stage` の値は次の `background` で消えるので、前の場面の意味が新しい背景に残らない。
`aria.toml` の `[presentation] stages = [...]` に表示パッケージが扱う名前を並べると、`aria check` はそれ以外の名前をエラーにする。

### カメラ

```aria
camera zoom 108 pan (0, -3) over 9000ms ease ease_out;
camera reset over 1200ms;
```

`zoom` は画面に対する拡大率（100 が等倍）、`pan` は画面に対する移動量で、どちらも整数の百分率である。
VM が持つのは目標の構図と時間だけで、表示パッケージがそこへ向かって動かす。
読者が「動きを抑える」を選んでいると、表示パッケージは目標の構図へすぐに切り替える。
カメラも `stage` と同じく、次の `background` で元の構図に戻る。

### 環境音

```aria
play ambient asset("assets/audio/ambient/rain.ogg") loop fade 800ms;
stop ambient;
```

`ambient` は BGM とは別の音量で鳴る環境音で、素材ごとに鳴るので雨と波のように重ねられる。

## プラグインの命令

```aria
state mut total: Int = 0;

scene shore {
    total = tally::add(3);
    if tally::reached(10) { narrate "届いた。"; }
    tally::mark("shore");
    end;
}
```

`名前空間::命令(引数)` は、プラグインが提供する命令を呼ぶ。
式としても文としても書け、Bool を返す命令は `if` と `while` の条件にも使える。
コンパイラは、プラグインが宣言した引数の数と型、戻り値の型と照らし合わせる。
名前空間を提供するプラグインがない、命令がない、型が合わない場合はコンパイルエラーになる。
プラグインの作り方は[プラグイン API](plugins.md)にある。

## 改稿と保存

保存には、命令の番地に加えて、コンパイラが付けた位置の目印が入る。
目印は、場面の先頭、選択肢、`call` の戻り先、本文の各行（場面名・話者・本文から作るキー）に付く。
同じバイトコードで読み込むと、保存した番地にそのまま戻る。
改稿したバイトコードで読み込むと、同じ行、同じ場面の同じ番目の行、場面の先頭の順に位置を探して再開する。
場面ごと削除した場合は戻す先が決まらないので、読み込みを拒否する。

既読の判定も同じ行のキーで行う。
表示した順序ではなく行で判定するので、別の章から読み始めても既読がずれず、誤字を直した行以外は改稿後も既読のままになる。

## 検査

```sh
cargo run --locked -p aria-cli --no-default-features -- check <作品のディレクトリ> --release
cargo run --locked -p aria-cli --no-default-features -- run <作品のディレクトリ> --headless
```
