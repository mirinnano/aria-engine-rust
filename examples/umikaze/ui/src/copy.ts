export type Locale = "ja-JP" | "en-US" | "zh-CN" | "zh-TW";

type MenuDescription = {
  start: string;
  resume: string;
  auto: string;
  skip: string;
  log: string;
  save: string;
  load: string;
  extra: string;
  config: string;
  title: string;
  exit: string;
};

type Copy = {
  title: string;
  subtitle: string;
  opening: string;
  openingRecord: string;
  demoComplete: string;
  demoLead: string;
  demoReplay: string;
  demoReturn: string;
  begin: string;
  menu: string;
  history: string;
  close: string;
  save: string;
  load: string;
  settings: string;
  chapters: string;
  gallery: string;
  auto: string;
  skip: string;
  reset: string;
  returnToTitle: string;
  returnToReading: string;
  quit: string;
  readingControls: string;
  records: string;
  environment: string;
  sound: string;
  display: string;
  choices: string;
  confirm: string;
  confirmReset: string;
  confirmQuit: string;
  confirmResume: string;
  proceed: string;
  cancel: string;
  ok: string;
  ng: string;
  next: string;
  saveSlot: (slot: number) => string;
  loadSlot: (slot: number) => string;
  recordIndex: (slot: number) => string;
  saveLead: string;
  loadLead: string;
  writeRecord: string;
  openRecord: string;
  previousRecord: string;
  emptyRecord: string;
  memory: (index: number) => string;
  previousMemory: string;
  nextMemory: string;
  firstLight: string;
  languagePrompt: string;
  reading: string;
  noEntries: string;
  locked: string;
  textSpeed: string;
  autoDelay: string;
  subtitleOpacity: string;
  music: string;
  effects: string;
  voice: string;
  textSize: string;
  fullscreen: string;
  contrast: string;
  reducedMotion: string;
  stageEffects: string;
  skipUnread: string;
  startupIssue: string;
  reopenRecord: string;
  menuDescription: MenuDescription;
  valuePercent: (value: number) => string;
  valueMs: (value: number) => string;
};

const copy: Record<Locale, Copy> = {
  "ja-JP": {
    title: "海風",
    subtitle: "The Records of Autumn",
    opening: "波が静まるまで、ここに記しておく。",
    openingRecord: "記録を開いています",
    demoComplete: "体験版はここまでです。",
    demoLead: "この先の記録は、まだ閉じられています。",
    demoReplay: "公開されている章を読み返す",
    demoReturn: "タイトル画面へ戻る",
    begin: "記録をはじめる",
    menu: "メニュー",
    history: "履歴",
    close: "閉じる",
    save: "セーブ",
    load: "ロード",
    settings: "設定",
    chapters: "章を選ぶ",
    gallery: "ギャラリー",
    auto: "オート",
    skip: "スキップ",
    reset: "リセット",
    returnToTitle: "タイトルへ",
    returnToReading: "記録へ戻る",
    quit: "終了",
    readingControls: "読む",
    records: "記録",
    environment: "環境",
    sound: "音",
    display: "表示",
    choices: "選択肢",
    confirm: "確認",
    confirmReset: "タイトルへ戻りますか？",
    confirmQuit: "アプリケーションを終了しますか？",
    confirmResume: "このページから読み直しますか？ 先の本文と選択の記録は新しい分岐になります。",
    proceed: "実行する",
    cancel: "戻る",
    ok: "OK",
    ng: "NG",
    next: "次へ",
    saveSlot: (slot) => `記録 ${slot} に残す`,
    loadSlot: (slot) => `記録 ${slot} を開く`,
    recordIndex: (slot) => String(slot).padStart(2, "0"),
    saveLead: "いま読んでいる場所を、ひとつの記録として残します。",
    loadLead: "残しておいた記録から、読む場所へ戻ります。",
    writeRecord: "この瞬間を残す",
    openRecord: "記録を開く",
    previousRecord: "以前の記録",
    emptyRecord: "この記録には保存されていません。",
    memory: (index) => `記憶の断片 ${String(index).padStart(2, "0")}`,
    previousMemory: "前の記憶",
    nextMemory: "次の記憶",
    firstLight: "FIRST LIGHT",
    languagePrompt: "記録を読む言葉を選んでください。",
    reading: "読書中",
    noEntries: "まだ読み返せる記録はありません。",
    locked: "まだ届かない記録",
    textSpeed: "文字速度",
    autoDelay: "オート待ち時間",
    subtitleOpacity: "字幕の濃さ",
    music: "BGM",
    effects: "効果音",
    voice: "ボイス",
    textSize: "文字サイズ",
    fullscreen: "フルスクリーン",
    contrast: "高コントラスト",
    reducedMotion: "動きを抑える",
    stageEffects: "背景演出",
    skipUnread: "未読もスキップ",
    startupIssue: "記録を開けませんでした。更新して、もう一度開きます。",
    reopenRecord: "更新して開き直す",
    menuDescription: {
      start: "記録をはじめる",
      resume: "読書画面へ戻る",
      auto: "文章を自動で送る",
      skip: "既読の文章をすばやく送る",
      log: "読んだ文章を確認する",
      save: "現在位置を記録する",
      load: "保存した記録を開く",
      extra: "解放済みの記憶を見る",
      config: "表示と音を設定する",
      title: "タイトル画面へ戻る",
      exit: "海風を終了する",
    },
    valuePercent: (value) => `${Math.round(value * 100)}%`,
    valueMs: (value) => `${Math.round(value)} ms`,
  },
  "en-US": {
    title: "Umikaze",
    subtitle: "The Records of Autumn",
    opening: "I will leave this here, until the sea settles.",
    openingRecord: "Opening the record",
    demoComplete: "This is the end of the demo.",
    demoLead: "The rest of this record remains closed.",
    demoReplay: "Read the available chapters again.",
    demoReturn: "Return to the title screen.",
    begin: "Begin the record",
    menu: "Menu",
    history: "History",
    close: "Close",
    save: "Save",
    load: "Load",
    settings: "Settings",
    chapters: "Chapters",
    gallery: "Gallery",
    auto: "Auto",
    skip: "Skip",
    reset: "Reset",
    returnToTitle: "Return to title",
    returnToReading: "Return to the record",
    quit: "Quit",
    readingControls: "Reading",
    records: "Records",
    environment: "Environment",
    sound: "Sound",
    display: "Display",
    choices: "Choices",
    confirm: "Confirm",
    confirmReset: "Return to the title screen?",
    confirmQuit: "Quit the application?",
    confirmResume: "Resume from this page? Later text and choices will become a new branch.",
    proceed: "Proceed",
    cancel: "Cancel",
    ok: "OK",
    ng: "No",
    next: "Next",
    saveSlot: (slot) => `Save to record ${slot}`,
    loadSlot: (slot) => `Open record ${slot}`,
    recordIndex: (slot) => String(slot).padStart(2, "0"),
    saveLead: "Keep this exact place as a record to return to.",
    loadLead: "Return to a place you kept in the record.",
    writeRecord: "Keep this moment",
    openRecord: "Open this record",
    previousRecord: "Earlier record",
    emptyRecord: "There is no saved record in this slot.",
    memory: (index) => `Fragment ${String(index).padStart(2, "0")}`,
    previousMemory: "Previous memory",
    nextMemory: "Next memory",
    firstLight: "FIRST LIGHT",
    languagePrompt: "Choose the language for this record.",
    reading: "Reading",
    noEntries: "There is no record to revisit yet.",
    locked: "A record still beyond the tide",
    textSpeed: "Text speed",
    autoDelay: "Auto delay",
    subtitleOpacity: "Subtitle opacity",
    music: "Music",
    effects: "Sound effects",
    voice: "Voice",
    textSize: "Text size",
    fullscreen: "Fullscreen",
    contrast: "High contrast",
    reducedMotion: "Reduce motion",
    stageEffects: "Stage effects",
    skipUnread: "Skip unread text",
    startupIssue: "The record could not be opened. Refresh it and try again.",
    reopenRecord: "Refresh the record",
    menuDescription: {
      start: "Begin reading this record.",
      resume: "Return to the reading screen.",
      auto: "Advance the text automatically.",
      skip: "Advance through text quickly.",
      log: "Review the text you have read.",
      save: "Record the current position.",
      load: "Open a saved record.",
      extra: "View unlocked memories.",
      config: "Adjust display and sound.",
      title: "Return to the title screen.",
      exit: "Exit Umikaze.",
    },
    valuePercent: (value) => `${Math.round(value * 100)}%`,
    valueMs: (value) => `${Math.round(value)} ms`,
  },
  "zh-CN": {
    title: "海风",
    subtitle: "The Records of Autumn",
    opening: "在海浪平息之前，把它记在这里。",
    openingRecord: "正在打开记录",
    demoComplete: "体验版到此结束。",
    demoLead: "之后的记录仍然封存着。",
    demoReplay: "重新阅读已公开的章节。",
    demoReturn: "回到标题画面。",
    begin: "开始阅读",
    menu: "菜单",
    history: "回顾",
    close: "关闭",
    save: "保存",
    load: "读取",
    settings: "设置",
    chapters: "章节",
    gallery: "图鉴",
    auto: "自动",
    skip: "跳读",
    reset: "重置",
    returnToTitle: "回到标题",
    returnToReading: "回到记录",
    quit: "退出",
    readingControls: "阅读",
    records: "记录",
    environment: "环境",
    sound: "声音",
    display: "显示",
    choices: "选项",
    confirm: "确认",
    confirmReset: "要回到标题吗？",
    confirmQuit: "要退出应用吗？",
    confirmResume: "要从这一页继续阅读吗？之后的文字和选择会成为新的分支。",
    proceed: "继续",
    cancel: "取消",
    ok: "确定",
    ng: "取消",
    next: "下一页",
    saveSlot: (slot) => `保存到记录 ${slot}`,
    loadSlot: (slot) => `打开记录 ${slot}`,
    recordIndex: (slot) => String(slot).padStart(2, "0"),
    saveLead: "将此刻阅读的位置留作一份记录。",
    loadLead: "从已经留下的记录回到阅读的位置。",
    writeRecord: "留下此刻",
    openRecord: "打开记录",
    previousRecord: "之前的记录",
    emptyRecord: "这个记录位没有保存内容。",
    memory: (index) => `记忆碎片 ${String(index).padStart(2, "0")}`,
    previousMemory: "上一段记忆",
    nextMemory: "下一段记忆",
    firstLight: "FIRST LIGHT",
    languagePrompt: "选择阅读这段记录的语言。",
    reading: "阅读中",
    noEntries: "还没有可以回看的记录。",
    locked: "尚未抵达的记录",
    textSpeed: "文字速度",
    autoDelay: "自动等待",
    subtitleOpacity: "字幕不透明度",
    music: "音乐",
    effects: "音效",
    voice: "语音",
    textSize: "文字大小",
    fullscreen: "全屏",
    contrast: "高对比度",
    reducedMotion: "减少动态效果",
    stageEffects: "背景演出",
    skipUnread: "跳过未读文本",
    startupIssue: "无法打开记录。请刷新后重新打开。",
    reopenRecord: "刷新并重新打开",
    menuDescription: {
      start: "开始阅读这段记录。",
      resume: "回到阅读画面。",
      auto: "自动推进文字。",
      skip: "快速推进已读文字。",
      log: "确认已经读过的文字。",
      save: "记录当前位置。",
      load: "打开已保存的记录。",
      extra: "查看已解锁的记忆。",
      config: "调整显示与声音。",
      title: "回到标题画面。",
      exit: "退出海风。",
    },
    valuePercent: (value) => `${Math.round(value * 100)}%`,
    valueMs: (value) => `${Math.round(value)} ms`,
  },
  "zh-TW": {
    title: "海風",
    subtitle: "The Records of Autumn",
    opening: "在海浪平息之前，把它記在這裡。",
    openingRecord: "正在開啟記錄",
    demoComplete: "體驗版到此結束。",
    demoLead: "之後的記錄仍然封存著。",
    demoReplay: "重新閱讀已公開的章節。",
    demoReturn: "回到標題畫面。",
    begin: "開始閱讀",
    menu: "選單",
    history: "回顧",
    close: "關閉",
    save: "儲存",
    load: "讀取",
    settings: "設定",
    chapters: "章節",
    gallery: "圖鑑",
    auto: "自動",
    skip: "跳讀",
    reset: "重設",
    returnToTitle: "回到標題",
    returnToReading: "回到記錄",
    quit: "結束",
    readingControls: "閱讀",
    records: "記錄",
    environment: "環境",
    sound: "聲音",
    display: "顯示",
    choices: "選項",
    confirm: "確認",
    confirmReset: "要回到標題嗎？",
    confirmQuit: "要結束應用程式嗎？",
    confirmResume: "要從這一頁繼續閱讀嗎？之後的文字和選擇會成為新的分支。",
    proceed: "繼續",
    cancel: "取消",
    ok: "確定",
    ng: "取消",
    next: "下一頁",
    saveSlot: (slot) => `儲存到記錄 ${slot}`,
    loadSlot: (slot) => `開啟記錄 ${slot}`,
    recordIndex: (slot) => String(slot).padStart(2, "0"),
    saveLead: "將此刻閱讀的位置留作一份記錄。",
    loadLead: "從已經留下的記錄回到閱讀的位置。",
    writeRecord: "留下此刻",
    openRecord: "開啟記錄",
    previousRecord: "先前的記錄",
    emptyRecord: "這個記錄位沒有儲存內容。",
    memory: (index) => `記憶碎片 ${String(index).padStart(2, "0")}`,
    previousMemory: "上一段記憶",
    nextMemory: "下一段記憶",
    firstLight: "FIRST LIGHT",
    languagePrompt: "選擇閱讀這段記錄的語言。",
    reading: "閱讀中",
    noEntries: "還沒有可以回看的記錄。",
    locked: "尚未抵達的記錄",
    textSpeed: "文字速度",
    autoDelay: "自動等待",
    subtitleOpacity: "字幕不透明度",
    music: "音樂",
    effects: "音效",
    voice: "語音",
    textSize: "文字大小",
    fullscreen: "全螢幕",
    contrast: "高對比度",
    reducedMotion: "減少動態效果",
    stageEffects: "背景演出",
    skipUnread: "跳過未讀文本",
    startupIssue: "無法開啟記錄。請重新整理後再開啟。",
    reopenRecord: "重新整理並再開啟",
    menuDescription: {
      start: "開始閱讀這段記錄。",
      resume: "回到閱讀畫面。",
      auto: "自動推進文字。",
      skip: "快速推進已讀文字。",
      log: "確認已經讀過的文字。",
      save: "記錄目前位置。",
      load: "開啟已儲存的記錄。",
      extra: "查看已解鎖的記憶。",
      config: "調整顯示與聲音。",
      title: "回到標題畫面。",
      exit: "結束海風。",
    },
    valuePercent: (value) => `${Math.round(value * 100)}%`,
    valueMs: (value) => `${Math.round(value)} ms`,
  },
};

export function localeFor(value: string): Locale {
  return value in copy ? (value as Locale) : "ja-JP";
}

export function strings(value: string): Copy {
  return copy[localeFor(value)];
}

export const languageNames: Array<{ locale: Locale; label: string; sublabel: string }> = [
  { locale: "ja-JP", label: "日本語", sublabel: "Japanese" },
  { locale: "en-US", label: "English", sublabel: "English" },
  { locale: "zh-CN", label: "简体中文", sublabel: "Simplified Chinese" },
  { locale: "zh-TW", label: "繁體中文", sublabel: "Traditional Chinese" },
];
