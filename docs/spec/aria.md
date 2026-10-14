# Aria 作者言語

現行の作者言語は、バージョン番号のない `aria;` から始まります。
コンパイラは、構文、型、所有権、制御フローを検査し、ARIAC7 のバイトコードへ変換します。
旧 NScripter 命令、`aria 3.1;`、互換モード、スクリプト内の visual UI DSL は受理しません。

この文書は現行の作者向け入口と、今回の拡張で変わる画面契約を記述します。
演算子・全命令の完全な文法一覧ではありません。
個別構文の実装と回帰例は [parser](../../crates/aria-core/src/modern.rs)、[compiler](../../crates/aria-core/src/modern_compiler.rs)、[機能テスト](../../crates/aria-core/tests/v3_feature_parity.rs)で確認できます。
[Aria 3.1 の資料](aria-3.1.md)は歴史資料であり、現行構文の正本ではありません。

## 最小の物語

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

scene shore {
    narrate "波の音が近づいた。";
    await advance;
    end;
}

scene stay {
    narrate "次の列車を待つことにした。";
    await advance;
    end;
}
```

`narrate` は無話者の本文、`say 話者: "本文";` は話者付きの本文を表示します。
`await advance;` は読者の入力を待ちます。
`choice` は選択肢の文字列と、移動先の scene を宣言します。
`cancel => scene;` を書くと、その選択肢を出した画面で読者が戻る操作（Escape、閉じる）をしたときの移動先になります。`cancel` は選択肢として表示されません。
`cancel` のない選択肢を出している画面では、戻る操作をしても画面は閉じません。選択が宙に浮いたまま別の画面へ移らないようにするためです。
scene は隣の宣言へ暗黙に流れません。
終了、ジャンプ、選択など、制御の行き先を明示します。

`use "scenario/chapter.aria";` はコンパイル時に別のモジュールを取り込みます。
実行時に原稿ファイルを読む命令ではありません。

## 本文を書く

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

`prose` の中では、文字列だけの行が本文の一行です。
一行ごとに、表示して読者の入力を待ち、本文を消して `breath` の時間だけ息継ぎします。
`識別子 "…";` は話者付きの行です。
行末の `+` は、本文を消さずに次の行へ続けます。
直後の `pause <n>ms;` は、その行の後だけ息継ぎの長さを変えます。`pause 0ms;` は消すだけで待ちません。
それ以外の文（`play`、`background`、`stage` など）は、書いた位置でそのまま実行します。

`prose` はパーサーの段階で通常の文へ展開されます。
`narrate`、`await advance`、`clear dialogue`、`wait breath` を並べて書いた場合と、生成されるバイトコードは同じです。

### 文中の記法

| 書き方 | 意味 |
|---|---|
| `\|宍道湖《しんじこ》` | ルビ。`｜` も使えます |
| `病室《びょうしつ》` | ルビ。直前に続く漢字が親文字になります |
| `《《ここ》》` | 傍点 |
| `{wait:300}` | 文中で 300ms 止める |
| `{days}` | 変数 `days` の現在の値を差し込む |

ウェブ小説の投稿サイトで使われている記法と同じです。
形が完成していない `《`、`|`、`{` は文字としてそのまま表示されるので、通常の本文で書き換える必要はありません。
親文字のないルビ、知らないコマンド、存在しない変数名はコンパイルエラーになります。
表示モデルには記号を取り除いた本文と、ルビ・傍点の範囲（ページ内の文字位置）が渡ります。
文字送りの途中では、ルビは親文字がすべて表示されてから現れます。

## 登場人物と章

```aria
character mio { name "ミオ"; }

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

`character` を宣言すると、`say mio "…";` は宣言した名前「ミオ」で表示され、表示モデルには話者 ID `mio` も渡ります。
登場人物を一人でも宣言した作品では、宣言のない話者を警告します。

`chapter` は章の一覧に出す情報を一箇所で宣言します。
選択肢の `chapter day01;` は、ラベルに `title` を、移動先に `start` を使います。
日付カードのように別の場面へ進む場合は `chapter day01 => day01_story;` と書きます。
章の情報は保存には入らず、読み込んだバイトコードから毎回読み取ります。
そのため、改稿で章の題名を直すと、既存の保存にもそのまま反映されます。
`unlock chapter "day01" progress 1;` の ID と宣言の ID が一致すると、解放状態と進み具合が結び付きます。

## 状態と表示資源

状態の型には `Int`、`Bool`、`String` があり、可変状態は `state mut` として宣言します。
局所変数は `let`、可変局所変数は `let mut` です。

```aria
aria;
entry opening;
state mut visits: Int = 0;

scene opening {
    visits = visits + 1;
    let mut portrait = show image(asset("assets/portrait.webp")) at (760px, 86px) z 20;
    borrow mut portrait as image {
        move &mut image to (720px, 86px);
    }
    narrate "また、この場所へ来た。";
    await advance;
    drop portrait;
    end;
}
```

表示 Node は所有者を持ちます。
`drop`、所有権の移動、スコープ終了によって解放し、変更には可変借用が必要です。
コンパイラは、移動済み・借用中の Node の不正な操作を拒否します。

### 式

代入の右辺には `+` と `-` の式を書けます。`+=` と `-=` もあります。

```aria
trust = trust + 4;
doubt -= trust;
if trust >= 5 && flag("met_mio") { … }
```

`flag("名前")` はフラグを Bool として読みます。前の周回で立てた `persistent flag` も読めます。
どのスクリプトも立てていないフラグを読むと、綴りの誤りとして警告します。

### 条件付きの選択肢

```aria
choice {
    "海へ行く" => shore;
    "あの話をする" if trust >= 5 => confession;
}
```

`if` の付いた選択肢は、条件が成り立つときだけ表示されます。
選択肢の番号（`choice:0` など）は、表示されている選択肢の並びに対して振られます。

## 時間と画面

```aria
wait 500ms;
wait breath 300ms;
clear dialogue;
screen dialogue;
```

`wait` は指定時間を待ちます。
`wait breath` は最小の息継ぎ後に通常入力で解放できる待機です。
`clear dialogue` は本文を消します。
`screen` は意味的な画面へ遷移し、DOM の座標・配色・文字サイズは指定しません。

標準画面は `setup`、`title`、`demo_end`、`dialogue`、`pause`、`save`、`load`、`settings`、`backlog`、`chapter_select`、`gallery`、`confirm` です。
独自の画面は [Rust/Wasm プラグイン](plugins.md)でポリシーを登録します。
`day_card`、`interlude`、`statement` は海風のプラグインが提供する名前で、標準命令の例外ではありません。
登録されていない独自画面はコンパイル時に診断し、検査を迂回したバイトコードも VM が拒否します。
埋め込みホストの compiler は `compile_with_routes` に検証済みの画面一覧を渡し、VM には対応する同一のプラグインを登録します。

### 背景の意味（stage）

```aria
background asset("#6d706f") with fade(260ms);
stage stillness;
```

`stage` は、直前の背景が物語のうえで何を表すかを名前で示します。
名前は小文字の英字で始まり、英小文字・数字・`-`・`_` だけからなる48文字以内の語です。
`#264450` のような色や座標は受理しません。見た目は表示パッケージが名前から決めます。

`stage` の値は次の `background` で消えます。
前の場面の意味が、新しい背景へ引き継がれないようにするためです。
`stage none;` で明示的に消すこともできます。
値は保存に含まれ、表示モデルの `stage` として表示パッケージへ渡ります。
`stage` を持たない旧い保存から再開した場合、表示パッケージは背景から推定します。

写真の背景は、素材のパスがそのまま場所を表します。
`stage` が必要になるのは、単色の背景や日付カードのように、素材だけでは意味が決まらない場面です。
`aria.toml` の `[presentation] stages = [...]` に表示パッケージが扱える名前を列挙すると、`aria check` はそれ以外の名前をエラーにします。

### 背景のカメラ

```aria
camera zoom 108 pan (0, -3) over 9000ms ease ease_out;
camera reset over 1200ms;
```

`zoom` は画面に対する拡大率（100 が等倍）、`pan` は画面に対する移動量で、どちらも整数の百分率です。
VM が持つのは目標の構図と時間だけで、表示パッケージがそこへ向かって動かします。
読者が「動きを抑える」を選んでいると、表示パッケージは目標の構図へ即座に切り替えます。
カメラも `stage` と同じく、次の `background` で元の構図に戻ります。

### 環境音

```aria
play ambient asset("assets/audio/ambient/rain.ogg") loop fade 800ms;
play ambient asset("assets/audio/ambient/waves.ogg") loop;
stop ambient;
```

`ambient` は BGM とは別系統の音量で鳴る環境音です。素材ごとに鳴るので、雨と波のように重ねられます。
音量は設定画面の「環境音」で変えられます。

## 作品パッケージと表示

作品の設定は `aria.toml`、本文は `.aria`、見た目は `presentation.frontend` のパッケージに置きます。
通常の Native/Web でも、物語の VM と保存の意味は共有します。
React/Tauri のような作品側の表示パッケージは、入力から意味的な intent を送り、返された view model を描画します。
プラグインはこの責務分離を変更しません。

- Aria：本文、状態、演出、表示資源、画面の遷移。
- Plugin：独自画面の入力、待機、文字表示ポリシー。
- Presentation：DOM、字体、写真、レイアウト、フォーカス、アクセシビリティ。
- Host：素材の読み込み、デバイス入力、描画・音声・保存の具体的な入出力。

## 検査する

リポジトリのルートで実行します。

```sh
cargo run --locked -p aria-cli --no-default-features -- check examples/umikaze --release
cargo run --locked -p aria-cli --no-default-features -- run examples/umikaze --headless
```

## 改稿と保存

保存には、命令の番地に加えて、コンパイラが付けた位置の目印が入ります。
目印は、場面の先頭、選択肢、`call` の戻り先、本文の各行（場面名・話者・本文から作るキー）です。
同じバイトコードで読み込むと、保存した番地にそのまま戻ります。
改稿したバイトコードで読み込むと、同じ行、同じ場面の同じ番目の行、場面の先頭の順で位置を探し、その行から再開します。
場面ごと削除した場合は、どこへ戻すべきか決まらないので、読み込みを明示的に拒否します。

既読の判定も同じ行のキーで行います。
表示した順序ではないので、別の章から読み始めても既読がずれず、誤字を直した行以外は改稿後も既読のままです。

旧形式を現形式として読み替える実行経路はありません。
保存については、[プラグインと schema の互換条件](plugins.md#制限と保存互換)を確認してください。
