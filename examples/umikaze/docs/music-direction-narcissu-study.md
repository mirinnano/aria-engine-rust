# 海風 — ナルキッソス参照による音楽設計言語

> Status: direction study / reusable composition brief
>
> Scope: `ラムネ79's（ラムネより）`、`一号線`、`誰が為に`、`終末の過ごし方より`の構造研究、`120円の冬より`の編成思想と、海風への翻訳
>
> Non-goal: 既存曲の旋律、和声進行、音色、録音を再現すること

この文書は「ナルキッソスっぽい曲」を発注するための雰囲気メモではない。参照曲が物語の時間をどう分担しているかを抽出し、海風の各キューを一貫して設計・生成・選別するための判断規則にする。

先に結論を書く。

**ナルキッソス性は、暗いピアノや少ない音そのものではない。停止した生活、記憶になる日常、移動、不可逆な場面に、それぞれ異なる反復の仕方を与えていることにある。**

- 停止した生活は、薄いデジタルピアノと眠い高域の合成音を均等に反復し、途中で低域だけを加える。
- 日常は、歌える旋律を長い周期でほぼ同じように帰還させる。
- 移動は、ギターとドラムの少し軽快な歩幅から始め、短い句を保ちながら編成を広げる。
- 不可逆な場面は、同じ場所へ戻らず、途中の断絶を境に音を退かせる。

海風では、これらを一種類の「悲しいBGM」に平均してはならない。

## 1. 研究対象と根拠

公開情報上の作曲・編曲・収録時間は、[narcissu Music Soundtrack のクレジットと曲目](https://vgmdb.net/album/12608)および[公式 Steam 版 10th Anniversary Soundtrack](https://store.steampowered.com/app/627310/Narcissu_10th_Anniversary_Soundtrack/?l=japanese)を基準にした。

構造は、手元の正規ゲームデータに含まれる音源とスクリプトを対象に、次の方法で確認した。

- ゲーム内サウンドモード画像とスクリプトから曲名、音源、使用場面を対応づけた。
- `ffprobe` で尺、`aubio` で拍候補、`ffmpeg` で無音区間とラウドネス推移を測定した。
- スペクトログラム、音量包絡、音高クラスと帯域分布の自己相似を比較し、反復周期と大きな境界を求めた。
- 時刻は版や圧縮差を考慮し、原則として約1秒の幅を持つ設計値として扱う。

楽器名については、音源上の特徴だけで断定しない。とくに版差については、2006年のリスナーによる[サウンドトラック比較記録](https://www.ptt.cc/bbs/AC_Music/M.1150323392.A.BA9.html)を二次資料として参照した。同記録は、ナルキッソス版 `ラムネ79's` をピアノ版として捉え、`一号線` のサウンドトラック版にはゲーム開始位置より前のギター導入があるとしている。これは公式仕様ではなく、版差を考えるための補助証拠である。

`120円の冬より` は手元のゲームアーカイブには含まれないため、下表の実測対象には混ぜない。公開盤では2分33秒、藤間仁作曲・石橋弘史編曲の曲として収録されていることを、[公式 Steam 版の曲目](https://store.steampowered.com/app/627310/Narcissu_10th_Anniversary_Soundtrack/?l=japanese)と[公開クレジット](https://www.dojin-music.info/cd/21083)で確認した。この文書では、同曲を旋律や個別の楽器を採譜する対象ではなく、**複数の楽器が背景へ溶けず、一曲を演奏していると感じられる編成思想**の補助参照にする。

手元のゲーム内サウンドモードでは `終末の過ごし方より`、公開10周年盤では `週末の過ごし方` と表記される曲は、ゲーム内音源 `BGM/N04.MP3` を実測対象にした。公開盤では3分44秒、矢野雅士作曲・猫野こめっと編曲として収録されていることを[公開クレジット](https://www.dojin-music.info/cd/21083)で確認した。表記差を解消するため、この文書ではゲーム内表記を主に用いる。

参照音源、MIDI、譜面、旋律の採譜はリポジトリへ含めない。生成サービスへ参照音源をアップロードせず、以下の構造言語だけを渡す。

## 2. 実測の要約

| 曲 | ゲーム内音源 | 実測尺 | 拍の読み | 強い反復 | LRA | 末尾の無音 |
| --- | --- | ---: | --- | --- | ---: | ---: |
| `ラムネ79's（ラムネより）` | `BGM/N03.MP3` | 188.94秒 | 約88–90 BPMの体感拍。解析器は細分拍を倍速で拾う | 92.75秒後にほぼ同形で帰還 | 4.6 LU | 1.93秒 |
| `一号線` | `BGM/SEN02_20.MP3` | 157.05秒 | 約101 BPM | 約19.25秒の句、48秒と93.5秒の大きな対応 | 4.3 LU | 2.07秒 |
| `誰が為に` | `BGM2/2SEN02.MP3` | 159.16秒 | 約140.6 BPM。体感は70 BPMのハーフタイムにも取れる | 約13.75秒の句。長い全体反復は弱い | 5.7 LU | 4.33秒 |
| `終末の過ごし方より` | `BGM/N04.MP3` | 223.92秒 | 約150 BPMの細分拍。体感は75 BPM | 約102.25秒後に大単位が帰還。各周期の約52.7秒で低域と密度が変わる | 3.1 LU | 1.44秒 |

LRAとピーク値は古いMP3のマスタリングを示すだけであり、海風の納品音量の目標にはしない。ここで見るのは、参照曲が大音量の劇的変化ではなく、比較的小さな音量幅の中で編成と反復を変えている点である。

## 2.1 `120円の冬より` — 楽器を「背景素材」にしない

この補助参照から採るのは、曲調のコピーではなく楽器の存在のさせ方である。

海風の初期案は、RPGらしさ、陽気さ、過剰な悲劇性を避けるために音数を減らしすぎた。その結果、ピアノ単音、遠い倍音、ドローン、ノイズといった「雰囲気を示す素材」が増え、誰が何を演奏しているのか分からない曲へ寄りやすくなった。

今後は、静けさと楽器感を対立させない。

- 前景楽器は、アタック、音の胴、減衰まで聞こえる録音にする。
- 伴奏楽器も「薄い色」ではなく、低音を受け持つ、対旋律を返す、長い音価を支えるなど、演奏上の仕事を持つ。
- 一つの楽器を消したとき、編曲上の役割が一つ失われる状態を作る。
- シンセパッド、環境ノイズ、残響は、演奏を代替せず、複数楽器の背後に一層だけ置く。
- 文学的な静けさは、楽器を遠ざけることではなく、演奏者が休符を選ぶことで作る。

### 楽器感のモード

これは物語上の時間系統とは別の軸である。各キューは時間系統と楽器モードを一つずつ持つ。

| 楽器モード | 編成の原則 | 適する場面 |
| --- | --- | --- |
| `played-ensemble` | 前景1、明確な伴奏2～3。各楽器が独立した奏法と役割を持つ | 移動、二人の会話、外界が開く場面、関係が一歩動く場面 |
| `intimate-solo` | ピアノなど前景1を近く置き、応答楽器は0～1 | 一人の観測、雨の室内、声量の小さい会話 |
| `ensemble-to-solo` | 複数奏者で始まり、物語上の蝶番を境に前景だけを残す | 告白、断念、別れ、不可逆な選択 |
| `soft-digital` | 薄いデジタルピアノ、高域の無言語ヴォイス系シンセ、後半だけ入る低音。均等な反復と丸い発音で、浅い眠気を作る | 病室の反復、季節の省略、生活が止まって見える場面 |

`played-ensemble` でも全員が常時鳴ってはならない。`intimate-solo` でも楽器を残響へ溶かしてはならない。`soft-digital` は音源を安価に偽装する規則ではなく、薄い合成音色と均等な歩幅によって、優雅な独奏から距離を取り、病室の眠い時間を作る規則である。

## 2.2 `終末の過ごし方より` — 美しく弾かず、生活の停止を反復する

### 物語上の位置

ゲーム内では、入院後の夏、見舞いが減っていく時間、深夜の補助ベッド、過去の入院、談話室、地図、雨の季節などに繰り返し使われる。決定的な死の場面だけを飾る曲ではない。時間が進んでいるのに、本人の生活だけが同じ場所へ戻る場面をつなぐ。

これは海風の序章に近い。ミオの病室を「優雅に失われた記憶」として先に美化せず、窓、テレビ、見舞い、季節だけが入れ替わる生活として置ける。

### 時間構造

体感は約75 BPMで、解析器はその二倍の約150 BPMを拾う。音量幅は3.1 LUと狭く、演奏の大きな抑揚ではなく、同じ歩幅の中で低域と密度を変える。

| 時刻 | 機能 |
| --- | --- |
| 0:00–0:08 | 短い発音と反復規則を提示する |
| 0:08–0:53 | 同じ句を保ち、感情的な歌い上げを避ける |
| 0:53–1:41 | 低域と密度を加えるが、テンポと基本動機は変えない |
| 1:41–1:42 | 大単位の境界を作る |
| 1:42–3:23 | 約102.25秒の単位を、対応する密度変化ごと再び通る |
| 3:23–3:44 | 帰還後の短い終結と、約1.4秒の無音へ渡す |

### 海風へ移すもの

- 独奏者の呼吸ではなく、75 BPMの均等な生活周期を前へ出す。
- 一周の前半は薄いデジタルピアノと高域の合成音だけにし、約半分を過ぎてから低音を加える。
- 旋律は歌い上げず、同じ四～六音の輪郭を少しずつ言い直す。
- 生ピアノとチェロの室内楽にはしない。病室を高雅な回想へ変える長い残響、ルバート、弓の情緒を避ける。
- 初期PCノベルの合成音色と少し眠い距離感は使うが、ビットクラッシュ、レコードノイズ、意図的な低音質で「レトロ」を記号化しない。

海風では既存曲の音色を複製せず、**薄いデジタルピアノ、丸くぼやけた高域の無言語ヴォイス系シンセ、後半だけ入る低音、狭い音量差**へ翻訳する。高音は眠気を作る持続と反復に使い、ベルやオルゴールの鋭いアタックにはしない。人の声を録音せず、歌詞や発語を持たせない。

## 3. `ラムネ79's` — 日常を、後から記憶に変える

### 物語上の位置

ゲーム内では、病室の放送、リンゴをむく会話、談話室、雨の車内、夕暮れの迷いなどに置かれる。決定的な台詞の専用曲ではない。先に生活の手触りへ入り、読後に「あれは失われる日常だった」と意味を変える。

ここが重要である。

**曲そのものを悲嘆にしすぎず、平凡な場面を覚えておける形にする。悲しさは、曲ではなく再出現した文脈が完成させる。**

### 時間構造

ローカル音源では、約92.75秒の大きな単位が、後半でもほぼそのまま繰り返される。特徴量の平均類似度は0.967であり、実測した参照曲中でもっとも明確な全体帰還を持つ。

| 時刻 | 機能 |
| --- | --- |
| 0:00–0:22 | 主旋律と日常の歩幅を提示する |
| 0:22–0:47 | 音域または上層を広げ、同じ生活を少し明るく見せる |
| 0:47–1:05 | 新しい事件を起こさず、提示した歩幅を保つ |
| 1:05–1:23 | 低域と密度を引き、先に余韻を作る |
| 1:23–1:33 | 終止と再開のための空間を置く |
| 1:33–3:09 | 約92.75秒の単位を、ほぼ同形で再び通る |

細部は約11秒単位で対応しやすい。4/4・約88 BPMとして読めば、およそ4小節の呼吸に近い。短い1～2小節ループではなく、旋律が一度生活して帰ってくる長さを持つ。

### 音楽的な働き

- 前景はピアノを中心に、一度で覚えられる旋律を置く。
- リズム隊で日常を表さず、フレーズの呼吸で時間を進める。
- 中盤で一度だけ音域または色を広げる。
- 後半に向けて足し続けず、最初の周期のうちに引き算を始める。
- ループを隠すために無限化するのではなく、長い一周をきちんと終えてから帰還する。

### 誤読するとどうなるか

- ピアノを遅く、暗く、残響深くするだけでは「病気を説明する曲」になる。
- オルゴール、高い単音、雨音、ローファイノイズを重ねると、既製の回想記号になる。
- 明るい和音と軽いパーカッションを足すと、日常ではなく日常系アニメの陽気さになる。

海風へ移すのはピアノ音色ではなく、**平凡な場面に先回りして旋律を置き、後の文脈に意味を変えさせる設計**である。

## 4. `一号線` — 速さではなく、止まらないことを鳴らす

### 物語上の位置

ゲーム内では、雨上がりのアスファルト、流れる雲、走行中の車、高速道路、山道、別れ、旅の終わりに使われる。単なる出発曲ではない。風景が変わり続ける場面と、もう戻れないと理解する場面の双方を接続する。

車のSEや背景切替を消して曲だけで移動を説明するのではない。物理的な移動は車輪、雨、道路、画面が担い、曲は「それでも走り続ける」という時間の連続性を担う。

### 時間構造

約101 BPMで、約19.25秒の句が強く対応する。4/4として読めば、およそ8小節の文に相当する。さらに48秒、93.5秒にも大きな対応があり、同じ材料を異なる大きさで言い直す階層的な構造を持つ。

| 時刻 | 機能 |
| --- | --- |
| 0:00–0:48 | 動機と走行の規則を提示する第一単位 |
| 0:48–1:29 | 同じ規則を保ったまま、密度と音域を拡張する第二単位 |
| 1:29–1:36 | エネルギーを大きく落とし、路面が一度途切れるような呼吸を作る |
| 1:36–2:34 | 冒頭と対応する材料を帰還させ、終点へ運ぶ |
| 2:34–2:37 | 音価を解放し、約2秒の無音へ渡す |

サウンドトラック版とゲーム使用版には導入位置の差があるとする二次記録があり、ギター導入の存在も報告されている。この差は、曲の「完全版」をそのまま鳴らすことより、場面が必要とする地点から入る編集が優先された可能性を示す。

### 音楽的な働き

- 移動感は、速いBPMやドラムだけに任せず、ギターの短いアタック、ドラムの歩幅、8小節句の継続を組み合わせて作る。
- 同じ動機を保ちつつ、48秒規模で編成を入れ替える。
- 中央付近の呼吸で、移動を一度「距離」として知覚させる。
- 最後までクレッシェンドせず、帰還した材料を終点へ置く。
- ギターは走行の標識として使えるが、常時ストロークして速度を説明させない。

### 誤読するとどうなるか

- アコースティックギターを刻み続けると、陽気なロードムービーまたはキャンプになる。
- シェイカー、ハイハット、手拍子を足すと「シャカシャカした移動曲」になる。
- ストリングスを上昇させ続けると、RPGのフィールド曲や旅立ちイベントになる。
- 車輪音をそのままビートにすると、場面のSEと競合する。

海風へ移すのは「ギターの旅曲」ではない。**8小節の規則をやめず、編成だけを変え、途中で一度だけ道路を無音にする設計**である。

## 5. `誰が為に` — 頂点の後を長く残す

### 物語上の位置

ゲーム内では、答えにくい問い、別れを予感する会話、泣き声、7Fの風景など、人物の選択が元へ戻らない場面へ置かれる。日常曲のように場面を包むのではなく、場面の中に一本の境界を作る。

スクリプト上でも、視覚変化や待ちを挟んだ後に曲が入る箇所がある。感情的な台詞と同時に大きく鳴らすのではなく、画面と沈黙が先に不可逆性を作り、その後から曲が意味を固定する。

### 時間構造

拍解析は約140.6 BPMを示すが、体感上は約70 BPMのハーフタイムとしても扱える。約13.75秒の対応は、4/4なら8小節に近い。

`ラムネ79's` のような全体のほぼ完全な再演はなく、`一号線` のような長い大ブロック対応も弱い。短い句を受け継ぎながら、全体は一方向へ変形する。

| 時刻 | 機能 |
| --- | --- |
| 0:00–0:16 | 音の規則を限定して提示する |
| 0:16–0:40 | 主たる声部と場面の重さを確立する |
| 0:40–1:26 | 句を継承しながら密度を上げる。頂点は終端より前に置く |
| 1:26–1:37 | 減速し、1:35–1:36付近に明瞭な断絶を作る |
| 1:37–2:10 | 断絶以前の材料を、そのまま戻さずに言い換える |
| 2:10–2:35 | 密度を段階的に退かせる |
| 2:35–2:39 | 約4.3秒の無音を残す |

### 音楽的な働き

- 高速な表面ではなく、ハーフタイムで大きな呼吸を感じさせる。
- 約8小節の句は保つが、曲全体を同じ形で循環させない。
- 感情の最大点を最後へ置かない。最大点の後に、結果を受け取る時間を残す。
- 中央より後ろに、編成が一度切断されたと分かるほどの蝶番を置く。
- 終止は解決を宣言せず、長い末尾無音へ譲る。

### 誤読するとどうなるか

- 冒頭から短調の弦を厚くすると、悲劇の予告編になる。
- 最後の台詞に合わせて最大音量へ達すると、感情の答えをプレイヤーへ強制する。
- 30秒程度の悲しいループにすると、不可逆性ではなく持続する気分になる。
- 全編を無拍のドローンにすると、断絶の前後が区別できない。

海風へ移すのは「深刻な曲調」ではない。**頂点を早めに通過し、断絶の後を曲の主要部分として扱う設計**である。

## 6. 参照曲を一つの様式へ平均しない

| 系統 | 参照上の役割 | 反復 | 主な時間感覚 | 海風で担うもの |
| --- | --- | --- | --- | --- |
| `stalled-routine` | `終末の過ごし方より` | 約102秒の大単位と、中間点の密度変化 | 季節は進むが、生活は同じ場所へ戻る | 序章の病室、長い療養、季節の省略 |
| `ordinary-memory` | `ラムネ79's` | 約90秒の長い全体帰還 | 生活がまた同じ場所へ戻る | 病室、雨宿り、食事、何気ない会話 |
| `road-continuity` | `一号線` | 約8小節句と大ブロックの変奏 | 景色は変わるが移動は止まらない | 列車、道路、海岸線、場所の移行 |
| `irreversible-question` | `誰が為に` | 短い句だけを継承し、全体は一方向 | 出来事の前後が同じでなくなる | 告白、断念、別れ、灯台、結末前 |

各系統に共通するのは次の点だけである。

1. 一度で識別できる旋律または動機を持つ。
2. 2～4小節の短い素材を、8小節以上の文へ育てる。
3. 音量の巨大な起伏ではなく、楽器の出入りと反復の差で語る。
4. 前景楽器を一つに絞り、伴奏奏者には互いに異なる仕事を与える。
5. 曲の終端に、文章が戻ってくる空間を残す。
6. 場面の感情を先に説明せず、再出現によって意味を変える。

## 7. 海風の音楽方針

### 7.1 基本編成

楽器の階層は次を初期値とする。

- 病室の長い反復: 生ピアノと弦の室内楽を避け、薄いデジタルピアノ、高域の無言語ヴォイス系シンセ、後半だけ現れる低音による `soft-digital` を使う。
- 旅の中の室内と日常: ピアノの打鍵、胴鳴り、減衰が分かる `intimate-solo` を使える。必要ならクラリネット、チェロ、ヴィオラのいずれか一つが短く応答する。
- 移動と風景: クリーンギターと乾いたドラムキットから入り、移動が成立した後にピアノ、ベース、弦または木管を順番に加える `played-ensemble` とする。
- 二人の距離が近づく場面: ピアノとギターを主従のある二声として扱い、低音楽器が和声の移動だけを支える。
- 不可逆な場面: `ensemble-to-solo` を使い、ピアノまたはギターの既出動機だけを蝶番の後へ残す。
- 弦: 単なる遠いパッドにせず、長い音価、対旋律、低音のいずれを担当するか決める。常時の感情増幅には使わない。
- 木管: 人の息が必要な場面に限り、一曲一種類まで使う。旋律の二重化ではなく、句末への返答にする。
- 打楽器: 移動曲では例外的に、冒頭から乾いたキック、柔らかいスネア、低く混ぜたハイハットまたはライドを使える。二小節以上まったく同じループを貼らず、フィルではなく休符と弱拍の差で句を示す。移動曲以外では常用しない。シェイカーと高域だけが残る連続刻みは禁止する。
- シンセとノイズ: 演奏の主役にも伴奏の代用品にもしない。使う場合は一層までとし、消しても和声とフレーズが成立すること。

これは全曲を同じ編成にする規則ではない。一曲につき主役を一つに限定しながら、場面によって「一人の演奏」か「複数人の演奏」かを明確に選ぶ規則である。

### 7.2 移動曲の入口と編成展開

移動曲は、海風の中で「少し軽快」を明示的に許す領域である。ただし、明るさや速さを旅の意味にしない。足元が動き始めたという身体感覚をドラムとギターが担い、ピアノ以降の楽器は、窓の外に見える景色と二人の会話が広がる順序を担う。

標準形は次のとおりとする。

| 区間 | 入る楽器 | 役割 |
| --- | --- | --- |
| 0–8小節 | クリーンエレクトリックギター、乾いたドラムキット | ギターが短いリフまたは分散した二音を提示し、ドラムが移動の歩幅を作る |
| 8–16小節 | エレキベース | キックと同じ音をなぞらず、和声の移動と低い重心を加える |
| 16–32小節 | ピアノ | ギターを二重化せず、長い音価、低い和音、短い対旋律のいずれかで景色を開く |
| 32小節以降 | ヴィオラ、チェロ、クラリネットのうち一つ | 句末への返答または一度だけ現れる第二の旋律として、人の気配を加える |
| 全体の55–65% | 編成を二人まで減らす | 道路や線路が一度途切れたような呼吸を置く |
| 帰還 | ギター、ドラム、ピアノを中心に再構成 | 冒頭をそのまま大音量にせず、同じ道の続きを示す |

編成は最大五人程度に留め、追加楽器は大きな句の境界で一つずつ入れる。ドラムとギターを冒頭から使うことは、陽気なドライブ曲にする許可ではない。

- ギターは常時の明るいコードストロークではなく、短いリフ、単音、二音、休符で動く。
- ドラムは軽いロックまたはポップの身体性を持ってよいが、派手なフィル、四つ打ち、クラッシュの煽りを使わない。
- ピアノは主旋律を奪わず、入った瞬間に「編成が豪華になった」より「窓の外が広がった」と感じさせる。
- 弦と木管は厚みを足すパッドではなく、後から参加した一人の奏者として聞こえるようにする。
- 列車や車輪の周期をドラムで模倣しない。物理的な移動はSE、音楽上の継続は演奏が担う。

### 7.3 既存キューへの割り当て

| Cue ID | 時間系統 | 楽器モード | 改訂する方向 |
| --- | --- | --- | --- |
| `umk.title.record` | silence | silence | 曲を置かない。タイトルの引用地形と海景へ音楽的な意味を足さない |
| `umk.ward.first-light` | `stalled-routine` | `soft-digital` | 75 BPM。薄いデジタルピアノと、眠気を誘う高域の無言語ヴォイス系シンセで、窓、テレビ、見舞い、季節が同じ場所へ戻る規則を作る。ギターと生ピアノ独奏は使わない |
| `umk.rail.departure` | `road-continuity` | `played-ensemble` | 約100–106 BPM、8小節句。ギターと乾いたドラムから入り、ベース、ピアノ、色彩楽器を順番に加える。少し軽快だが、冒険や陽気さにはしない |
| `umk.rain.room` | `ordinary-memory` の影 | `intimate-solo` | 72 BPM。少し古びたエレクトリックピアノの未完五音句に、低いクラリネットが選んだ句末だけを返す。雨SEを伴奏化せず、濡れた情緒を音楽で説明しない |
| `umk.recording.trace` | `irreversible-question` の短形 | `ensemble-to-solo` | 体感70 BPM、約86秒の非ループ。指で弾くクリーンエレクトリックギターへ乾いたピアノが返し、約56%の完全な蝶番後はギター一人だけを残す |
| `umk.clear-between` | `road-continuity` の軽形 | `played-ensemble` | 102 BPM。ギターと軽い乾いたドラムで入り、8小節後にピアノ、16小節後にベース、32小節後にヴィオラを一度だけ加える。晴れを大音量や明るいコードで宣言しない |
| `umk.island.distance` | hybrid | `played-ensemble` | 96 BPM、約190秒の非ループ。ナイロン弦ギターと軽いドラムで始め、ピアノを加え、約52%でドラムを永久に抜く。一句後から低いチェロが句末だけを返す |
| `umk.north.grey` | `road-continuity` → `irreversible-question` | `played-ensemble` | 92 BPM、約180秒の非ループ。ギターと低いドラムの歩幅を保ちながら最高音とハイハットを一方向に失い、低いピアノとチェロへ重心を移す |
| `umk.lighthouse.edge` | `irreversible-question` | `ensemble-to-solo` | 70 BPM、約165秒の非ループ。ギター、乾いたピアノ、低いチェロで始め、約60%の1.8秒完全無音後は最高音を欠いたギター一人だけを残す |
| `umk.spring.after` | `ordinary-memory` の残像 | `intimate-solo` | 68 BPM、約132秒の非ループ。乾いたアップライトピアノが六音句を不完全に戻し、テナー域のバスーンが中盤に一度だけ四小節で答える。ギターは使わない |

### 7.4 曲と場面の入口

曲の良し悪しだけでなく、入る位置を設計する。

#### 日常

1. 空調、雨、食器、車内などのSEを先に聞かせる。
2. 感情的でない具体的な動作を一つ置く。
3. その動作の直前または直後から曲を入れる。
4. 後の重要台詞では、曲を切り替えず同じ日常曲を残す。

#### 移動

1. 背景をフェードさせる。
2. 車輪、道路、風などの物理音を先に置く。
3. 0.5～1.5秒の無音またはSEだけの時間を作る。
4. 移動が始まった後から8小節句へ入る。

出発ボタンや章扉と同時に勇ましく始めない。

#### 不可逆

1. 以前のBGMを1.2～2.0秒で止める。
2. 1.0～2.5秒、環境音または完全な無音だけを残す。
3. 背景、単色、人物の不在など、視覚上の事実を一つ保持する。
4. 新しい曲を小さく入れる。
5. 重要な一文は、曲の開始ではなく最初の句が成立した後へ置く。

曲の頂点と最重要文を常に同期させない。同期は一作品中で数回しか使えない強い手段として保留する。

## 8. 再利用用 Composition Brief

各曲の依頼前に、次を埋める。空欄のまま生成しない。

```yaml
music_brief:
  cue_id:
  narrative_function: stalled-routine | ordinary-memory | road-continuity | irreversible-question | silence
  scene_before:
  scene_after:

  instrument_presence: played-ensemble | intimate-solo | ensemble-to-solo | soft-digital
  foreground_instrument:
  supporting_instruments:
  player_roles:
    foreground:
    harmony:
    low_end:
    response:
  articulations:
  audible_instrument_traits:
  forbidden_instruments:
  synth_role: none | background-only | foreground-soft
  orchestration_route:
    opening_players:
    first_entry:
    second_entry:
    color_entry:
    reduction_players:
    return_players:

  felt_tempo_bpm:
  meter:
  phrase_length_bars:
  full_form_seconds:

  motif:
    character: singable | fragmentary | withheld
    first_statement_seconds:
    return_policy: exact | varied | no-full-return

  density_curve:
    opening:
    expansion:
    hinge:
    aftermath:

  runtime:
    entry_after_event:
    loop: full-form | one-restart-max | no-loop
    fade_in_ms:
    fade_out_ms:
    required_silence_after_ms:

  ending:
    cadence: open | suspended | incomplete
    final_tail_seconds:

  rejection_risks:
    - rpg-field
    - cheerful-road-movie
    - trailer-tragedy
    - lo-fi-study-beat
    - generic-sad-piano
```

## 9. 生成プロンプトの組み立て

生成AIへ作品名や作曲者名を渡さない。観測可能な構造へ翻訳し、既存旋律を引用しないことを明記する。

### 全系統で固定する部分

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a new, clearly identifiable motif; do not quote or recreate any existing melody.
Use one unmistakable foreground player and, when the scene calls for an ensemble,
two or three clearly identifiable supporting instruments.
Every supporting instrument must have a named musical job: harmonic anchor,
low-end movement, counter-line, or phrase-ending response.
Preserve the natural attack, body, articulation, and decay of each instrument.
Do not dissolve the players into an indistinct ambient wash.
Emotion must emerge from repetition, subtraction, and scene context rather than a large cinematic crescendo.
Leave room for spoken Japanese dialogue and environmental sound.
Dry, close recording; modest stereo width; restrained reverb.
No vocals, no heroic cadence, no trailer riser, no fantasy/RPG orchestration,
no continuous shaker or closed hi-hat, no cheerful folk strumming,
no lo-fi beat, no music-box cliché, no wall-to-wall ambient pad.
```

### `played-ensemble`

`120円の冬より` から抽出した「演奏者が見える」側の発注に使う。曲名は生成AIへ渡さない。

```text
Make the cue feel performed by a small group of real players, not assembled from atmospheric layers.
Choose one lead instrument and two or three supporting instruments with different roles.
Let the lead state the motif, let one instrument answer only at phrase endings,
let one instrument carry slow bass movement, and let the remaining player sustain or punctuate harmony.
Use rests and instrumental hand-offs so the listener can follow who is playing.
Keep attacks and natural decays audible.
The ensemble may become warm and melodic, but never heroic, lush, or symphonic.
Do not keep every player active at once.
No anonymous string pad, no constant cymbal texture, and no percussion loop used as glue.
```

### `intimate-solo`

`ラムネ79's` から抽出した「一人の打鍵が生活になる」側の発注に使う。

```text
Place one acoustic instrument close to the listener.
Its finger, key, string, breath, attack, body, and decay should define the cue,
without exaggerated mechanical noise or ASMR detail.
Use zero or one response instrument, entering only at selected phrase endings.
Do not fill the empty register with a pad.
Silence between phrases is part of the performance.
The cue should feel like one person continuing to play in an ordinary room,
not like an ambient soundtrack describing loneliness.
```

### `ensemble-to-solo`

```text
Begin as a restrained small ensemble with one clear lead and two supporting players.
At the structural hinge, stop the supporting parts rather than fading the entire mix into ambience.
After the hinge, leave the lead instrument physically present and exposed.
The listener should recognize that other players have stopped.
Do not replace the missing instruments with reverb, drones, or noise.
```

### `soft-digital`

`終末の過ごし方より` から抽出した「生活が均等に戻る」側の発注に使う。曲名、和声進行、旋律、原音の音色は生成AIへ渡さない。

```text
Use a thin, softly synthesized digital piano and a rounded high-register
wordless vocal-like synth tone, without any real voice, lyrics, or speech.
The high tone should feel gently drowsy rather than sparkling, angelic, or dramatic.
Keep a steady 75 BPM half-time walk and repeat a plain four-to-six-note contour
with very little expressive rubato.
For the first half of the long cycle, use only the digital piano figure,
the high synthetic tone, and sparse harmonic punctuation.
Around the midpoint, add one rounded low voice and slightly greater density,
without introducing a new emotional theme or a cinematic lift.
Keep the loudness range narrow.
The result should feel like days continuing in the same room,
not like a performer elegizing a remembered life.
No guitar, acoustic grand or upright piano, solo cello, chamber-music elegance,
bright bell, music box, real choir, lo-fi degradation, chiptune, or ambient wash.
```

### `ordinary-memory`

```text
Foreground: intimate acoustic piano with audible attack, body, and natural decay.
Felt tempo: 88–90 BPM, 4/4.
Build a singable four-bar idea into a long 85–95 second cycle.
State it plainly before adding any emotional color.
Expand the register once around the first third, then begin subtracting layers before the final third.
Allow the full cycle to finish and breathe before it returns.
The scene should feel ordinary while it is happening and precious only in retrospect.
For private scenes, use intimate-solo.
For shared everyday scenes, keep the same form but use a small played-ensemble.
```

### `road-continuity`

```text
Tempo: 100–106 BPM, 4/4, slightly brisk but never cheerful or heroic.
Open with clean electric guitar and a dry, human drum kit for the first eight bars.
The guitar plays a memorable short riff, single notes, and short dyads with clear rests;
never use continuous bright strumming.
The drum part uses a restrained kick, soft snare, and low-mixed hi-hat or ride.
It may establish a real groove, but it must breathe with each phrase
instead of repeating an unchanged stock loop.
Bring in a rounded electric bass during the next eight bars.
After movement is already established, introduce piano as a separate player:
longer harmonic punctuation, low open voicings, or a short counter-line,
never a doubled copy of the guitar motif.
In a later large section, add only one color player—viola, cello, or clarinet—
for phrase-ending answers or one secondary line.
Use a memorable eight-bar sentence and vary the orchestration in larger 40–50 second blocks.
Keep the ensemble to roughly five identifiable players.
Create forward continuity through the relationship between the groove and instrumental hand-offs.
Around 55–65% of the form, reduce the group to two players or a near-stop,
then rebuild the opening material without a louder final chorus.
The feeling is not adventure or optimism; it is the fact that the road keeps continuing.
No four-on-the-floor kick, flashy drum fill, crash-cymbal lift, shaker,
campfire strumming, pop-rock chorus, RPG field theme, or triumphant travel montage.
```

### `irreversible-question`

```text
Foreground: a previously heard piano or guitar motif, altered and incomplete.
Pulse: 140 BPM grid perceived in half-time around 70 BPM, 4/4.
Keep local eight-bar continuity but do not repeat the whole form unchanged.
Reach the greatest density before the final third.
Create a clear one-to-two second hinge of near-silence around 60%,
then devote the remaining form to a reduced aftermath.
End with an unresolved cadence and at least four seconds of air.
The music must not announce tragedy; it must make the scene before and after the hinge feel unequal.
```

## 10. 採否チェック

### 10.1 音楽単体

- 一度聞いた後、既存曲ではない短い動機を口で再現できるか。
- 前景楽器を一つ答えられるか。
- 目を閉じたまま、伴奏を含む各楽器名と役割を答えられるか。
- 打鍵、撥弦、弓、息など、選んだ楽器固有の発音が残っているか。
- 伴奏を一つミュートしたとき、単に厚みではなく音楽上の仕事が失われるか。
- パッド、残響、ノイズを消しても旋律、和声、低音、休符が成立するか。
- 15秒ごとに新しい楽器を足していないか。
- 全奏者が常時鳴らず、誰かが休むことで編成が見えるか。
- 最終30秒が最大音量になっていないか。
- 終止の後に、本文が戻れる空間があるか。
- 既存参照曲の旋律、特徴的なリズム列、録音を連想できるほど再現していないか。

### 10.2 読書面との併用

- 小音量にしたときも、ハイハットや高域の反復だけが残らないか。
- 日本語の子音と競合する2–6 kHzのアタックが常時鳴っていないか。
- 車輪、雨、風、海のSEと同じ周期を刻んでいないか。
- 曲を外しても場面の意味は成立し、曲を戻すと時間だけが深くなるか。
- 重要な一文を曲が感情語へ翻訳しすぎていないか。

### 10.3 移動曲

- 最初の八小節だけで、ギターの句とドラムの歩幅を別々に認識できるか。
- ピアノが後から入り、単なる音量増加ではなく視界の広がりを作っているか。
- ベースがキックを常時なぞらず、和声の移動を担当しているか。
- 色彩楽器が一度に複数入らず、誰が新しく参加したか耳で追えるか。
- ドラムに小節ごとの演奏差があり、短い素材を貼っただけのループに聞こえないか。
- 「少し軽快」が、陽気、青春、冒険、勝利のいずれにも変換されていないか。
- 55–65%付近で編成が減り、景色を受け取る空間が生まれているか。

### 10.4 失敗判定

次のどれかを最初の20秒で感じた場合は、部分修正より再生成を優先する。

- 地図を開いて冒険へ出るRPGフィールド曲
- 陽気なドライブ、キャンプ、青春ロードムービー
- シェイカーやハイハットが前景に残る「シャカシャカ」
- 冒頭から最後まで暗いだけのドローン
- 楽器名を答えられない薄いパッドと倍音だけの曲
- ピアノが遠い残響の発生源にしかなっていない曲
- 小編成の全員が同じリズムでコードを重ね続ける曲
- 映画予告のような弦の上昇と大団円
- 雨、オルゴール、テープノイズに頼った既製の回想

## 11. メタ認知的な修正記録

最初の判断では、ナルキッソスを「少ない楽器、暗い感情、ピアノまたはギター」という音色の集合として捉えていた。その抽象化では、生成結果が次の両極へ崩れた。

- 移動を表そうとして、RPGらしい前進感や陽気な刻みを足す。
- それを抑えようとして、今度は全編を暗く、遅く、出来事のない曲にする。
- 楽器を増やす危険ばかり警戒し、演奏者の気配まで消してしまう。

問題は生成AIの性能だけではなく、発注側が「何のために反復するのか」を指定していなかったことにある。

参照曲を比較すると、同じ作品内でも暗さ、テンポ、楽器は統一されていない。統一されているのは、物語上の役割に応じて時間を割り当てる態度である。

- `終末の過ごし方より` は、均等に戻るから生活の停止になる。
- `ラムネ79's` は、戻ってくるから記憶になる。
- `一号線` は、言い換えながら続くから距離になる。
- `誰が為に` は、同じ場所へ戻らないから決断になる。

さらに、`120円の冬より` 側の小編成、`ラムネ79's` 側の近いピアノ、`終末の過ごし方より` 側の乾いた均等な反復は、静けさの異なる作り方を示す。複数人が互いの休符を聞く静けさ、一人の演奏が部屋の時間になる静けさ、演奏者の感情より生活周期が前へ出る静けさである。いずれも楽器を曖昧な背景へ溶かすことではない。

したがって、今後「ナルキッソス的」と言うとき、それを一つのスタイルプリセットとして扱わない。まず場面を時間系統のどこへ置くか決め、次に楽器モードから演奏者の人数と距離を決め、その後に楽器、テンポ、密度を選ぶ。

海風が借りるべきものは、既存曲の表面ではない。

**文章の外側に、文章とは異なる長さの時間を置く技術である。**

## 12. 最初の再生成ブリーフ — `umk.ward.first-light`

このキューIDの `first-light` は制作上の識別子であり、本文の時刻を意味しない。実際の場面は、曇った春の昼、病室の窓、桜、母親の見舞いである。

最初の案は、生ピアノとチェロによって場面を「美しく失われた時間」へ先回りさせ、優雅すぎた。改訂版は `stalled-routine` と `soft-digital` を組み合わせる。灰色の独白までは無音にし、「私はいつものように、窓のむこうを見る。」から曲を入れる。曲が担うのは死や母娘の感動ではなく、同じ部屋へ戻る生活の周期と、浅く眠りかける感覚である。

```yaml
music_brief:
  cue_id: umk.ward.first-light
  narrative_function: stalled-routine
  scene_before: silence through the grey-screen statement
  scene_after: ward window, cherry blossoms, mother's visit, decision to go home

  instrument_presence: soft-digital
  foreground_instrument: rounded high-register wordless vocal-like synthesizer
  supporting_instruments:
    - thin early-digital piano
    - soft synthesized bass, entering only after the cycle midpoint
  player_roles:
    foreground: sustains and gently repeats a plain four-to-six-note contour in the upper register
    harmony: digital piano provides sparse broken intervals and two-note punctuation
    low_end: synthesized bass marks slow harmonic movement only in the denser half
    response: high synth and digital piano leave space for one another
  articulations:
    - rounded non-breathy synth attack with audible note boundaries
    - light digital-piano attacks without acoustic hammer realism
    - nearly even timing with clear rests and only small human variation
  audible_instrument_traits:
    - the high tone remains soft and sleepy without becoming a pad
    - the piano sounds synthesized and thin rather than elegant or realistic
    - the bass entrance changes weight without creating momentum or a climax
  forbidden_instruments:
    - guitar
    - acoustic grand piano
    - acoustic upright piano
    - felt piano
    - solo cello
    - expressive strings
    - real voice
    - real choir
    - music box
    - bright bell
    - glass mallet
    - shaker
    - hi-hat
  synth_role: foreground-soft

  felt_tempo_bpm: 75
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 220

  motif:
    character: fragmentary
    first_statement_seconds: 8
    return_policy: exact

  density_curve:
    opening: high wordless synth and sparse digital-piano punctuation
    expansion: soft synthesized bass enters near the midpoint without a new theme
    hinge: brief thinning near the end of the long cycle
    aftermath: a near-identical second cycle, followed by a short reduced coda

  runtime:
    entry_after_event: begin after "私はいつものように、窓のむこうを見る。"
    loop: full-form
    fade_in_ms: 700
    fade_out_ms: 900
    required_silence_after_ms: 900

  ending:
    cadence: incomplete
    final_tail_seconds: 1.5
```

生成時は、作品名や参照曲名を外して次をそのまま使う。

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a new melody and do not quote, imitate, or reconstruct any existing composition.

The cue begins after silence in an overcast hospital room during spring daytime.
A girl looks through the same window again. Cherry blossoms, television,
visits, and seasons change, but her daily life keeps returning to the same bed.
Do not make the room beautiful, graceful, nostalgic, tragic, or consoling.
The music represents routine continuing, not a memory being mourned.

Use two core colors associated with restrained early PC visual-novel music.

First, use a rounded high-register wordless vocal-like synthesizer:
an old digital voice or vowel-shaped ROMpler-style tone, but with no real singer,
no lyrics, no speech, no choir, and no recognizable human breath.
Let it play soft sustained notes with clear beginnings and endings.
It should feel slightly sleep-inducing, as if someone might drift off
while the television remains on in the next room.
It must not sound angelic, sacred, sparkling, ghostly, or cinematic.

Second, use a thin early-digital piano.
The piano should sound intentionally synthesized rather than like a realistic
grand, upright, or felt piano. Give it light attacks, limited resonance,
simple broken intervals, and sparse two-note harmonic punctuation.
It supports and occasionally answers the high synth instead of performing
an elegant solo.

Do not use guitar in this cue.

Felt tempo 75 BPM, 4/4, with a stable half-time walk.
Write an original four-to-six-note contour using small intervals,
soft sustained upper notes, sparse digital-piano figures, clear rests,
and very little expressive rubato.
It should be recognizable without becoming a lyrical solo melody or lullaby.
Write a new harmonic sequence; do not reuse the harmony of any reference track.

Build one long cycle of roughly 95 to 105 seconds.
For approximately the first half, use only the high synthetic voice-like tone
and sparse digital-piano punctuation.
Near the midpoint, introduce one very soft rounded synthesized bass voice
carrying slow harmonic movement. The bass must add weight, not momentum or climax.
Do not add a new emotional theme.
Thin the texture briefly near the end of the cycle, then repeat the long cycle
almost unchanged. Finish with a reduced 15-to-20-second coda and an incomplete cadence.
Total duration about 3 minutes 35 seconds to 3 minutes 45 seconds.

Leave space for softly spoken Japanese dialogue.
Keep the loudness range narrow, the stereo field modest, and reverb short.
The cue should feel structurally steady even when the reader advances at an irregular pace.

No guitar. No real vocals, lyrics, speech, realistic choir, or human breathing.
No acoustic piano solo, felt piano, solo cello, expressive string section,
chamber-music elegance, jazz lounge harmony, music box, bright bell, glass mallet,
fairy-like sparkle, sacred or angelic choir, ambient wash, drone, field recording,
hospital beeps, air-conditioner noise, rain, vinyl crackle, lo-fi treatment,
chiptune, cinematic swell, trailer crescendo, heroic cadence, fantasy or RPG mood,
anime sentimentality, funeral mood, shaker, hi-hat, percussion loop,
cheerful groove, or generic sad-piano writing.
Do not fill rests with reverb or texture.
```

同じプロンプトで三テイクを生成し、後から指示文を変えずに選別する。最初の20秒で「優雅なピアノ曲」「悲しい回想曲」「天使のコーラス」「レトロ音源の物真似」と説明できてしまう候補は不採用にする。高音が鋭く輝かず、合成された薄い声とピアノの境目を聞き分けられ、反復の中の一音の変化だけで浅い眠気が生まれる候補を残す。

## 13. 移動曲の生成ブリーフ — `umk.rail.departure`

この曲は、逃避行の決意や旅立ちの高揚を鳴らさない。横浜駅の冷気、列車到着、乗車、「家に帰りたい」という言葉までは無音とSEで保つ。「ガクンと鈍い衝撃があり、列車がゆっくりと滑り出す。」の後から曲を入れ、ホームが後ろへ流れ始めた事実をギターとドラムで受け取る。

```yaml
music_brief:
  cue_id: umk.rail.departure
  narrative_function: road-continuity
  scene_before: cold station, train arrival, boarding, departure impact
  scene_after: first westbound local train, stolen money, sparse conversation, morning window

  instrument_presence: played-ensemble
  foreground_instrument: clean electric guitar
  supporting_instruments:
    - dry human drum kit
    - rounded electric bass
    - restrained acoustic or lightly sampled piano
    - muted viola
  player_roles:
    foreground: guitar states the eight-bar road sentence with single notes and short dyads
    rhythm: drums establish a slightly brisk physical step without imitating train wheels
    low_end: bass carries slow harmonic movement independently from the kick
    harmony: piano opens the view after the movement is already established
    response: viola appears later for selected phrase-ending answers
  articulations:
    - clean guitar pick attack with short sustain and audible rests
    - dry kick, soft snare, low-mixed hi-hat or ride with phrase-level variation
    - rounded bass notes that do not walk or become funky
    - sparse piano voicings with longer values than the guitar
    - muted viola entries with clear bow release
  forbidden_instruments:
    - acoustic strumming guitar
    - shaker
    - tambourine
    - crash-cymbal lift
    - brass
    - orchestral percussion
    - ambient pad
  synth_role: none
  orchestration_route:
    opening_players: clean electric guitar and dry drum kit
    first_entry: rounded electric bass after eight bars
    second_entry: piano after sixteen bars
    color_entry: muted viola after thirty-two bars
    reduction_players: guitar and piano, or guitar and bass
    return_players: guitar, drums, bass, and piano; viola only at selected endings

  felt_tempo_bpm: 104
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 165

  motif:
    character: singable
    first_statement_seconds: 18
    return_policy: varied

  density_curve:
    opening: guitar and drums establish motion
    expansion: bass, piano, then viola enter one at a time
    hinge: reduce to two players around sixty percent
    aftermath: rebuild the opening sentence without a louder final chorus

  runtime:
    entry_after_event: begin after "ガクンと鈍い衝撃があり、列車がゆっくりと滑り出す。"
    first_exit_before_event: fade before the grey pause after "俺にできることなんて何もない"
    optional_reentry_after_event: "静岡で乗り換え。浜松で乗り換え。"
    loop: full-form
    fade_in_ms: 500
    fade_out_ms: 600
    required_silence_after_ms: 680

  ending:
    cadence: open
    final_tail_seconds: 2
```

生成時は、作品名や参照曲名を外して次をそのまま使う。

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, rhythm, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

The scene begins on an early autumn morning inside a westbound local train.
Two teenagers have already boarded and the train has just started moving.
The cold platform is sliding backward outside the window.
They have little money, no secure destination, and no heroic reason for leaving.
There is a small release in finally moving, mixed with fatigue, awkward silence,
ordinary teenage conversation, and fear that has not yet found words.

The cue may be slightly brisk, but it must not become cheerful,
triumphant, adventurous, or emotionally uplifting.
Movement is a physical fact, not a celebration.

Tempo: 104 BPM.
Meter: 4/4.
Use clear eight-bar sentences and a total duration of approximately 2 minutes 45 seconds.

Open with only two players for the first eight bars:
a clean electric guitar and a dry human drum kit.

The clean electric guitar is the foreground instrument.
Give it an original short motif using single picked notes, brief dyads,
small intervals, short sustain, and clearly audible rests.
Do not use continuous strumming, bright acoustic chords, blues phrasing,
rock power chords, or virtuoso guitar gestures.

The drum kit establishes a restrained, slightly brisk walking motion.
Use a dry kick, soft snare, and a low-mixed closed hi-hat or ride.
The performance must change subtly across phrases through rests,
ghosted weak beats, and small velocity differences.
Do not imitate train-wheel rhythm and do not repeat an unchanged stock drum loop.

After eight bars, introduce a rounded electric bass.
The bass carries slow harmonic movement and must remain independent from the kick.
No walking bass, funk line, or pop-rock drive.

After sixteen bars, introduce piano.
The piano must not double the guitar motif.
Use sparse open voicings, longer harmonic punctuation,
or a short restrained counter-line.
Its entrance should feel like the view outside the window becoming wider,
not like the arrangement becoming grander.

After thirty-two bars, introduce one muted viola as the only color instrument.
Use it only for selected phrase-ending answers or one brief secondary line.
Do not turn it into a string pad and do not keep it active continuously.

Keep the ensemble to five identifiable players:
guitar, drums, bass, piano, and muted viola.
Every player must have a separate musical job.
Bring instruments in one at a time at large phrase boundaries.

Around 55 to 65 percent of the full form,
reduce the arrangement clearly to two players:
guitar and piano, or guitar and bass.
Leave enough space to notice the distance already travelled.
Then rebuild the opening eight-bar material with guitar, drums, bass, and piano.
Do not create a louder final chorus.
Allow the viola to return only for one or two phrase endings.

End with an open cadence and approximately two seconds of air.
The full form should be able to restart without sounding like a victory lap.

Leave room for softly spoken Japanese dialogue, train ambience, and station sounds.
Keep the drums below the dialogue, the guitar attack controlled,
the stereo width moderate, and the reverb restrained.

No vocals. No cheerful road-trip mood. No RPG field theme.
No anime opening, pop-rock chorus, post-rock crescendo, heroic travel montage,
country or folk strumming, campfire guitar, funk bass, jazz fusion,
four-on-the-floor kick, shaker, tambourine, busy hi-hat, flashy drum fill,
crash-cymbal lift, cinematic strings, brass, orchestral percussion,
ambient wash, lo-fi beat, vinyl noise, or triumphant cadence.
```

同じプロンプトで三テイクを生成する。冒頭八小節でギターとドラムの二人を聞き分けられない候補、ピアノが最初から鳴る候補、最初の20秒で「楽しい旅行が始まる」と感じる候補は不採用にする。軽快さが行き先の希望ではなく、身体がすでに移動している事実として聞こえる候補を残す。

## 14. 雨の部屋の生成ブリーフ — `umk.rain.room`

この曲の主場面はDAY 5とする。雨で目が覚めた朝、ミオの熱、もう一泊する判断までは音楽を置かない。ここで曲を入れると、発熱や雨に「悲しい」という答えを先回りして与えてしまうためである。

最初の入口は「冷えた缶を口に当てると、雨の音が少し遠くなった。」の後とする。缶飲料、本、携帯ゲーム、持ち帰った食事、MDへの録音が、安いホテルの一室を一日だけの生活へ変えていく。曲が描くのは恋愛でも看病でもなく、移動を止めたことで偶然共有された生活の手触りである。

```yaml
music_brief:
  cue_id: umk.rain.room
  narrative_function: ordinary-memory-shadow
  primary_scene: day-5 hotel room in continuous rain
  scene_before: waking, mild fever, dim room, decision to stay one more night
  scene_after: canned drinks, worn book, handheld game, takeaway food, portable recorder

  instrument_presence: intimate-solo
  foreground_instrument: slightly worn soft electric piano
  supporting_instruments:
    - B-flat clarinet in the low chalumeau register
  player_roles:
    foreground: electric piano states an incomplete five-note domestic sentence
    response: clarinet answers only selected large phrase endings
  articulations:
    - soft electric-piano attack without felt-piano hammer noise
    - low-mid register notes with audible key-to-key separation
    - plain low clarinet tone with restrained breath and a clean release
    - written rests that remain empty
  forbidden_instruments:
    - acoustic grand piano
    - felt piano
    - guitar
    - drums
    - percussion
    - solo cello
    - string section
    - vibraphone
    - music box
    - ambient pad
  synth_role: electric-piano timbre only; no atmospheric layer
  orchestration_route:
    opening_players: electric piano alone
    first_entry: low clarinet after sixteen bars
    reduction_players: electric piano right hand only
    return_players: electric piano and low clarinet, never louder than the opening

  felt_tempo_bpm: 72
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 170

  motif:
    character: incomplete-five-note-sentence
    first_statement_seconds: 14
    return_policy: reorder one inner note and omit one expected downbeat

  density_curve:
    opening: one close electric piano with long literal rests
    expansion: clarinet gives one short answer after two piano sentences
    hinge: remove the clarinet and most left-hand notes around fifty-eight percent
    aftermath: restore two players with fewer notes and no dynamic climax

  runtime:
    entry_after_event: begin after "冷えた缶を口に当てると、雨の音が少し遠くなった。"
    first_exit_before_event: fade before "ホテルの扉を開ける。"
    optional_reentry_after_event: "カップの湯気が、二人の間で細く揺れた。"
    final_exit_before_event: fade before the hospital-memory blackout
    loop: full-form
    fade_in_ms: 800
    fade_out_ms: 700
    required_silence_after_ms: 900

  ending:
    cadence: open-and-incomplete
    final_tail_seconds: 2.5
```

生成時は、作品名や参照曲名を外して次をそのまま使う。

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, timing, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

The scene takes place through one rainy day in a small inexpensive hotel room.
Two teenagers have stopped travelling because one of them has a mild fever.
The room is dim and humid. They share canned drinks, a worn paperback,
a handheld game, takeaway food, and the sound captured by a portable recorder.
Neither person calls this care, romance, comfort, or home.
For a few hours, ordinary objects quietly make the stopped room feel lived in.

This is not music about rain, illness, tragedy, or tender romance.
It is music about two people accidentally sharing a small piece of daily life.
The emotional temperature is tired, close, plain, and faintly warm.
Do not tell the listener to feel sad.

Tempo: 72 BPM.
Meter: 4/4.
Use clear eight-bar sentences and a total duration of approximately 2 minutes 50 seconds.

Use only two identifiable players:
a slightly worn soft electric piano in the foreground,
and one B-flat clarinet used only in its low chalumeau register.

Open with electric piano alone for sixteen bars.
Write an original incomplete five-note sentence in the low-middle register.
Give the notes soft but distinct attacks, modest sustain, and literal empty rests.
The motif should feel easy to remember but should stop before sounding resolved.
Do not use an acoustic grand, felt-piano hammer noise, decorative arpeggios,
romantic rubato, jazz voicings, or a conventional sad-piano progression.

After sixteen bars, let the low clarinet answer one selected phrase ending.
Its line should be short, plain, and lower than a lyrical solo.
Use restrained breath, no vibrato display, and a clean release into silence.
The clarinet must not double the piano melody, remain continuously active,
or turn the scene whimsical, pastoral, elegant, or sentimental.

Develop the piece by reordering one inner note of the five-note sentence
and occasionally omitting an expected downbeat.
Do not add new layers merely to create growth.
Around 58 percent of the full form, remove the clarinet
and reduce the piano to its right hand with longer rests.
Make this a clearly audible absence, not a reverb-filled ambient break.
Then restore the two players quietly.
The return must contain fewer notes and must not become a final chorus.

Keep the harmony mostly plain and close.
Avoid strong functional cadences, sweeping minor-key drama,
and any modulation designed as an emotional revelation.
End open and incomplete, followed by approximately 2.5 seconds of clean air,
so the full form can restart without announcing a loop.

Leave space for quiet Japanese dialogue and separately mixed room ambience.
The music must work completely dry, with no rain recording embedded in it.
Use a close perspective, restrained reverb, moderate-to-narrow stereo width,
controlled low mids, and no sparkling high-frequency layer.

No vocals. No rain samples. No droplets imitated by notes.
No thunder, air-conditioner hum, vinyl noise, tape hiss, field recording,
ambient drone, pad, lo-fi beat, café jazz, bossa nova, waltz, lullaby,
guitar, drums, percussion, solo cello, string pad, vibraphone, music box,
anime romance, sentimental illness scene, cinematic swell,
Ghibli-like whimsy, RPG inn music, or tragic cadence.
```

同じプロンプトで三テイクを生成する。雨音や残響が情緒の大半を担う候補、ピアノだけで「悲しい病人の場面」と分かってしまう候補、クラリネットが旋律の主役になる候補は不採用にする。ピアノの休符が本当に空白として残り、クラリネットが入った瞬間だけ「部屋にもう一人いる」と聞こえる候補を残す。

## 15. 最初の録音の生成ブリーフ — `umk.recording.trace`

この曲はDAY 4の二度の録音を一括して説明しない。主場面は、松江のコインランドリーでミオが初めてMDウォーカーを取り出す場面に限定する。回る二人分の服へ日付と場所を吹き込み、「残しておけば、あとで聞ける」と言った時点で、何気ない現在は再生可能な過去へ変わり始める。

曲はその意味を悲劇として告知しない。前半ではギターとピアノが不器用に同じ時間を共有し、白いラベルが見えた後、一度だけ来るはずのピアノの返答を欠かす。完全な無音の蝶番を通過した後はギターだけが残る。これは登場人物を楽器へ割り当てる表現ではなく、「記録の前」と「記録の後」が同じ形へ戻れないことを、編成の事実にするための構造である。

```yaml
music_brief:
  cue_id: umk.recording.trace
  narrative_function: irreversible-question-short
  primary_scene: day-4 first MD recording in an empty coin laundry
  scene_before: two sets of clothes turning together in a washing machine
  scene_after: spoken date and place, blank labels, recording stopped, almost no conversation

  instrument_presence: ensemble-to-solo
  foreground_instrument: clean electric guitar played with fingers
  supporting_instruments:
    - close dry upright piano
  player_roles:
    foreground: guitar states a four-bar sentence made from single notes and one dyad
    response: piano answers for two bars, then leaves two bars empty
    low_end: piano left hand supplies one low note at selected pre-hinge boundaries
  articulations:
    - warm neck-pickup guitar with finger attack and short natural decay
    - dry piano notes with no felt noise and no sustaining-pedal wash
    - clearly released phrase endings
    - one expected piano response deliberately omitted before the hinge
  forbidden_instruments:
    - acoustic guitar
    - drums
    - percussion
    - bass
    - strings
    - cello
    - woodwind
    - music box
    - ambient pad
  synth_role: none
  orchestration_route:
    opening_players: guitar and piano
    greatest_density: the same two players with one additional low piano note
    omitted_event: remove one expected two-bar piano answer
    hinge: 1.8 seconds of complete silence
    aftermath_players: guitar alone

  felt_tempo_bpm: 70
  subdivision_grid_bpm: 140
  meter: 4/4
  local_phrase_length_bars: 8
  full_form_seconds: 86

  motif:
    character: plain-four-bar-question
    first_statement_seconds: 12
    return_policy: keep the opening interval but remove the final note after the hinge

  density_curve:
    opening: two close players exchanging short phrases
    expansion: piano left hand appears at only two large boundaries
    peak: occur before halfway without an increase in loudness
    omission: an expected piano answer does not arrive
    hinge: complete silence around fifty-six percent
    aftermath: guitar alone with fewer attacks and longer gaps

  runtime:
    entry_after_event: begin after the washing drum starts and Mio takes out the MD walker
    no_sync_event: do not synchronize a climax to "残しておけば、あとで聞けるでしょ"
    hinge_region: blank labels and stopping the recording
    exit_before_event: finish before the washing drum stops
    loop: none
    fade_in_ms: 450
    fade_out_ms: 0
    required_silence_after_ms: 4500

  ending:
    cadence: unresolved
    final_tail_seconds: 4.5
```

生成時は、作品名や参照曲名を外して次をそのまま使う。

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, timing, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

The scene takes place in an empty coin laundry on a cold early-autumn morning.
Two teenagers have put both sets of clothes into the same washing machine.
One of them takes out a portable MiniDisc recorder,
connects a small microphone, and calmly records the date, the city,
and the ordinary fact that their clothes are turning together.
She says they will be able to listen to it later.
Several blank disc labels remain beside them.
After the recording stops, they say almost nothing.

This is not nostalgic recording music and not a tragic memory cue.
Do not imitate a washing machine, a MiniDisc mechanism, or a human voice.
The music should make the present feel ordinary at first,
then make the time before and after one missing reply feel subtly unequal.

Duration: approximately 1 minute 26 seconds.
Felt tempo: 70 BPM, with a quiet 140 BPM subdivision grid.
Meter: 4/4.
Use local eight-bar continuity, but do not repeat the complete form.
This is a non-looping cue.

Use exactly two identifiable players before the central hinge:
a clean electric guitar played gently with fingers,
and one close, dry upright piano.

The clean electric guitar is the foreground instrument.
Use a warm neck-pickup tone, controlled finger attack, short natural decay,
single notes, and no more than one brief dyad in each four-bar statement.
Write an original plain four-bar question with audible rests.
It must not sound bluesy, virtuosic, romantic, or like acoustic fingerstyle.

The dry upright piano answers for approximately two bars,
then leaves the remaining space empty.
Use isolated low-middle notes and restrained two-note voicings.
At only two large pre-hinge boundaries,
the piano left hand may add one separate low note.
Do not use decorative arpeggios, felt-piano noise, sustain-pedal wash,
jazz harmony, or a conventional sad-piano progression.

Reach the greatest density before the halfway point,
using only the same guitar and piano and without becoming louder.
Before the hinge, establish the expectation of one more two-bar piano answer.
Then omit that answer completely.

At approximately 56 percent of the full form,
create 1.8 seconds of absolute musical silence.
No reverb tail, drone, pad, noise, or held resonance may cross this hinge.

After the hinge, return with the clean electric guitar alone.
Do not bring the piano back.
Keep the opening interval of the guitar question,
but remove its expected final note.
Use fewer attacks and increasingly long literal gaps.
Do not rebuild the arrangement and do not create a climax.

End unresolved and leave at least 4.5 seconds of clean silence
inside the delivered file.
The ending should feel like the performers stopped before the machine did,
not like a dramatic death, farewell, or revelation.

Keep both instruments physically close and clearly separable.
Use narrow-to-moderate stereo width, almost no room reverb,
controlled upper mids, and enough space for softly spoken Japanese dialogue
and separately mixed washing-machine ambience.

No vocals. No spoken samples. No MiniDisc beep. No tape hiss.
No washing-machine rhythm, mechanical pulse, field recording, vinyl noise,
acoustic guitar, guitar strumming, drums, percussion, bass, strings, cello,
woodwind, music box, bell, ambient pad, lo-fi beat, jazz, bossa nova,
romantic duet, sentimental flashback, memorial montage, cinematic swell,
anime tragedy, post-rock build, RPG music, or final resolved chord.
```

同じプロンプトで三テイクを生成する。前半だけで「悲しい回想」と分かる候補、ギターがアコースティックに聞こえる候補、ピアノが伴奏コードを埋め続ける候補は不採用にする。1.8秒の蝶番で残響まで本当に消え、その後のギター一人が「寂しい独奏」ではなく、返事を待ったまま残った句として聞こえる候補を選ぶ。

## 16. 晴れ間の移動の生成ブリーフ — `umk.clear-between`

この曲は、DAY 6の晴天や「また、旅を続けようぜ」という台詞へ直接入れない。旅を続けると決めても、財布の札は薄く、行き先はまだ決まっていない。ホテルの鍵を返し、二度と来られないかもしれない場所へ社交辞令を残し、ロビーのドアを開けた後、身体が再び駅へ向かい始めた事実から鳴らす。

最初の移動曲 `umk.rail.departure` と同じ速さや編成を複製しない。今回は雨で一日止まった後なので、ピアノをベースより先に入れる。先に視界が開き、その後から低音が歩幅へ追いつく。明るさは長調や大音量ではなく、参加順と休符の短さが少し変わったことで表す。

```yaml
music_brief:
  cue_id: umk.clear-between
  narrative_function: road-continuity-light
  primary_scene: day-6 leaving the hotel and returning to westbound travel
  scene_before: decision to continue, packing, thin wallet, returning the room key
  scene_after: hot street, route map, arbitrary distant ticket, empty platform, local train departure

  instrument_presence: played-ensemble
  foreground_instrument: clean electric guitar
  supporting_instruments:
    - light dry human drum kit
    - restrained close piano
    - rounded electric bass
    - muted viola
  player_roles:
    foreground: guitar states a new eight-bar walking sentence with single notes and brief dyads
    rhythm: drums give the body a light forward step without sounding cheerful
    opening_horizon: piano enters before the bass and opens the register
    low_end: bass later settles the harmonic ground without following the kick
    response: viola gives one or two late phrase-ending replies
  articulations:
    - clean guitar pick attack with moderate sustain and frequent rests
    - dry kick, side-stick or soft snare, and low-mixed ride or closed hi-hat
    - sparse piano notes longer than the guitar phrase
    - rounded bass with restrained finger attack
    - muted viola with short written entries and audible bow release
  forbidden_instruments:
    - acoustic strumming guitar
    - shaker
    - tambourine
    - bright crash cymbal
    - brass
    - orchestral percussion
    - ambient pad
  synth_role: none
  orchestration_route:
    opening_players: clean electric guitar and light dry drum kit
    first_entry: piano after eight bars
    second_entry: rounded electric bass after sixteen bars
    color_entry: muted viola after thirty-two bars
    reduction_players: guitar and piano around fifty-nine percent
    return_players: guitar, drums, piano, and bass; viola only for one final answer

  felt_tempo_bpm: 102
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 168

  motif:
    character: lightly-forward-eight-bar-sentence
    first_statement_seconds: 19
    return_policy: preserve the pickup but shorten one held note

  density_curve:
    opening: two players establish motion with more air than drive
    expansion: piano enters before bass, then viola appears only after the form is stable
    hinge: reduce to guitar and piano around fifty-nine percent
    aftermath: restore the rhythm section without a louder final chorus

  runtime:
    entry_after_event: begin after "俺は社交辞令を呟き、ロビーのドアを開ける。"
    continue_through: hot street, route choice, platform, boarding
    exit_after_event: fade after "窓の外の寂れた街がゆっくりと後方へと流れていった。"
    do_not_continue_into: arrival in Onomichi at dusk
    loop: full-form
    fade_in_ms: 600
    fade_out_ms: 700
    required_silence_after_ms: 700

  ending:
    cadence: open
    final_tail_seconds: 2
```

生成時は、作品名や参照曲名を外して次をそのまま使う。

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, rhythm, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

The scene begins after two teenagers leave a dim inexpensive hotel
following a full day of rain.
They have decided to continue travelling west,
but they still have no real destination and their money is getting thinner.
Outside, the sky is suddenly clear and the day is unseasonably hot.
They walk to the station, study an unfamiliar route map,
buy a ticket toward an arbitrarily distant place,
wait on an almost empty platform, and board an old local train.

This is a return to physical movement, not a hopeful new beginning.
The cue may feel a little lighter and more awake than the earlier journey,
but it must not become cheerful, triumphant, carefree, or optimistic.
Clear weather is a change in visibility, not an answer to their lives.

Tempo: 102 BPM.
Meter: 4/4.
Use original eight-bar sentences.
Total duration: approximately 2 minutes 48 seconds.
The complete form must be able to loop without announcing its restart.

Open with two clearly identifiable players:
a clean electric guitar in the foreground
and a light, dry human drum kit.

Give the guitar an original eight-bar walking sentence
made from single picked notes, brief dyads, moderate sustain,
and frequent audible rests.
The phrase may lean slightly forward,
but must not use bright chord strumming, rock riffs,
blues vocabulary, virtuoso gestures, or acoustic folk phrasing.

The drums should create a restrained physical step.
Use dry kick, side-stick or soft snare,
and a low-mixed ride or closed hi-hat.
Vary weak beats, rests, and velocity across phrases.
Do not imitate train wheels and do not use an unchanged stock loop.

After eight bars, introduce a restrained close piano before adding bass.
The piano must not double the guitar melody.
Use sparse notes, open two-note voicings, or one short counter-line
with longer values than the guitar.
Its entrance should feel like the visible distance has widened,
not like the song has turned major, grand, or sentimental.

After sixteen bars, introduce a rounded fingered electric bass.
Let it settle the harmonic ground independently from the kick.
No walking bass, funk motion, or pop-rock drive.

After thirty-two bars, introduce one muted viola.
Use it for only one or two selected phrase-ending replies.
It must remain an individual player, never a string pad,
continuous counter-melody, or emotional swell.

Around 59 percent of the full form,
reduce the arrangement clearly to guitar and piano.
Leave the reduced group exposed long enough to hear both players.
Then restore drums and bass without becoming louder.
The viola may return for one final short answer only.
Do not create a final chorus or arrival climax.

End with an open cadence and approximately two seconds of air.
The loop point should suggest that the same road continues,
not that a destination has been reached.

Leave room for quiet Japanese dialogue, footsteps, cicadas,
station ambience, diesel engine, and train-door sounds.
Keep the drums below speech, control the guitar attack,
use moderate stereo width, and keep reverb restrained.

No vocals. No cheerful summer song. No road-trip celebration.
No anime opening, youth montage, slice-of-life comedy theme,
RPG field music, victory theme, pop-rock chorus, post-rock crescendo,
country or folk strumming, funk bass, jazz fusion,
four-on-the-floor kick, shaker, tambourine, busy hi-hat,
flashy drum fill, crash-cymbal lift, cinematic strings,
brass, orchestral percussion, ambient wash, lo-fi beat,
or triumphant resolved cadence.
```

同じプロンプトで三テイクを生成する。冒頭が夏の青春アニメ、楽しい旅行、RPGのフィールドに聞こえる候補は不採用にする。ピアノが八小節後に入った瞬間、コードが明るくなったのではなく「先が見えるようになった」と感じられ、ベースが後から身体の歩幅を落ち着かせる候補を残す。

## 17. 島に残る距離の生成ブリーフ — `umk.island.distance`

この曲の主場面はDAY 7、直島で二人乗りの自転車が走り出してから、一度止まり、遠ざかるフェリーを眺め、「次の海まで連れてってよ」と言われるまでとする。海、霧、坂道は美しい。しかし、カズキが意識しているのは景色よりも、背中に回った腕の冷たさと軽さである。

前半は移動曲として身体を進めるが、観光や解放の曲にはしない。ミオの腕から力が抜けた後、ドラムを永久に退場させる。後半はテンポを変えず、発音の間隔だけを広げることで、同じ島にいる二人の距離が開いたように聞かせる。チェロは悲劇を予告せず、ドラムが担っていた低い身体性の一部だけを引き受ける。

```yaml
music_brief:
  cue_id: umk.island.distance
  narrative_function: road-continuity-to-ordinary-distance
  primary_scene: day-7 bicycle ride and first roadside stop on Naoshima
  scene_before: ferry arrival, brief excitement, renting one bicycle for two
  scene_after: weakening grip, distant ferry, promise of the next sea, slower departure

  instrument_presence: played-ensemble
  foreground_instrument: nylon-string guitar played as single notes
  supporting_instruments:
    - light dry human drum kit
    - restrained close piano
    - one low cello
  player_roles:
    foreground: nylon guitar states an eight-bar road sentence with held notes and physical rests
    rhythm: drums provide forward balance without imitating pedals or wheels
    harmony: piano opens a second, slower sentence after movement is established
    aftermath_low_end: cello replaces only part of the missing bodily weight after the drums leave
  articulations:
    - warm nylon pluck with short sustain and no continuous fingerpicking
    - dry kick, soft snare, and very low closed hi-hat with phrase-level variation
    - sparse piano notes longer than both guitar and drum gestures
    - low cello entries with little vibrato and clearly released bows
  forbidden_instruments:
    - steel-string acoustic guitar
    - ukulele
    - hand percussion
    - shaker
    - tambourine
    - electric bass
    - string section
    - flute
    - tropical percussion
    - ambient pad
  synth_role: none
  orchestration_route:
    opening_players: nylon-string guitar and light dry drum kit
    first_entry: restrained piano after eight bars
    greatest_density: guitar, drums, and piano only
    permanent_exit: remove all drums around fifty-two percent
    empty_sentence: guitar and piano alone for eight bars
    aftermath_entry: low cello after the empty sentence
    ending_players: nylon guitar, piano, and cello with increasing rests

  felt_tempo_bpm: 96
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 190

  motif:
    character: held-road-sentence
    first_statement_seconds: 20
    return_policy: preserve the opening pickup but double the final rest after the drums leave

  density_curve:
    opening: two players move without celebration
    expansion: piano enters after the first complete road sentence
    hinge: drums stop permanently around fifty-two percent
    suspension: one full eight-bar sentence without a replacement low instrument
    aftermath: cello enters sparsely and all players increase their rests

  runtime:
    entry_after_event: begin after "俺はペダルを踏み込み、潮風を切り裂いて走り出す。"
    narrative_hinge_region: weakening grip and roadside stop
    exit_after_event: fade no later than "俺はゆっくりとペダルを漕ぐ。"
    day_8_reprise_source: post-hinge drumless section only
    loop: none
    fade_in_ms: 650
    fade_out_ms: 800
    required_silence_after_ms: 1100

  ending:
    cadence: open
    final_tail_seconds: 4
```

生成時は、作品名、島名、参照曲名を外して次をそのまま使う。

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, rhythm, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

The scene follows two teenagers riding one bicycle along a small coastal island.
One pedals while the other sits behind and holds his waist.
The road descends through pale mist toward a calm inland sea.
At first there is physical motion and a brief sense of openness.
Then the passenger's grip gradually weakens.
They stop beside the road and watch their ferry become very small in the distance.
The sea is close, but the person standing nearby somehow appears farther away.
Before leaving, she asks to be taken to the next sea.

This is not island tourism, summer freedom, romance, or an illness tragedy.
The music must begin as restrained shared movement
and gradually become a quieter measurement of distance.
Do not make the landscape promise happiness.

Tempo: 96 BPM.
Meter: 4/4.
Use original eight-bar sentences.
Total duration: approximately 3 minutes 10 seconds.
This is a non-looping cue with a one-directional orchestration change.

Use four clearly identifiable players at most:
nylon-string guitar, light dry human drum kit,
restrained close piano, and one low cello.

Open with nylon-string guitar and the light dry drum kit.
The nylon guitar is the foreground instrument.
Write an original eight-bar road sentence from isolated single notes,
one or two brief dyads, held notes, and clearly audible rests.
Use warm physical plucks and short natural sustain.
Do not use continuous fingerpicking, chord strumming,
flamenco gestures, folk patterns, or virtuoso playing.

The drums provide restrained forward balance.
Use a dry kick, soft snare, and a very low closed hi-hat.
Vary weak beats, velocity, and rests across phrases.
Do not imitate bicycle pedals, wheels, ferry engines, or waves.
Do not use hand percussion or a tropical groove.

After eight bars, introduce a restrained close piano.
The piano must not double the guitar.
Give it a slower second sentence using sparse notes,
open two-note voicings, and longer values than the guitar.
The greatest density must contain only guitar, drums, and piano.
Do not add bass or strings beneath them.

At approximately 52 percent of the complete form,
remove the entire drum kit permanently.
Do not mark this change with a fill, crash, slowdown, or dramatic silence.
The next full eight-bar sentence must contain only guitar and piano.
Leave the missing rhythmic weight clearly audible.

After that empty sentence, introduce one low cello.
The cello may answer selected phrase endings
and replace only part of the bodily weight previously carried by the drums.
Use little vibrato, restrained dynamics, short written entries,
and clearly released bow endings.
It must not become a lyrical solo, continuous bass line,
string pad, or signal of approaching death.

Keep the tempo unchanged after the drums leave.
Create stillness by increasing the literal rests
and lengthening the space after the guitar motif.
Preserve its opening pickup, but double its final rest.
Do not rebuild the drums and do not create a final chorus.

End on an open cadence with approximately four seconds of clean air.
The ending should feel as though the road continues outside the music,
not as though the island has been reached or left behind.

Leave room for quiet Japanese dialogue, bicycle sounds, wind,
distant ferry horn, breathing, and coastal ambience.
Use controlled guitar attack, narrow-to-moderate stereo width,
restrained reverb, and no atmospheric layer.

No vocals. No tropical or Mediterranean mood. No holiday music.
No romantic cycling montage, anime summer theme, coming-of-age triumph,
RPG island theme, pastoral fantasy, Ghibli-like whimsy,
steel-string acoustic guitar, ukulele, continuous fingerpicking,
hand percussion, shaker, tambourine, bossa nova, reggae,
surf music, pop-rock chorus, post-rock crescendo,
electric bass, string section, flute, ambient pad,
ocean recording, wind recording, cinematic swell,
sentimental illness music, funeral cello, or resolved final chord.
```

同じプロンプトで三テイクを生成する。南国、観光、恋愛、自転車の爽快感に聞こえる候補は不採用にする。ドラムが消えた後もテンポが落ちず、単に寂しくなるのではなく、ペダルの身体性だけが失われて二人の距離を意識させる候補を残す。チェロが入った瞬間に死を連想させる候補も落とす。

## 18. 北へ狭まる線路の生成ブリーフ — `umk.north.grey`

この曲の主場面はDAY 9、山陰へ向かう二両編成のディーゼル車内とする。二人は行き先を知らず、一番安い切符で北へ進み、残ったパンを半分ずつ食べる。トンネルを抜けると空と海は鉛色になるが、曲は「寒い景色」を短調、ノイズ、低音の持続で説明しない。

変化させるのは選択肢の数である。ギターの八小節句は同じ線路を保ちながら、帰還するたび最高音を一つ欠く。ドラムもテンポを落とさず、約60%から高域の刻みだけを永久に失う。低いピアノとチェロは悲劇を足すのではなく、失われた音域の下で移動を継続させる。

```yaml
music_brief:
  cue_id: umk.north.grey
  narrative_function: road-continuity-narrowing-into-irreversibility
  primary_scene: day-9 northbound diesel local train
  scene_before: repeated transfers, station sleep, one remaining piece of bread
  scene_after: unknown destination, weakening body, grey sea, shared earphone

  instrument_presence: played-ensemble
  foreground_instrument: restrained clean electric guitar
  supporting_instruments:
    - low dry human drum kit
    - close upright piano in the low-middle register
    - one restrained low cello
  player_roles:
    foreground: guitar carries an eight-bar sentence that loses its highest note on each return
    rhythm: drums preserve the trainless walking pulse while permanently losing high articulation
    harmony: piano establishes low harmonic ground without a sad progression
    response: cello enters late for selected phrase endings only
  articulations:
    - clean guitar pick attack, short sustain, single notes, and brief dyads
    - dry kick, soft snare, initially low closed hi-hat, no cymbal wash
    - sparse low piano notes with restrained pedal
    - low cello with little vibrato and plainly released bows
  forbidden_instruments:
    - acoustic guitar
    - shaker
    - tambourine
    - ride wash
    - crash cymbal
    - electric bass
    - string section
    - brass
    - ambient pad
    - noise layer
  synth_role: none
  orchestration_route:
    opening_players: clean electric guitar and low dry drum kit
    first_entry: low piano after eight bars
    color_entry: low cello after twenty-four bars
    pitch_narrowing: remove the highest guitar note from each large return
    permanent_exit: remove the hi-hat around sixty percent
    ending_players: guitar, kick and soft snare, sparse piano, selected cello replies

  felt_tempo_bpm: 92
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 180

  motif:
    character: narrowing-eight-bar-road-sentence
    first_statement_seconds: 21
    return_policy: preserve rhythm and opening interval while removing one highest pitch each time

  density_curve:
    opening: guitar and low drums continue movement without urgency
    expansion: piano enters, then cello appears after the route is established
    peak: occur before fifty-five percent without a dynamic swell
    narrowing: lose high guitar pitches and all hi-hat after sixty percent
    aftermath: retain pulse but leave increasingly fewer possible notes

  runtime:
    entry_after_event: begin after "俺は受け取って、黙って食った。"
    continue_through: destination question, coat, tunnel, first view of the Sea of Japan
    exit_after_event: fade after Kazuki places the shared earphone in his right ear
    do_not_continue_into: recorded waves, rain, collapse, or "頑張ったんだよ"
    day_10_reprise_source: post-sixty-percent low-register section only
    loop: none
    fade_in_ms: 650
    fade_out_ms: 900
    required_silence_after_ms: 1200

  ending:
    cadence: unresolved
    final_tail_seconds: 3.5
```

生成時は、作品名、地名、参照曲名を外して次をそのまま使う。

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, rhythm, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

The scene takes place inside an old two-car diesel train moving north.
Two teenagers do not know the destination.
After many transfers and brief sleep on station benches,
they share the last piece of bread from their bag.
One of them speaks less than before and holds the other's coat for warmth.
The train passes through a long tunnel.
Beyond it are a low grey sky and a rough northern sea.
Later, one earbud will be shared so that an older recording of calm waves
can be heard against the real rough sea outside.

This is not music about cold weather, death, or an approaching tragedy.
It is music about movement continuing while available choices quietly decrease.
Do not make the grey landscape emotionally grand.

Tempo: 92 BPM.
Meter: 4/4.
Use original eight-bar sentences.
Total duration: approximately 3 minutes.
This is a non-looping cue with irreversible register reduction.

Use four clearly identifiable players:
restrained clean electric guitar, low dry human drum kit,
close upright piano in the low-middle register,
and one restrained low cello.

Open with clean electric guitar and the low dry drum kit.
The guitar is the foreground instrument.
Write an original eight-bar road sentence from single picked notes,
brief dyads, short sustain, and audible rests.
Keep its rhythm and opening interval recognizable on every large return.
On each return, remove one of its highest pitches.
Do not replace the missing pitch with a lower ornament.
The sentence must retain its forward shape while its usable register narrows.

The drums preserve a restrained physical pulse.
Use dry kick, soft snare, and an initially low-mixed closed hi-hat.
Vary rests and weak-beat velocity across phrases.
Do not imitate train wheels, diesel engines, or rain.
No fills may announce structural changes.

After eight bars, introduce a close upright piano in the low-middle register.
Use sparse isolated notes and restrained two-note voicings.
The piano must not double the guitar or play a conventional sad progression.
Its job is to establish harmonic ground beneath the narrowing guitar,
not to make the scene darker.

After twenty-four bars, introduce one low cello.
Use it only for selected phrase-ending replies.
Keep little vibrato, restrained dynamics, and clearly released bows.
The cello must not become a lyrical solo, continuous bass line,
string pad, or omen of death.

Reach the greatest density before 55 percent without becoming louder.
At approximately 60 percent, remove the closed hi-hat permanently.
Keep the tempo unchanged and retain only dry kick and soft snare.
Continue removing the guitar's highest available notes on later returns.
Do not replace the lost high register with a pad, cymbal, or reverberant texture.

The final section should still move forward,
but with fewer pitches, less high-frequency articulation,
and longer literal rests.
Do not rebuild the high register and do not create a final chorus.

End unresolved with approximately 3.5 seconds of clean air.
The ending should feel as though the line continues beyond sight,
not as though the train has arrived or someone has died.

Leave room for quiet Japanese dialogue, train ambience,
wind, rain, and two separately mixed wave recordings.
Use controlled guitar attack, dry drums below speech,
narrow-to-moderate stereo width, and restrained reverb.

No vocals. No cold ambient music. No dark drone or noise bed.
No funeral mood, tragic foreshadowing, sentimental illness theme,
sad-piano ballad, lyrical cello solo, cinematic swell,
train rhythm imitation, industrial pulse, snowfield theme,
RPG travel music, post-rock crescendo, pop-rock chorus,
acoustic guitar, shaker, tambourine, ride wash, crash cymbal,
electric bass, string section, brass, ambient pad,
field recording, rain recording, ocean recording,
lo-fi treatment, vinyl noise, or resolved final chord.
```

同じプロンプトで三テイクを生成する。暗いアンビエント、雪国、死の予告、悲しいピアノに聞こえる候補は不採用にする。最高音とハイハットが失われても曲が遅くなったようには聞こえず、同じ速度で進みながら選択肢だけが減ったと感じられる候補を残す。

## 19. 灯台の蝶番の生成ブリーフ — `umk.lighthouse.edge`

この曲の対象となるDAY 14は、正本には存在するが現在の公開Ariaでは非コンパイル草稿である。DAY 11–13と終盤の接続が確定するまで、曲を本編へ実装しない。以下は、本文が維持された場合にも、灯台が別の転回点へ改稿された場合にも転用できるよう、「死の場面」ではなく「登る前後が同じでなくなる場面」として設計する。

鎖を壊す金属音、身体を背負う重さ、螺旋階段はSEと文章に任せる。前半の三人は登る運動を直接模倣せず、同じ短い句を手渡しながら密度を早めに使い切る。「目に光が射す」に対応する蝶番では残響を含むすべてを1.8秒消し、後半には最高音と終止音を欠いたギターだけを置く。「旅の終わり」という台詞には音楽を残さない。

```yaml
music_brief:
  cue_id: umk.lighthouse.edge
  narrative_function: irreversible-question
  source_status: provisional-noncompiled-ending-draft
  primary_scene: breaking a lighthouse lock, carrying someone upward, opening into sunset
  scene_before: rusted latch, repeated stone strikes, chain falling to the ground
  scene_after: spiral stairs, sudden light, horizon, pulse, dispute over whether the journey ends

  instrument_presence: ensemble-to-solo
  foreground_instrument: restrained clean electric guitar
  supporting_instruments:
    - close dry upright piano
    - one low cello
  player_roles:
    foreground: guitar carries a five-note physical sentence without depicting footsteps
    harmony: piano places separated low-middle dyads beneath selected statements
    response: cello answers only pre-hinge phrase endings
  articulations:
    - warm clean guitar pick attack with short sustain and literal rests
    - dry piano with restrained pedal and clearly separated notes
    - low cello with little vibrato and clearly released bows
    - complete release by all players before the hinge
  forbidden_instruments:
    - drums
    - percussion
    - acoustic guitar
    - electric bass
    - string section
    - brass
    - choir
    - bells
    - ambient pad
    - noise layer
  synth_role: none
  orchestration_route:
    opening_players: clean electric guitar, dry piano, and low cello
    greatest_density: all three players before fifty percent
    reduction: remove most piano left-hand notes before the hinge
    hinge: 1.8 seconds of absolute silence around sixty percent
    permanent_exits: piano and cello
    aftermath_players: clean electric guitar alone

  felt_tempo_bpm: 70
  subdivision_grid_bpm: 140
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 165

  motif:
    character: physical-five-note-question
    first_statement_seconds: 14
    return_policy: after the hinge retain the opening interval but remove the highest and final notes

  density_curve:
    opening: three close players with clear individual roles
    expansion: reach greatest density before halfway without a swell
    approach: piano loses low notes and cello responses become shorter
    hinge: total digital silence with no tail
    aftermath: guitar alone, fewer attacks, longer literal gaps

  runtime:
    entry_after_event: begin after the chain falls to the ground
    split_event: split the delivered master at the silent hinge
    ascent_file: umk.lighthouse.edge-ascent
    open_file: umk.lighthouse.edge-open
    switch_on_event: "目に光が射す。"
    final_exit_after_event: fade after "生きてるんだな" / "...うん"
    do_not_continue_into: dialogue beginning "もう終わりなんだよ"
    loop: none
    fade_in_ms: 700
    fade_out_ms: 1000
    required_silence_after_ms: 1400

  ending:
    cadence: absent
    final_tail_seconds: 5
```

生成時は、作品名、地名、参照曲名を外して次をそのまま使う。

```text
Original instrumental cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, timing, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

The scene begins outside an old coastal lighthouse at sunset.
A rusted lock has been broken and its chain has fallen to the ground.
One exhausted teenager carries another person up a narrow spiral staircase.
At the top, a small door opens into sudden amber light and a wide horizon.
They confirm that the weakened person is still alive.
Soon afterward, they will disagree about whether their journey is ending.

This is not a death scene, ascension, rescue, victory, farewell, or final ending.
The music must not decide what the lighthouse means.
It should make the time before and after the door opens feel unequal,
then leave the decisive conversation completely unscored.

Felt tempo: 70 BPM over a quiet 140 BPM subdivision grid.
Meter: 4/4.
Use original eight-bar sentences.
Total duration: approximately 2 minutes 45 seconds.
This is a non-looping cue designed to be split into two runtime files.

Use exactly three identifiable players before the central hinge:
a restrained clean electric guitar in the foreground,
one close dry upright piano,
and one low cello.

The clean electric guitar carries an original five-note physical question.
Use a warm controlled pick attack, short natural sustain,
single notes, no more than one brief dyad,
and literal rests between gestures.
Do not imitate footsteps, climbing, heartbeat, or repeated metal strikes.
Do not use rock riffs, blues phrasing, acoustic fingerstyle, or virtuoso playing.

The dry upright piano places separated low-middle dyads
beneath selected guitar statements.
Use restrained pedal and allow every note to release clearly.
It must not play decorative arpeggios,
a conventional sad progression, or a dramatic rising sequence.

The low cello answers only selected pre-hinge phrase endings.
Use little vibrato, restrained dynamics, short entries,
and clearly released bow endings.
It must not become a lyrical solo, continuous bass line,
string pad, or omen of death.

Reach the greatest density before 50 percent without becoming louder.
As the hinge approaches, remove most piano left-hand notes
and shorten the cello replies.
Do not create an orchestral climb or synchronize a climax to the door.

At approximately 60 percent of the complete form,
release every instrument fully and create exactly 1.8 seconds
of absolute digital silence.
No reverb tail, resonance, pad, wind, noise, or sustained note may cross it.
This must be a clean edit point.

After the silence, return with the clean electric guitar alone.
Never bring back the piano or cello.
Retain the opening interval of the five-note question,
but remove both its highest pitch and its expected final note.
Use fewer attacks and increasingly long literal gaps.
Do not introduce a new melody, rebuild the arrangement,
or create a final emotional statement.

End without a cadence and leave at least five seconds of clean silence
inside the delivered file.
The music should stop before the characters discuss
whether their journey is over.

Leave room for quiet Japanese dialogue, metal impacts,
wooden stair sounds, breath, wind, and waves,
all of which will be mixed separately.
Use a close dry perspective, controlled guitar attack,
narrow-to-moderate stereo width, and very restrained reverb.

No vocals. No choir. No sacred or angelic sound.
No funeral mood, elegy, sentimental death scene, heroic ascent,
arrival triumph, rescue music, farewell theme, final-episode climax,
cinematic swell, post-rock crescendo, sad-piano ballad,
lyrical cello solo, string section, brass, bells, organ,
drums, percussion, heartbeat imitation, acoustic guitar,
ambient pad, drone, wind recording, ocean recording,
metal sound effect, lo-fi treatment, or resolved final chord.
```

同じプロンプトで三テイクを生成する。葬送、昇天、救済、最終回、灯台への到達達成に聞こえる候補は不採用にする。蝶番前の三人が物理的な奏者として聞こえ、1.8秒の無音で残響まで切れ、後半のギターが泣く独奏ではなく「答えの最後を発音しなかった句」として聞こえる候補を残す。

## 20. 答えを持たない春の生成ブリーフ — `umk.spring.after`

現在の `ex.md` は、ミオの死、石碑、思い出話を明示する旧後日談であり、公開経路から外れている。この曲を旧本文へ最適化すると、将来の後日談を死別の余韻へ固定してしまう。したがって、生存、死別、旅の継続を一切プロンプトへ含めず、「ある時間が過ぎた後も世界と生活が普通に続いている」という構造だけを作る。

ギターは使用しない。前景には乾いたアップライトピアノを残すが、終曲らしい独奏や感傷的な回想にしない。中盤でバスーンが一度だけ四小節の返答を行い、その後は戻らない。木管の息を人の代理にせず、日常句に対して別の現在が一度だけ応答したという編曲上の事実にする。

```yaml
music_brief:
  cue_id: umk.spring.after
  narrative_function: ordinary-memory-afterimage
  source_status: provisional-until-epilogue-rewrite
  primary_scene: an ordinary spring day after an unspecified passage of time
  forbidden_story_assumptions:
    - death
    - survival
    - cure
    - reunion
    - separation
    - grave
    - memorial

  instrument_presence: intimate-solo
  foreground_instrument: close dry upright piano
  supporting_instruments:
    - one bassoon in the restrained tenor register
  player_roles:
    foreground: piano states a six-note everyday sentence that never returns complete
    response: bassoon gives one four-bar answer near the middle and never returns
  articulations:
    - plain piano attack with short natural room decay
    - restrained pedal and separated low-middle two-note voicings
    - bassoon tenor tone without comic staccato or lyrical vibrato
    - written rests that remain completely empty
  forbidden_instruments:
    - guitar
    - drums
    - percussion
    - strings
    - cello
    - flute
    - bells
    - celesta
    - music box
    - choir
    - ambient pad
  synth_role: none
  orchestration_route:
    opening_players: upright piano alone
    first_response: bassoon enters once around forty-two percent
    permanent_exit: bassoon leaves after one four-bar answer
    ending_players: upright piano alone with one missing inner note and no final note

  felt_tempo_bpm: 68
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 132

  motif:
    character: plain-six-note-everyday-sentence
    first_statement_seconds: 16
    return_policy: each return preserves the opening three notes but omits a different inner note

  density_curve:
    opening: one close piano states the complete sentence once
    continuation: left hand adds only two low boundary notes
    response: bassoon answers once without doubling the motif
    aftermath: piano returns with fewer notes and longer gaps
    ending: stop before both harmonic and melodic completion

  runtime:
    entry_rule: begin only after one present-day concrete object has been observed
    forbidden_entry_events: season name, memory statement, character name, revelation
    final_exit_rule: stop before the final proposition or final sentence
    loop: none
    fade_in_ms: 850
    fade_out_ms: 1200
    required_silence_after_ms: 1400

  ending:
    cadence: absent
    final_tail_seconds: 5
```

生成時は、作品名、登場人物名、参照曲名、結末の生死を外して次をそのまま使う。

```text
Original instrumental epilogue cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, timing, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

The scene takes place on an ordinary spring day
after an unspecified amount of time has passed.
The world has continued without becoming kinder, crueler, or more meaningful.
A small present-day object, taste, sound, or movement
causes an earlier period of life to become briefly perceptible again.
The past does not return as a flashback and no final answer is given.

Do not assume whether anyone died, survived, recovered,
returned, separated, or remained together.
This is not grief music, reunion music, or a song of acceptance.
It is the sound of ordinary life continuing
while one familiar sentence can no longer be remembered completely.

Tempo: 68 BPM.
Meter: 4/4.
Use original eight-bar sentences.
Total duration: approximately 2 minutes 12 seconds.
This is a non-looping cue.

Use exactly two identifiable players:
one close dry upright piano in the foreground
and one bassoon used only in its restrained tenor register.
Do not use guitar.

The upright piano states an original plain six-note everyday sentence.
Use a natural, unpolished attack, short room decay,
restrained pedal, separated low-middle notes,
and occasional two-note voicings.
The first statement may be complete once.
On every later return, preserve the opening three notes
but omit a different inner note.
Never replace the missing note with ornament.
Do not use decorative arpeggios, romantic rubato,
a conventional sad progression, or a final-theme melody.

The piano left hand may add only two low boundary notes
before the middle of the piece.
It must not create a repeating ostinato,
walking bass, hymn progression, or cinematic foundation.

At approximately 42 percent, introduce one bassoon
for a single four-bar answer.
Use its restrained tenor register, plain sustained articulation,
minimal vibrato, and a clean release.
The bassoon must not double the piano motif,
sound comic, pastoral, whimsical, or elegiac.
After this one answer, remove the bassoon permanently.

Return to piano alone.
Use fewer attacks and increasingly long literal rests.
On the final return, omit one inner note
and also omit the expected final note.
Do not isolate one high note as a symbol of memory.
Do not create a final chorus, emotional revelation, or resolved cadence.

End before both melodic and harmonic completion,
then leave at least five seconds of clean silence inside the delivered file.
The silence must feel available for the final sentence,
not like an announcement that the story is over.

Keep the recording close, dry, and human in scale.
Use narrow-to-moderate stereo width, restrained reverb,
controlled upper mids, and generous space for quiet Japanese dialogue
and separately mixed present-day ambience.

No vocals. No guitar. No drums or percussion.
No strings, cello, flute, bells, celesta, music box, choir,
ambient pad, drone, field recording, spring birds,
wind recording, train recording, vinyl noise, or tape hiss.
No funeral mood, memorial music, grief montage, reunion theme,
healing theme, hopeful new beginning, nostalgic waltz,
pastoral fantasy, whimsical animation style,
sad-piano ballad, cinematic swell, final-episode climax,
or resolved final chord.
```

同じプロンプトで三テイクを生成する。死別、墓参り、春の再生、希望、感動的な最終回に聞こえる候補は不採用にする。バスーンが人物の声や故人の返答に聞こえず、単に別の現在が一度だけ演奏へ参加したと感じられる候補を残す。最後の五秒が「終わった合図」ではなく、まだ文章を一行置ける空間として聞こえることを採用条件にする。

## 21. 生活の小さな手順の生成ブリーフ — `umk.everyday.table`

これは「幸せな日常」や「食事の温かさ」を描く曲ではない。缶のプルタブを開ける、安い食べ物を籠へ入れる、箸を置くといった、物語上の結論を持たない身体の手順へ短い歩幅を与える。三人の奏者は登場人物や家族を代理せず、一つの狭い時間を別々の役割で扱う。

使用候補は三箇所に限定する。DAY 4はミルクティーの缶を開けてから路線図を選ぶ前までを縮小版で、DAY 6はコンビニで水、おにぎり、アイスを籠へ入れてから会計を終えるまでを通常版で、DAY 8は旅館で「米に、味噌汁に、鮭。」が届いてから食事を終えるまでを縮小版で支える。DAY 8の皿洗いは感謝の行為なので曲を持ち込まない。DAY 5の雨の食事、DAY 9の残ったパン、DAY 10の立ち食いうどんには使わない。終盤まで同じ日常曲を反復すると、食べられなくなる事実を感傷的な回想へ変えてしまうためである。

```yaml
music_brief:
  cue_id: umk.everyday.table
  narrative_function: ordinary-procedure-without-emotional-conclusion
  candidate_uses:
    - day_4_milk_tea_can_reduced
    - day_6_convenience_store_full
    - day_8_inn_breakfast_reduced
  forbidden_uses:
    - day_5_rain_meal
    - day_9_remaining_bread
    - day_10_standing_noodles
    - confession
    - illness_deterioration
    - chapter_ending

  instrument_presence: played-trio
  foreground_instrument: dry Wurlitzer-style electric piano with tremolo off
  supporting_instruments:
    - fingered electric bass
    - one bassoon in the restrained tenor register
  player_roles:
    foreground: electric piano states a compact eight-bar procedural sentence
    boundary: bass adds long rounded notes after the first eight bars
    response: bassoon answers only selected phrase endings after sixteen bars
  forbidden_instruments:
    - guitar
    - acoustic_piano
    - drums
    - percussion
    - strings
    - brass
    - bells
    - ambient_pad

  felt_tempo_bpm: 84
  meter: 4/4
  phrase_length_bars: 8
  full_form_seconds: 128
  loop: seamless_full_form

  motif:
    character: compact-eight-bar-procedural-sentence
    return_policy: preserve the rhythm but exchange one adjacent pitch pair
    forbidden_readings:
      - cafe_jazz
      - family_warmth
      - cooking_comedy
      - shopping_music

  orchestration_route:
    bars_1_8: Wurlitzer alone
    bars_9_16: add electric bass
    after_bar_16: add bassoon only at selected two-bar phrase endings
    around_fifty_six_percent: remove bass and bassoon for eight bars
    final_return: Wurlitzer and bass return, bassoon gives one final answer
    loop_boundary: all notes release before approximately one-point-five seconds of literal air

  runtime:
    fade_in_ms: 450
    fade_out_ms: 550
    required_silence_after_ms: 500
    reduced_version: Wurlitzer foreground plus no more than one supporting player

  mix:
    perspective: close and dry
    stereo_width: narrow-to-moderate
    dialogue_space: generous
    low_volume_test: every player retains a distinct function
```

生成時は、作品名、登場人物名、参照曲名を外して次をそのまま使う。

```text
Original instrumental everyday cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, timing, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

This music accompanies small practical procedures:
opening a drink can, choosing inexpensive food,
placing a few items in a basket, setting down chopsticks,
and sharing an ordinary meal before moving on.
Nothing important is confessed or resolved.
Do not interpret the scene as family warmth, romance,
healing, nostalgia, happiness, or comic domestic life.
The cue should give these physical actions a modest playable pulse
without telling the listener how to feel about them.

Tempo: 84 BPM.
Meter: 4/4.
Use an original eight-bar sentence.
Total duration: approximately 2 minutes 8 seconds.
Create a seamless full-form loop.

Use exactly three identifiable players:
one dry Wurlitzer-style electric piano in the foreground,
one fingered electric bass,
and one bassoon in a restrained tenor register.
Do not use guitar or acoustic piano.

Begin with the Wurlitzer alone for eight bars.
Turn tremolo off.
Use a close dry attack, short release,
single notes and occasional plain two-note shapes.
Write one compact procedural sentence whose rhythm feels deliberate
but not mechanical, jaunty, syncopated, or cute.
Do not use chord comping, decorative arpeggios,
jazz sevenths or ninths, gospel voicings, blues phrasing,
romantic rubato, or a sentimental melody.

After eight bars, add fingered electric bass.
Use long rounded boundary notes with clean releases.
The bass must not walk, slap, play funk,
create a pop-rock groove, or double every keyboard onset.

After sixteen bars, allow the bassoon to answer
only selected phrase endings in short two-bar replies.
Use restrained tenor register, plain sustained articulation,
minimal vibrato, and clearly released endings.
The bassoon must not sound comic, clumsy, pastoral,
whimsical, elegiac, or like a character speaking.

When the opening sentence returns,
preserve its rhythm but exchange one adjacent pitch pair.
Do not decorate the motif or increase its emotional intensity.

At approximately 56 percent of the complete form,
remove both bass and bassoon for eight bars.
Let the Wurlitzer continue alone without slowing down
and without turning the absence into a dramatic break.
Then return with Wurlitzer and bass.
Allow the bassoon one final restrained reply.
Do not create a climax, final chorus, expanded orchestration,
or resolved ending.

Release every note cleanly before the loop boundary
and leave approximately 1.5 seconds of literal breathable air
before the opening Wurlitzer sentence begins again.
The restart should feel like another ordinary procedure,
not a new scene or a repeated theme.

Keep the recording close, dry, lightly aged, and human in scale.
Use narrow-to-moderate stereo width, very restrained reverb,
controlled upper mids, and generous space for quiet Japanese dialogue
and separately mixed room, shop, dish, and clothing sounds.
At low playback volume, all three players must retain separate roles;
the mix must not collapse into only bright keyboard attacks.

No vocals. No guitar. No acoustic piano.
No drums or percussion, strings, brass, bells, celesta,
music box, choir, ambient pad, drone, field recording,
vinyl noise, tape hiss, sound effects, or cinematic swell.
No jazz lounge, cafe music, cooking music, shopping music,
family theme, slice-of-life comedy, whimsical animation style,
romantic domestic scene, sentimental comfort,
funk groove, blues, gospel, pop-rock, or resolved final chord.
```

同じプロンプトで三テイクを生成する。ジャズ、カフェ、料理番組、買い物、家族団らん、日常コメディに聞こえる候補は不採用にする。三人が同じ和音を厚くするのではなく、鍵盤が手順、ベースが境界、バスーンが句末の返答として分離して聞こえる候補を残す。小音量でも高い鍵盤音だけにならず、八小節の二人不在が悲しい中断に聞こえないことを採用条件にする。

## 22. 何も起きない窓の生成ブリーフ — `umk.waiting.window`

これは待つ理由や、待った先に起こる事件の曲ではない。ルームサービスが届くまで、空いた車内から駅へ着くまで、始発前の会話が体調確認へ変わるまでという、結論以前の時間を扱う。時計、車輪、心拍を模倣せず、演奏が自分の歩幅を保つことで、画面外の時間だけが通過している状態を作る。

モーターを止めたヴィブラフォンを前景にする。回転するトレモロや長い残響で夢へ逃がさず、マレットが板を打ち、ペダルが音を止める物理を残す。アルトフルートは旋律を奪わず、十小節の観測句のうち限られた句末にだけ応答する。この前景と応答の関係は、後の `umk.tears.after-breath` で反転させる。

```yaml
music_brief:
  cue_id: umk.waiting.window
  narrative_function: uneventful-time-passing-before-consequence
  candidate_uses:
    - day_1_room_service_wait_reduced
    - day_4_empty_train_window_reduced
    - day_7_predeparture_waiting_room_full
  forbidden_uses:
    - prologue_hospital_routine
    - day_6_empty_platform
    - day_7_post-news_ferry_wait
    - day_9_night_waiting_room
    - day_10_deterioration
    - diagnosis
    - confession
    - revelation

  instrument_presence: played-duo
  foreground_instrument: motor-off vibraphone with soft yarn mallets
  supporting_instruments:
    - one alto flute
  player_roles:
    foreground: vibraphone repeats a ten-bar observation sentence with physical damping
    response: alto flute answers only selected phrase endings
  forbidden_instruments:
    - guitar
    - piano
    - electric_piano
    - bass
    - drums
    - additional_percussion
    - strings
    - brass
    - bells
    - ambient_pad

  felt_tempo_bpm: 63
  meter: 4/4
  phrase_length_bars: 10
  full_form_bars: 40
  full_form_seconds: 152
  loop: seamless_full_form

  motif:
    character: five-small-gestures-across-one-ten-bar-observation-sentence
    attack_density: two-to-four-vibraphone-attacks-per-two-bar-cell
    return_policy: keep contour and cell lengths; shift only one middle gesture by one beat
    forbidden_imitation:
      - clock
      - heartbeat
      - train_wheels
      - announcement_chime

  orchestration_route:
    bars_1_10: vibraphone alone
    bars_11_20: alto flute gives two separate two-note replies
    bars_21_30: repeat the observation sentence with one displaced middle gesture and one three-note flute reply
    bars_31_35: vibraphone alone without slowing
    bars_36_38: one final two-note alto-flute reply
    bars_39_40: vibraphone returns to the opening gesture and releases before the final one-point-five beats

  runtime:
    fade_in_ms: 600
    fade_out_ms: 700
    required_silence_after_ms: 650
    full_version: motor-off vibraphone and alto flute
    reduced_version: motor-off vibraphone alone from the same form

  mix:
    perspective: close and dry
    stereo_width: narrow
    motor: completely_off
    reverb: short_room_only
    dialogue_space: generous
```

生成時は、作品名、登場人物名、参照曲名を外して次をそのまま使う。

```text
Original instrumental waiting cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, timing, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

This cue accompanies ordinary intervals in which nothing decisive happens:
waiting for room service after a short phone call,
sitting in an almost empty train while a station approaches,
and looking through a clouded waiting-room window before departure.
Time continues, but the music must not predict what happens next.
Do not portray suspense, illness, loneliness, grief,
romance, safety, nostalgia, or relief.

The result must feel like a small physically played instrumental cue,
not ambient texture, wellness music, cinematic underscore,
or a symbolic depiction of waiting.

Tempo: 63 BPM.
Meter: 4/4.
Build the complete form from four original ten-bar sentences.
Total form: 40 bars, approximately 2 minutes 32 seconds.
Create a seamless full-form loop.

Use exactly two identifiable players:
one motor-off vibraphone in the foreground
and one alto flute used only as a restrained response.

For the vibraphone, use soft yarn mallets.
The motor must remain completely off:
no rotating tremolo and no artificial modulation.
Keep the instrument close and dry.
Let each mallet contact remain audible,
then use clear physical pedal damping before the next gesture.
Use low-middle and middle registers,
with no exposed sparkling high notes.

Write one original ten-bar observation sentence
made from five small two-bar gestures.
Each two-bar gesture should contain only two to four attacks,
using single notes and occasional plain two-note intervals.
Keep a clear tonal center without a functional emotional progression.
Use literal rests between gestures,
but do not dissolve into isolated ambient notes.

Do not use four-note chord voicings, decorative arpeggios,
jazz harmony, lounge comping, blues phrasing,
music-box patterns, clock-like repetition,
train-wheel rhythm, heartbeat imitation,
or an announcement-chime melody.

Bars 1 through 10: vibraphone alone.

Bars 11 through 20:
repeat the observation sentence
and let the alto flute give exactly two separate two-note replies
at selected phrase endings.
Use its low and middle register, plain breath-supported tone,
minimal vibrato, and clean releases.
Do not exaggerate breath noise.

Bars 21 through 30:
retain the same contour and five-cell length,
but delay only one middle vibraphone gesture by one beat.
The change should be perceptible without sounding like a mistake.
Allow the alto flute one restrained three-note reply.
It must not become a foreground melody.

Bars 31 through 35:
remove the alto flute and continue with vibraphone alone.
Do not slow down, thin into a dramatic pause,
or suggest that someone has left.

Bars 36 through 38:
allow one final two-note alto-flute reply.

Bars 39 through 40:
return to only the opening vibraphone gesture.
Release all resonance cleanly before the final one and a half beats.
Keep those final beats literally empty,
then let the loop restart on the original downbeat.
The boundary must feel like waiting continuing,
not a cadence, scene change, or new beginning.

Do not build a climax, countermelody, final chorus,
harmonic revelation, sentimental return, or resolved ending.
Dynamics should remain restrained and nearly level throughout.

Keep the recording close, dry, lightly aged, and human in scale.
Use narrow stereo width, a short physical room,
softened upper transients, and generous space for quiet Japanese dialogue
and separately mixed air conditioning, doors, train noise,
room tone, cans, clothing, and station announcements.

No vocals. No guitar, piano, electric piano, bass, drums,
or additional percussion.
No strings, brass, glockenspiel, marimba, celesta, bells,
music box, choir, synthesizer lead, ambient pad, drone,
field recording, vinyl noise, tape hiss, sound effects,
or cinematic swell.
No jazz lounge, cafe music, spa music, aquarium ambience,
lullaby, magical scene, dream sequence, suspense cue,
loneliness theme, grief music, romantic pause,
whimsical animation style, or resolved final chord.
```

同じプロンプトで三テイクを生成する。子守歌、悲しい回想、サスペンス、魔法、スパ、アクアリウム、ジャズラウンジに聞こえる候補は不採用にする。ヴィブラフォンの打鍵と制音が物理的に聞こえ、アルトフルートが人物の溜息や悲しみの声にならず、低音量でも十小節の歩幅が消えない候補を残す。終端の一・五拍をエンジン停止ではなく、次の観測へ続く空き時間として聞けることを採用条件にする。

## 23. 小さな用事の歩幅の生成ブリーフ — `umk.errand.steps`

この曲はコンビニ、洗濯、荷造りを楽しいイベントへ変えない。靴を履いて廊下へ出る、濡れた道を歩く、機械へ服を入れる、鞄を閉じるという、短い目的を持った身体の連鎖だけを扱う。目的地の魅力、食事の温かさ、金が減る不安、録音の意味が現れた時点で役割を終える。

低域マリンバは可愛さや異国情緒ではなく、手で演奏された短い行為の輪郭を担う。ドラムはキックと低いクロススティックだけに制限し、ハイハットやシェイカーの連続音を使わない。指弾きエレキベースはグルーヴを強化せず、八小節の境界を身体に伝える。三人を重ねても陽気、RPG、料理番組、泥棒の準備に聞こえないことを優先する。

```yaml
music_brief:
  cue_id: umk.errand.steps
  narrative_function: small-purposeful-actions-before-meaning
  candidate_uses:
    - day_2_late_night_store_round_trip_full
    - day_4_laundry_loading_reduced
    - day_6_packing_before_wallet_reduced
  forbidden_uses:
    - meal
    - recording
    - money_shortage
    - farewell
    - illness
    - pursuit
    - gratitude
    - chapter_ending

  instrument_presence: played-trio
  foreground_instrument: low-register marimba with medium-soft mallets
  supporting_instruments:
    - compact dry drum kit limited to kick and low cross-stick
    - fingered electric bass
  player_roles:
    foreground: marimba states an eight-bar procedural sentence without continuous ostinato
    motion: dry kit supplies an incomplete two-bar physical pulse
    boundary: electric bass marks selected four-bar and eight-bar boundaries
  forbidden_instruments:
    - guitar
    - piano
    - electric_piano
    - strings
    - woodwind
    - brass
    - vibraphone
    - glockenspiel
    - additional_percussion
    - ambient_pad

  felt_tempo_bpm: 92
  meter: 4/4
  phrase_length_bars: 8
  full_form_bars: 48
  full_form_seconds: 125
  loop: seamless_full_form

  motif:
    character: six-note-folded-procedural-figure
    phrase_shape: four-bar statement followed by four-bar plain response
    return_policy: preserve the first four attacks and reverse only the direction of the final two pitches
    forbidden_imitation:
      - footsteps
      - washing_machine
      - barcode_scanner
      - train
      - clock

  drum_language:
    kit_pieces:
      - soft dry kick
      - low cross-stick
    maximum_kick_attacks_per_bar: 2
    maximum_cross_stick_attacks_per_bar: 1
    continuous_eighth_notes: forbidden
    cymbals: forbidden
    fills: forbidden

  orchestration_route:
    bars_1_8: low marimba and dry kit, no bass
    bars_9_16: add electric bass at phrase boundaries
    bars_17_24: retain trio but remove cross-stick from alternate bars
    bars_25_32: remove the entire drum kit, leaving marimba and bass
    bars_33_40: return to the original trio without increasing density
    bars_41_48: remove bass and return to the opening marimba-and-kit state
    loop_boundary: no fill, crash, cadence, or held bass note

  runtime:
    fade_in_ms: 350
    fade_out_ms: 500
    required_silence_after_ms: 700
    full_version: marimba, dry kit, and electric bass
    reduced_version: marimba and electric bass from the same complete form

  mix:
    perspective: close and dry
    stereo_width: narrow-to-moderate
    transient_shape: rounded but physical
    dialogue_space: generous
```

生成時は、作品名、登場人物名、参照曲名を外して次をそのまま使う。

```text
Original instrumental errand cue for a quiet Japanese visual novel.
Write a completely new melody, harmony, timing, and arrangement.
Do not quote, imitate, or reconstruct any existing composition.

This music accompanies a chain of small purposeful actions:
putting on shoes, crossing a worn corridor,
walking to a late-night convenience store,
placing clothes into a washing machine,
folding a few belongings, and closing a bag.
The actions have practical goals but no emotional conclusion.

Do not make the destination attractive.
Do not portray shopping pleasure, food, domestic happiness,
adventure, urgency, stealth, comedy, or productive self-improvement.
The cue must stop being appropriate as soon as money,
illness, farewell, memory, or the meaning of an object becomes important.

Tempo: 92 BPM.
Meter: 4/4.
Use an original eight-bar sentence.
Total form: 48 bars, approximately 2 minutes 5 seconds.
Create a seamless full-form loop.

Use exactly three identifiable players:
one low-register marimba in the foreground,
one compact dry drum kit limited to soft kick and low cross-stick,
and one fingered electric bass.
Do not use guitar or piano.

Play the marimba with medium-soft mallets.
Keep it in the low and low-middle registers.
Use rounded physical attacks, natural short decay,
single notes, and rare plain two-note intervals.
Do not use rolls.

Write one original six-note folded procedural figure.
Shape it as a four-bar statement followed by a plain four-bar response.
Use literal rests and no continuous ostinato.
When the sentence returns, preserve its first four attacks
and reverse only the direction of the final two pitches.
The change must not sound like melodic development or a new theme.

The marimba must not sound cute, tropical, folkloric,
magical, mysterious, comic, or like a wooden xylophone character.
Do not use bright upper-register patterns,
arpeggiated chord loops, pentatonic travel music,
or a bouncy major-key melody.

The dry kit may use only a soft compact kick
and one low cross-stick sound.
Use no more than two kick attacks per bar
and no more than one cross-stick attack per bar.
Do not play continuous eighth notes.
Do not use hi-hat, ride, crash, shaker, tambourine,
tom fills, snare fills, hand percussion, or electronic clicks.
The pulse must not imitate footsteps, machinery,
train wheels, a clock, or a barcode scanner.
It must not become danceable.

Begin bars 1 through 8 with marimba and dry kit only.

At bar 9, add fingered electric bass.
Use only two or three rounded notes per four-bar unit,
placed near selected phrase boundaries.
The bass must not walk, slap, syncopate continuously,
double the marimba, or create funk, rock, or pop momentum.

Bars 17 through 24:
retain all three players,
but remove the cross-stick from alternate bars.
Do not compensate by making the kick or marimba busier.

Bars 25 through 32:
remove the entire drum kit.
Continue with marimba and bass at the same tempo.
This is a change of physical surface, not a reflective interlude.
Do not slow down or become sentimental.

Bars 33 through 40:
return to the original trio without increasing density or volume.

Bars 41 through 48:
remove the bass and return to the opening marimba-and-kit state.
Do not add a drum fill, cymbal, cadence, held bass note,
or final melodic gesture at the loop boundary.
The opening must resume as if the next small task has simply begun.

Keep dynamics restrained and nearly level.
The piece must feel physically played by three people
with separate jobs, not assembled from a cheerful rhythm preset.

Use a close dry recording, narrow-to-moderate stereo width,
short natural room decay, rounded transients,
controlled low mids, and generous space for quiet Japanese dialogue
and separately mixed footsteps, doors, rain residue,
washing-machine sound, shop ambience, bags, and clothing.

No vocals. No guitar, piano, electric piano, strings,
woodwinds, brass, vibraphone, glockenspiel, celesta, bells,
or additional percussion.
No synthesizer lead, ambient pad, drone, field recording,
vinyl noise, tape hiss, sound effects, or cinematic swell.
No RPG town music, overworld music, platform game,
puzzle music, stealth cue, heist preparation,
cooking montage, shopping theme, commercial jingle,
sitcom, slapstick, tropical style, world-music pastiche,
funk, jazz, Latin groove, upbeat pop, whimsical animation style,
or resolved final chord.
```

同じプロンプトで三テイクを生成する。RPGの町、パズル、料理番組、買い物、泥棒の準備、コミカルな木琴、南国、ファンクに聞こえる候補は不採用にする。マリンバが連続オスティナートではなく短い行為の句を演奏し、ドラムが足音や機械を真似ず、ベースが曲を陽気なグルーヴへ変えない候補を残す。三人を小音量で混ぜてもプリセット伴奏に潰れず、ドラムを抜いた八小節が感傷的な中間部に聞こえないことを採用条件にする。
