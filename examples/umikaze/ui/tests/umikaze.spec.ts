import { expect, test } from "@playwright/test";

async function beginJapaneseRecord(page: import("@playwright/test").Page) {
  // Keep the suite valid for the GitHub Pages project URL as well as a local
  // root host. Vite emits relative assets, so the test must not throw away a
  // configured base path by navigating to the origin root.
  await page.goto("./");
  await expect(page.getByRole("heading", { name: "海風" })).toBeVisible();
  await expect(page.getByRole("button", { name: "日本語" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "English" })).toHaveCount(0);
}

async function openChapterCard(
  page: import("@playwright/test").Page,
  chapter: string | RegExp,
) {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  await expect(catalogue).toBeVisible();
  const chapterButton =
    typeof chapter === "string"
      ? catalogue.getByRole("button", { name: chapter, exact: true })
      : catalogue.getByRole("button", { name: chapter });
  await chapterButton.click();
  const card = page.locator(".day-card");
  await expect(card).toBeVisible();
  return card;
}

async function beginFirstChapter(page: import("@playwright/test").Page) {
  const card = await openChapterCard(page, /PROLOGUE/);
  await card.getByRole("button", { name: "次へ" }).click();
  await expect(card).toBeHidden();
  const band = page.locator(".reading-band");
  // The card's semantic choice changes the route first; the VM emits the
  // first subtitle on its following deterministic tick. Wait for the actual
  // page identity rather than treating the empty dialogue shell as prose.
  await expect(band).toHaveAttribute("data-page-id", /.+/);
  const advance = band.getByRole("button", { name: "次へ" });
  await expect(advance).toBeVisible();
  return advance;
}

async function waitForCompletedPage(page: import("@playwright/test").Page) {
  await expect(page.locator(".continue-mark")).toBeVisible({ timeout: 15_000 });
}

test("first light reaches a playable chapter catalogue without an operation guide", async ({ page }) => {
  await beginJapaneseRecord(page);
  await expect(page.getByText("操作方法")).toHaveCount(0);
  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  await expect(catalogue).toBeVisible();
  await expect(catalogue.getByRole("button", { name: /PROLOGUE/ })).toBeVisible();
  await expect(catalogue.getByRole("button", { name: "DAY 1", exact: true })).toBeVisible();
});

test("title load opens a record table and explains an empty slot", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "LOAD" }).click();
  const load = page.getByRole("dialog", { name: "LOAD" });
  await expect(load).toBeVisible();
  const first = load.getByRole("button", { name: "記録 1 を開く" });
  await expect(first).toBeDisabled();
  await expect(load.getByText("この記録には保存されていません。", { exact: true })).toHaveCount(10);
});

test("the public release enters the Japanese title without language setup", async ({ page }) => {
  await page.goto("./");
  await expect(page.getByRole("heading", { name: "海風" })).toBeVisible();
  await expect(page.getByRole("button", { name: "日本語" })).toHaveCount(0);

  await page.reload();
  await expect(page.getByRole("heading", { name: "海風" })).toBeVisible();
  await expect(page.getByRole("button", { name: "日本語" })).toHaveCount(0);
});

test("a settled title does not keep an animation-frame loop alive", async ({ page }) => {
  await page.addInitScript(() => {
    const nativeRequestAnimationFrame = window.requestAnimationFrame.bind(window);
    let requests = 0;
    window.requestAnimationFrame = (callback) => {
      requests += 1;
      return nativeRequestAnimationFrame(callback);
    };
    Object.defineProperty(window, "__umikazeAnimationFrameRequests", {
      configurable: true,
      get: () => requests,
    });
  });
  await beginJapaneseRecord(page);
  // Let initial font/layout work and the one-shot title entrance settle.
  await page.waitForTimeout(600);
  const before = await page.evaluate(() => Number(Reflect.get(window, "__umikazeAnimationFrameRequests")));
  await page.waitForTimeout(600);
  const after = await page.evaluate(() => Number(Reflect.get(window, "__umikazeAnimationFrameRequests")));
  expect(after - before).toBeLessThanOrEqual(1);
});

test("automatic checkpoint stays invisible while LOAD exposes only manual records", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "LOAD" }).click();
  const load = page.getByRole("dialog", { name: "LOAD" });
  await expect(load.getByText("AUTO SAVE", { exact: true })).toHaveCount(0);
  await expect(load.locator("[data-aria-action='load.slot.0']")).toHaveCount(0);
  await expect(load.locator(".record-slot")).toHaveCount(10);
  await expect(load.getByRole("button", { name: "記録 1 を開く" })).toBeDisabled();
});

test("title and transparent RMenu use English commands with a stable localized description", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await beginJapaneseRecord(page);
  await expect(page.getByRole("button", { name: "START" })).toBeVisible();
  await expect(page.getByRole("button", { name: "LOAD" })).toBeVisible();
  await expect(page.getByRole("button", { name: "EXTRA" })).toBeVisible();
  await expect(page.getByRole("button", { name: "CONFIG" })).toBeVisible();
  await expect(page.getByRole("button", { name: "EXIT" })).toBeVisible();
  const titleStage = page.locator(".record-title-screen--home");
  await expect(titleStage.locator(".record-stage-photograph--window")).toHaveAttribute("src", /train-window-summer-v1-/);
  await expect(titleStage.locator(".title-record-card, .record-stage-slip, .title-opening")).toHaveCount(0);
  const titleTypeface = await titleStage.getByRole("heading", { name: "海風" }).evaluate((element) => getComputedStyle(element).fontFamily);
  expect(titleTypeface).toContain("UmikazeTitle");
  await expect(titleStage.locator(".record-stage-fragment--tractatus-silence")).toContainText("Wovon man nicht sprechen kann");
  await expect(titleStage.locator(".record-stage-fragment--yodaka")).toHaveText("よだかは、実にみにくい鳥です。");
  await expect(titleStage.locator(".record-stage-fragment")).toHaveCount(3);
  await expect(titleStage.getByText("AUTUMN RECORD / 03", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "LOAD" }).focus();
  const loadNote = page.getByText("保存した記録を開く", { exact: true });
  await expect(loadNote).toBeVisible();
  const titleLayout = await loadNote.evaluate((note) => {
    const command = note.closest<HTMLElement>("[data-stage-menu-item]");
    const commandLabel = command?.querySelector<HTMLElement>(".focus-menu-command");
    const noteBox = note.getBoundingClientRect();
    const commandBox = commandLabel?.getBoundingClientRect();
    const stage = document.querySelector<HTMLElement>(".record-title-screen--home");
    const fragments = stage?.querySelector<HTMLElement>(".record-stage-fragments");
    return {
      noteBelowCommand: Boolean(commandBox && noteBox.top >= commandBox.bottom),
      dividerWidth: command ? getComputedStyle(command).borderBottomWidth : "missing",
      fragmentAnimation: fragments ? getComputedStyle(fragments).animationName : "missing",
    };
  });
  expect(titleLayout.noteBelowCommand).toBe(true);
  expect(titleLayout.dividerWidth).toBe("0px");
  expect(titleLayout.fragmentAnimation).toBe("none");

  await page.getByRole("button", { name: "LOAD" }).click();
  // The demo deliberately excludes the later-day understructure photograph.
  // Its archive screen must still have a complete, authored fallback rather
  // than pulling an unreleased asset back into the public bundle.
  const archivePhoto = page.getByRole("dialog", { name: "LOAD" })
    .locator(".record-stage-photograph--understructure");
  await expect(archivePhoto).toHaveAttribute(
    "src",
    process.env.UMIKAZE_DEMO === "true"
      ? /hospital-corridor-overcast-v1-/
      : /understructure-evening-v1-/,
  );
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  await catalogue.getByRole("button", { name: /PROLOGUE/ }).click();
  await expect(page.getByRole("button", { name: "次へ" })).toBeVisible();
  await page.keyboard.press("Escape");
  const menu = page.getByRole("dialog", { name: "メニュー" });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("button", { name: "RESUME" })).toBeVisible();
  await expect(menu.getByRole("button", { name: "SAVE" })).toBeVisible();
  await menu.getByRole("button", { name: "SAVE" }).focus();
  const saveNote = menu.getByText("現在位置を記録する", { exact: true });
  await expect(saveNote).toBeVisible();
  const rmenuLayout = await saveNote.evaluate((note) => {
    const command = note.closest<HTMLElement>("[data-stage-menu-item]");
    const commandLabel = command?.querySelector<HTMLElement>(".focus-menu-command");
    const list = command?.parentElement;
    const noteBox = note.getBoundingClientRect();
    const commandLabelBox = commandLabel?.getBoundingClientRect();
    return {
      noteBelowCommand: Boolean(commandLabelBox && noteBox.top >= commandLabelBox.bottom),
      dividerWidth: command ? getComputedStyle(command).borderBottomWidth : "missing",
      rowGap: list ? getComputedStyle(list).rowGap : "missing",
    };
  });
  expect(rmenuLayout.noteBelowCommand).toBe(true);
  expect(rmenuLayout.dividerWidth).toBe("0px");
  expect(rmenuLayout.rowGap).not.toBe("0px");
  const surface = await menu.evaluate((element) => {
    const overlay = element.closest(".rmenu-overlay");
    const box = element.getBoundingClientRect();
    return {
      overlayBackground: overlay ? getComputedStyle(overlay).backgroundColor : "missing",
      menuBackground: getComputedStyle(element).backgroundColor,
      left: box.left,
      top: box.top,
    };
  });
  expect(surface.overlayBackground).toBe("rgba(0, 0, 0, 0)");
  expect(surface.menuBackground).toBe("rgba(0, 0, 0, 0)");
  expect(surface.left).toBeGreaterThanOrEqual(68);
  expect(surface.top).toBeGreaterThanOrEqual(68);
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
});

test("title EXIT confirms safely, and RMenu arrows move the focused command", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "EXIT" }).click();
  const confirm = page.getByRole("dialog", { name: "CONFIRM" });
  await expect(confirm).toBeVisible();
  await expect(confirm.getByText("アプリケーションを終了しますか？", { exact: true })).toBeVisible();
  await confirm.locator('[data-aria-action="confirm.cancel"]').click();
  await expect(page.getByRole("button", { name: "START" })).toBeVisible();

  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  await catalogue.getByRole("button", { name: /PROLOGUE/ }).click();
  await expect(page.getByRole("button", { name: "次へ" })).toBeVisible();
  await page.keyboard.press("Escape");

  const menu = page.getByRole("dialog", { name: "メニュー" });
  // The first command is focused on opening, so arrows are immediately
  // usable without a preliminary Tab or mouse action.
  await expect(menu.getByRole("button", { name: "RESUME" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(menu.getByRole("button", { name: "AUTO" })).toBeFocused();
  await expect(menu.getByText("文章を自動で送る", { exact: true })).toBeVisible();
});

test("CONFIG uses explicit rails, supports arrows, and keeps its value while open", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "CONFIG" }).click();
  const config = page.getByRole("dialog", { name: "CONFIG" });
  await expect(config).toBeVisible();
  await expect(config.locator('input[type="range"]')).toHaveCount(0);
  await expect(config.locator(".react-aria-Switch")).toHaveCount(0);
  await expect(config.getByText(/SECTION(?: INDEX)? \/?\d*/, { exact: false })).toHaveCount(0);
  await expect(config.getByRole("button", { name: "TEXT" })).toHaveAttribute("aria-pressed", "true");

  const textValue = config.locator(".setting-rail-value").first();
  const before = await textValue.textContent();
  await config.getByRole("button", { name: "文字速度: increase" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(textValue).not.toHaveText(before || "");
  const after = await textValue.textContent();

  await config.getByRole("button", { name: "SOUND" }).click();
  await expect(config.getByText("音", { exact: true })).toBeVisible();
  await config.getByRole("button", { name: "TEXT" }).click();
  await expect(textValue).toHaveText(after || "");
  await config.getByRole("button", { name: "閉じる" }).click();
  await page.getByRole("button", { name: "CONFIG" }).click();
  await expect(page.getByRole("dialog", { name: "CONFIG" }).locator(".setting-rail-value").first()).toHaveText(after || "");
});

test("CONFIG keeps high contrast and reduced-motion feedback deliberate", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "CONFIG" }).click();
  const config = page.getByRole("dialog", { name: "CONFIG" });
  await config.getByRole("button", { name: "DISPLAY" }).click();
  await config.getByRole("group", { name: "高コントラスト" }).getByRole("button", { name: "ON" }).click();
  await config.getByRole("group", { name: "動きを抑える" }).getByRole("button", { name: "ON" }).click();
  await expect(page.locator(".umikaze")).toHaveClass(/high-contrast/);
  await expect(page.locator(".umikaze")).toHaveClass(/reduce-motion/);
  const motion = await config.locator(".stage-sheet-content").evaluate((element) => {
    const style = getComputedStyle(element);
    return { duration: style.animationDuration, name: style.animationName };
  });
  expect(motion.name).toBe("stage-fade");
  expect(motion.duration).toBe("0.12s");
});

test("CONFIG adds reading atmosphere while keeping automatic checkpoints internal", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "CONFIG" }).click();
  const config = page.getByRole("dialog", { name: "CONFIG" });

  await config.getByRole("button", { name: "TEXT" }).click();
  const opacity = config.getByRole("button", { name: "字幕の濃さ: decrease" });
  await opacity.click();
  await expect(page.locator(".umikaze")).toHaveCSS("--subtitle-opacity", "0.96");

  await config.getByRole("button", { name: "DISPLAY" }).click();
  await config.getByRole("group", { name: "背景演出" }).getByRole("button", { name: "OFF" }).click();
  await expect(page.locator(".umikaze")).toHaveClass(/stage-effects-off/);

  await config.getByRole("button", { name: "SYSTEM" }).click();
  await expect(config.getByText("AUTO SAVE", { exact: true })).toHaveCount(0);
  await expect(config.locator(".settings-status")).toHaveCount(0);
});

test("chapter focus changes only the preview until a command is confirmed", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  const firstPreview = await catalogue.locator(".chapter-preview-image").getAttribute("src");
  const dayOne = catalogue.getByRole("button", { name: "DAY 1", exact: true });
  await dayOne.focus();
  await expect(dayOne).toHaveClass(/is-preview/);
  await expect(page.getByRole("button", { name: "次へ" })).toHaveCount(0);
  await expect(catalogue.locator(".chapter-preview-image")).not.toHaveAttribute("src", firstPreview || "");
  await expect(catalogue.locator(".chapter-preview-image")).toHaveAttribute("src", /station-night-pass-v1-/);
  await expect(catalogue.locator(".chapter-preview-date")).toHaveText("9月21日・横浜駅");
  await expect(catalogue.locator(".chapter-preview-description"))
    .toHaveText("西へ向かう最初の列車が、朝のホームを離れる。");
  await dayOne.press("Enter");
  await expect(page.locator(".day-card")).toBeVisible();
  await expect(page.locator(".day-card").getByRole("heading", { name: "DAY 1" })).toBeVisible();
  await expect(catalogue.getByText(/^CHAPTER \d+$/)).toHaveCount(0);
});

test("a chapter day card holds the place, weather, and spoiler-free invitation before prose", async ({ page }) => {
  const card = await openChapterCard(page, "DAY 1");
  await expect(card).toBeVisible();
  await expect(card.locator(".day-card-kicker")).toHaveCount(0);
  await expect(card.getByText("9月21日・横浜駅", { exact: true })).toBeVisible();
  await expect(card.getByText("西へ向かう最初の列車が、朝のホームを離れる。", { exact: true })).toBeVisible();
  await expect(card.getByRole("button", { name: "次へ" })).toBeVisible();

  await page.keyboard.press("Escape");
  const menu = page.getByRole("dialog", { name: "メニュー" });
  await expect(menu).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(card).toBeVisible();

  await page.keyboard.press("Enter");
  await expect(card).toBeHidden();
  await expect(page.getByRole("region", { name: "読書中" })).toBeVisible();
});

test("a chapter day card advances from its open surface as well as BEGIN", async ({ page }) => {
  const card = await openChapterCard(page, "DAY 1");
  await expect(card).toBeVisible();
  const box = await card.boundingBox();
  if (!box) throw new Error("day card has no visible bounds");

  // Deliberately click the prose region, not the explicit button.
  await page.mouse.click(box.x + box.width * 0.24, box.y + box.height * 0.36);
  await expect(card).toBeHidden();
  await expect(page.getByRole("region", { name: "読書中" })).toBeVisible();
});

test("the demo contains only the opening arc and reaches its quiet end after DAY 4", async ({ page }) => {
  test.skip(process.env.UMIKAZE_DEMO !== "true", "requires the demo content bundle");
  test.setTimeout(45_000);
  await beginJapaneseRecord(page);
  await expect(page.locator(".title-edition")).toHaveText("DEMO");
  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  await expect(catalogue).toBeVisible();
  await catalogue.getByRole("button", { name: "DAY 4", exact: true }).click();
  const card = page.locator(".day-card");
  await expect(card).toBeVisible();
  await card.getByRole("button", { name: "次へ" }).click();

  const demoEnd = page.locator(".demo-end-screen");
  for (let input = 0; input < 700 && !await demoEnd.isVisible().catch(() => false); input += 1) {
    await page.keyboard.press("Enter");
    await page.waitForTimeout(24);
  }
  await expect(demoEnd).toBeVisible();
  await expect(demoEnd.getByRole("button", { name: "もう一度読む" })).toBeVisible();
  await expect(demoEnd.getByRole("button", { name: "タイトルへ戻る" })).toBeVisible();
  await expect(page.getByText("DAY 5", { exact: true })).toHaveCount(0);
});

test("an interlude is a dark logged story beat and every reading input releases it", async ({ page }) => {
  const card = await openChapterCard(page, "DAY 1");
  await card.getByRole("button", { name: "次へ" }).click();

  const interlude = page.locator(".interlude-screen");
  await expect(interlude).toBeVisible();
  await expect(interlude.getByText("9月21日　横浜駅 6:00", { exact: true })).toBeVisible();
  await expect(page.locator(".scene-photograph")).toHaveCount(0);
  await expect(interlude.locator(".interlude-line")).toHaveCSS("animation-delay", "0.2s");

  await page.keyboard.press("h");
  const backlog = page.getByRole("dialog", { name: "LOG" });
  await expect(backlog).toBeVisible();
  await expect(backlog.getByText("9月21日　横浜駅 6:00", { exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(interlude).toBeVisible();

  await page.keyboard.press("Enter");
  await expect(interlude).toBeHidden();
  await expect(page.getByRole("region", { name: "読書中" })).toBeVisible();
});

test("a story scene selects its deterministic photograph after the chapter interlude", async ({ page }) => {
  const card = await openChapterCard(page, "DAY 1");
  await card.getByRole("button", { name: "次へ" }).click();

  await expect(page.locator(".interlude-screen")).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "読書中" })).toBeVisible();
  await expect(page.locator(".scene-photograph--station img"))
    .toHaveAttribute("src", /station-night-pass-v1-/);
});

test("subtitle content and Next are separate, fixed-grid controls", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await beginFirstChapter(page);
  const band = page.getByRole("region", { name: "読書中" });
  const metrics = await band.evaluate((element) => {
    const text = element.querySelector<HTMLElement>(".dialogue-text");
    const content = element.querySelector<HTMLElement>(".subtitle-content");
    const next = element.querySelector<HTMLElement>(".reading-advance");
    return {
      contentContainsButton: Boolean(content?.querySelector("button")),
      fontFamily: text ? getComputedStyle(text).fontFamily : "",
      whiteSpace: text ? getComputedStyle(text).whiteSpace : "",
      textWrap: text ? getComputedStyle(text).textWrap : "",
      textOverflow: text ? text.scrollWidth > text.clientWidth : true,
      nextHeight: next?.getBoundingClientRect().height || 0,
    };
  });
  expect(metrics.contentContainsButton).toBe(false);
  expect(metrics.fontFamily).toContain("AriaBundledFont0");
  expect(metrics.whiteSpace).toBe("pre");
  expect(metrics.textWrap).not.toBe("balance");
  expect(metrics.textOverflow).toBe(false);
  expect(metrics.nextHeight).toBeGreaterThanOrEqual(44);
  await expect(band.getByRole("button", { name: "次へ" })).toBeVisible();
});

test("a completed page advances to the next page or source line only on the following input", async ({ page }) => {
  const advance = await beginFirstChapter(page);
  const band = page.locator(".reading-band");
  const firstPage = await band.getAttribute("data-page-id");
  await waitForCompletedPage(page);
  await advance.click();
  await expect(band).not.toHaveAttribute("data-page-id", firstPage || "");
  await expect(page.locator(".continue-mark")).toHaveCount(0);
});

test("every ordinary reading surface, Enter, Space, and a downward wheel gesture advance", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await beginFirstChapter(page);
  const band = page.locator(".reading-band");

  await waitForCompletedPage(page);
  const topEdgePage = await band.getAttribute("data-page-id");
  await page.mouse.click(12, 12);
  await expect(band).not.toHaveAttribute("data-page-id", topEdgePage || "");

  await waitForCompletedPage(page);
  const enterPage = await band.getAttribute("data-page-id");
  await page.keyboard.press("Enter");
  await expect(band).not.toHaveAttribute("data-page-id", enterPage || "");

  await waitForCompletedPage(page);
  const spacePage = await band.getAttribute("data-page-id");
  await page.keyboard.press("Space");
  await expect(band).not.toHaveAttribute("data-page-id", spacePage || "");

  await waitForCompletedPage(page);
  const wheelPage = await band.getAttribute("data-page-id");
  await page.mouse.wheel(0, 120);
  await expect(band).not.toHaveAttribute("data-page-id", wheelPage || "");
});

test("H opens history and a history page resumes through an OK / NG confirmation", async ({ page }) => {
  const advance = await beginFirstChapter(page);
  const band = page.locator(".reading-band");
  const firstPage = await band.getAttribute("data-page-id");
  await waitForCompletedPage(page);
  await advance.click();
  await waitForCompletedPage(page);

  await page.keyboard.press("h");
  const backlog = page.getByRole("dialog", { name: "LOG" });
  await expect(backlog).toBeVisible();
  const ledger = backlog.locator(".backlog-list");
  await expect(ledger).toHaveAttribute("role", "region");
  await expect(ledger).toHaveAttribute("tabindex", "0");
  await expect(ledger).toHaveAttribute("aria-keyshortcuts", "PageUp PageDown Home End");
  const scrollSurface = await ledger.evaluate((element) => ({
    ownOverflow: getComputedStyle(element).overflowY,
    sheetOverflow: getComputedStyle(element.closest(".stage-sheet-content")!).overflowY,
  }));
  expect(scrollSurface.ownOverflow).toBe("auto");
  expect(scrollSurface.sheetOverflow).toBe("hidden");
  await ledger.focus();
  await page.keyboard.press("PageDown");
  await expect(ledger).toBeFocused();
  // Chapter-entry interludes are intentionally recorded too.  Resume the
  // prose page that was on screen, rather than assuming the first log row is
  // prose.
  const firstEntry = backlog.locator(`[data-aria-action="backlog:${firstPage}"]`);
  await expect(firstEntry).toBeVisible();
  await firstEntry.click();

  const confirm = page.getByRole("dialog", { name: "CONFIRM" });
  await expect(confirm).toBeVisible();
  await expect(confirm.getByText("このページから読み直しますか？ 先の本文と選択の記録は新しい分岐になります。"))
    .toBeVisible();
  await confirm.getByRole("button", { name: "NG" }).click();
  await expect(backlog).toBeVisible();

  await firstEntry.click();
  await confirm.getByRole("button", { name: "OK" }).click();
  await expect(backlog).toBeHidden();
  await expect(band).toHaveAttribute("data-page-id", firstPage || "");
  await expect(page.locator(".continue-mark")).toBeVisible();
});

test("top edge advances, while H and right click keep their intended topmost routes", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await beginFirstChapter(page);
  await waitForCompletedPage(page);

  const band = page.locator(".reading-band");
  const firstPage = await band.getAttribute("data-page-id");
  await page.mouse.click(12, 12);
  await expect(band).not.toHaveAttribute("data-page-id", firstPage || "");

  const backlog = page.getByRole("dialog", { name: "LOG" });
  await page.keyboard.press("h");
  await expect(backlog).toBeVisible();
  await backlog.locator(".backlog-list").click({ button: "right", position: { x: 8, y: 8 } });
  await expect(backlog).toBeHidden();
  await expect(page.getByRole("dialog", { name: "メニュー" })).toBeHidden();

  await page.keyboard.press("h");
  await expect(backlog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(backlog).toBeHidden();
});

test("Escape opens rmenu on the chapter invitation reading surface", async ({ page }) => {
  await openChapterCard(page, /PROLOGUE/);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "メニュー" })).toBeVisible();
});

test("gallery does not invent a memory before the scenario releases a CG", async ({ page }) => {
  await beginFirstChapter(page);
  await page.keyboard.press("Escape");
  const menu = page.getByRole("dialog", { name: "メニュー" });
  await expect(menu).toBeVisible();
  await menu.getByRole("button", { name: "EXTRA" }).click();

  const gallery = page.getByRole("dialog", { name: "EXTRA" });
  await expect(gallery).toBeVisible();
  await expect(gallery.getByText("まだ読み返せる記録はありません。", { exact: true })).toBeVisible();
  await expect(gallery.locator(".gallery-card")).toHaveCount(0);
});

test("narrow reading layout has no horizontal overflow and preserves a 44px Next target", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await beginFirstChapter(page);
  const metrics = await page.evaluate(() => ({
    overflow: document.documentElement.scrollWidth > window.innerWidth,
    target: document.querySelector(".reading-advance")?.getBoundingClientRect().height || 0,
  }));
  expect(metrics.overflow).toBe(false);
  expect(metrics.target).toBeGreaterThanOrEqual(44);
});

test("settled title creates neither a hidden GPU context nor a continuous animation clock", async ({ page }) => {
  await page.addInitScript(() => {
    const monitored = window as Window & { draws?: number; frames?: number; contexts?: number };
    monitored.draws = 0;
    monitored.frames = 0;
    monitored.contexts = 0;
    const originalRaf = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (callback) => {
      monitored.frames = (monitored.frames || 0) + 1;
      return originalRaf(callback);
    };
    const prototype = HTMLCanvasElement.prototype as unknown as {
      getContext: (this: HTMLCanvasElement, contextId: string, options?: unknown) => unknown;
    };
    const originalContext = prototype.getContext;
    prototype.getContext = function getContext(contextId, options) {
      if (contextId === "webgl" || contextId === "webgl2" || contextId === "webgpu") {
        monitored.contexts = (monitored.contexts || 0) + 1;
      }
      return originalContext.call(this, contextId, options);
    };
  });
  await beginJapaneseRecord(page);
  await page.waitForTimeout(850);
  const before = await page.evaluate(() => ({
    frames: (window as Window & { frames?: number }).frames || 0,
    contexts: (window as Window & { contexts?: number }).contexts || 0,
  }));
  await page.waitForTimeout(500);
  const after = await page.evaluate(() => (window as Window & { frames?: number }).frames || 0);
  expect(before.contexts).toBe(0);
  expect(after).toBe(before.frames);
});
