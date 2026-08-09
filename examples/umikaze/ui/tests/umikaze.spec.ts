import { expect, test } from "@playwright/test";

async function beginJapaneseRecord(page: import("@playwright/test").Page) {
  // Keep the suite valid for the GitHub Pages project URL as well as a local
  // root host. Vite emits relative assets, so the test must not throw away a
  // configured base path by navigating to the origin root.
  await page.goto("./");
  await expect(page.getByRole("heading", { name: "無意味な生にて、なお。" })).toBeVisible();
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

async function autosaveGenerations(page: import("@playwright/test").Page): Promise<number[]> {
  const namespace = process.env.UMIKAZE_DEMO === "true" ? "umikaze-demo-v2" : "umikaze-v5";
  return page.evaluate(async () => {
    const database = await new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(`aria-v3-${namespace}`, 1);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    try {
      const transaction = database.transaction("generations", "readonly");
      const request = transaction.objectStore("generations").index("slot").getAll(`${namespace}:autosave`);
      const records = await new Promise<Array<{ generation: number }>>((resolve, reject) => {
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      return records.map((record) => record.generation).sort((left, right) => left - right);
    } finally {
      database.close();
    }
  });
}

async function expectOneAutosaveAfterRestore(
  page: import("@playwright/test").Page,
  before: number[],
) {
  const expected = Math.max(...before) + 1;
  await expect.poll(async () => Math.max(...await autosaveGenerations(page))).toBe(expected);
  await page.waitForTimeout(300);
  expect(Math.max(...await autosaveGenerations(page))).toBe(expected);
}

test("first light reaches a playable chapter catalogue without an operation guide", async ({ page }) => {
  await beginJapaneseRecord(page);
  await expect(page.getByText("操作方法")).toHaveCount(0);
  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  await expect(catalogue).toBeVisible();
  await expect(catalogue.getByRole("button", { name: /PROLOGUE/ })).toBeVisible();
  await expect(catalogue.getByRole("button", { name: /^DAY 1(?:\s|—)/ })).toBeDisabled();
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
  await expect(page.getByRole("heading", { name: "無意味な生にて、なお。" })).toBeVisible();
  await expect(page.getByRole("button", { name: "日本語" })).toHaveCount(0);

  await page.reload();
  await expect(page.getByRole("heading", { name: "無意味な生にて、なお。" })).toBeVisible();
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

test("a warmed offline PWA reopens the reader from its static-host subpath", async ({ page, context }) => {
  test.setTimeout(45_000);
  // Reach one subtitle before disconnecting. This makes the assertion cover
  // the actual reader contract: its WASM runtime, hot PAK, and delayed text
  // face must all have a recoverable cache entry, not merely the title DOM.
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  await catalogue.getByRole("button", { name: /PROLOGUE/ }).click();
  await page.locator(".day-card").getByRole("button", { name: "次へ" }).click();
  const reader = page.locator(".reading-band");
  await expect(reader).toBeVisible();
  await waitForCompletedPage(page);
  const pageId = await reader.getAttribute("data-page-id");
  const prose = await reader.locator(".dialogue-text").innerText();
  expect(pageId).toBeTruthy();
  expect(prose).toBeTruthy();

  // The first navigation installs the worker; a reload gives it control of
  // the document before network access is withdrawn.
  await page.evaluate(() => navigator.serviceWorker.ready.then(() => undefined));
  await page.waitForTimeout(250);
  const generationsBeforeReload = await autosaveGenerations(page);
  expect(generationsBeforeReload.length).toBeGreaterThan(0);
  await page.reload({ waitUntil: "networkidle" });
  await expect(reader).toHaveAttribute("data-page-id", pageId!);
  await expect(reader.locator(".dialogue-text")).toHaveText(prose);
  // The current exact checkpoint is already durable. Restarting it must not
  // create a new IndexedDB generation merely because the startup tick runs.
  await expect.poll(() => autosaveGenerations(page)).toEqual(generationsBeforeReload);
  await page.waitForFunction(() => Boolean(navigator.serviceWorker?.controller));

  await context.setOffline(true);
  try {
    await page.reload({ waitUntil: "domcontentloaded" });
    await expect(reader).toHaveAttribute("data-page-id", pageId!);
    await expect(reader.locator(".dialogue-text")).toHaveText(prose);
  } finally {
    await context.setOffline(false);
  }
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

test("manual and quick restores each create one fresh automatic generation", async ({ page }) => {
  test.setTimeout(45_000);
  const advance = await beginFirstChapter(page);
  const reader = page.locator(".reading-band");
  await waitForCompletedPage(page);
  const savedPage = await reader.getAttribute("data-page-id");

  // Quick record, move on, then restore it through the same semantic command
  // used by F9. Exactly one fresh AUTO generation follows the restored view.
  await page.keyboard.press("F5");
  await page.waitForTimeout(200);
  await advance.click();
  await waitForCompletedPage(page);
  await page.waitForTimeout(300);
  const quickBefore = await autosaveGenerations(page);
  await page.keyboard.press("F9");
  await expect(reader).toHaveAttribute("data-page-id", savedPage!);
  await expectOneAutosaveAfterRestore(page, quickBefore);

  // Deliberate manual records use the identical post-restore AUTO contract.
  await page.keyboard.press("Escape");
  const menu = page.getByRole("dialog", { name: "メニュー" });
  await menu.getByRole("button", { name: "SAVE", exact: true }).click();
  await page.getByRole("dialog", { name: "SAVE" })
    .getByRole("button", { name: "記録 1 に残す" }).click();
  await expect(reader).toBeVisible();
  await reader.getByRole("button", { name: "次へ" }).click();
  await waitForCompletedPage(page);
  await page.waitForTimeout(300);
  await page.keyboard.press("Escape");
  await page.getByRole("dialog", { name: "メニュー" })
    .getByRole("button", { name: "LOAD", exact: true }).click();
  const load = page.getByRole("dialog", { name: "LOAD" });
  await expect(load).toBeVisible();
  await page.waitForTimeout(300);
  const manualBefore = await autosaveGenerations(page);
  await load.getByRole("button", { name: "記録 1 を開く" }).click();
  await expect(reader).toHaveAttribute("data-page-id", savedPage!);
  await expectOneAutosaveAfterRestore(page, manualBefore);
});

test("title and transparent RMenu use English commands with a stable localized description", async ({ page }) => {
  const isDemo = process.env.UMIKAZE_DEMO === "true";
  await page.setViewportSize({ width: 1440, height: 900 });
  await beginJapaneseRecord(page);
  await expect(page.getByRole("button", { name: "START" })).toBeVisible();
  await expect(page.getByRole("button", { name: "LOAD" })).toBeVisible();
  await expect(page.getByRole("button", { name: "EXTRA" })).toBeVisible();
  await expect(page.getByRole("button", { name: "CONFIG" })).toBeVisible();
  await expect(page.getByRole("button", { name: "EXIT" })).toHaveCount(0);
  const titleStage = page.locator(".record-title-screen--home");
  await expect(titleStage.locator(".record-stage-photograph--night-motion")).toHaveAttribute(
    "src",
    isDemo ? /station-night-pass-v1-/ : /night-window-motion-v1-/,
  );
  await expect(titleStage.locator(".title-record-card, .record-stage-slip, .title-opening")).toHaveCount(0);
  const titleTypeface = await titleStage.getByRole("heading", { name: "無意味な生にて、なお。" }).evaluate((element) => getComputedStyle(element).fontFamily);
  expect(titleTypeface).toContain("UmikazeTitle");
  await expect(titleStage.locator(".record-stage-quotations"))
    .toContainText("語ることのできないものについては、沈黙しなければならない。");
  await expect(titleStage.locator(".record-stage-quotations"))
    .toContainText("よだかは、実にみにくい鳥です。");
  await expect(titleStage.locator(".record-stage-quotation")).toHaveCount(24);
  await expect(titleStage.locator(".title-subtitle")).toHaveCount(0);
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
    const quotations = stage?.querySelector<HTMLElement>(".record-stage-quotations");
    return {
      noteBelowCommand: Boolean(commandBox && noteBox.top >= commandBox.bottom),
      dividerWidth: command ? getComputedStyle(command).borderBottomWidth : "missing",
      quotationAnimation: quotations ? getComputedStyle(quotations).animationName : "missing",
      quotationCount: quotations?.querySelectorAll(".record-stage-quotation").length || 0,
    };
  });
  expect(titleLayout.noteBelowCommand).toBe(true);
  expect(titleLayout.dividerWidth).toBe("0px");
  expect(titleLayout.quotationAnimation).toBe("none");
  expect(titleLayout.quotationCount).toBeLessThanOrEqual(32);
  const renderedQuotationSources = await titleStage.locator(".record-stage-quotation").evaluateAll((bands) =>
    [...new Set(bands.flatMap((band) => (band.getAttribute("data-quotation-sources") || "").split(" ").filter(Boolean)))],
  );
  expect(renderedQuotationSources).toHaveLength(45);

  await page.getByRole("button", { name: "LOAD" }).click();
  await expect(page.getByRole("dialog", { name: "LOAD" }).locator(".record-stage-photograph--understructure")).toHaveAttribute(
    "src",
    isDemo ? /hospital-corridor-overcast-v1-/ : /understructure-evening-v1-/,
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
  await expect(menu.getByRole("button", { name: "QUICK SAVE" })).toBeVisible();
  await expect(menu.getByRole("button", { name: "QUICK LOAD" })).toBeVisible();
  await expect(menu.getByRole("button", { name: "SAVE", exact: true })).toBeVisible();
  await menu.getByRole("button", { name: "SAVE", exact: true }).focus();
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

test("mobile title commands retain 44px touch targets without separating their description", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await beginJapaneseRecord(page);
  const title = page.locator(".record-title-screen--home");
  const metrics = await title.locator("[data-stage-menu-item]").evaluateAll((items) => items.map((item) => {
    const element = item as HTMLElement;
    const bounds = element.getBoundingClientRect();
    const note = element.querySelector<HTMLElement>(".focus-menu-inline-description");
    const command = element.querySelector<HTMLElement>(".focus-menu-command");
    const noteBounds = note?.getBoundingClientRect();
    const commandBounds = command?.getBoundingClientRect();
    return {
      label: element.getAttribute("data-aria-action"),
      height: bounds.height,
      visible: bounds.width > 0 && bounds.height > 0 && getComputedStyle(element).visibility !== "hidden",
      noteBelowCommand: !note || getComputedStyle(note).display === "none"
        || Boolean(commandBounds && noteBounds && noteBounds.top >= commandBounds.bottom),
      overflowsViewport: bounds.left < 0 || bounds.right > window.innerWidth,
    };
  }));
  expect(metrics.filter((item) => item.visible)).toHaveLength(4);
  expect(metrics.every((item) => !item.visible || item.height >= 44)).toBe(true);
  expect(metrics.every((item) => item.noteBelowCommand && !item.overflowsViewport)).toBe(true);
});

test("Web omits EXIT, and RMenu arrows move the focused command", async ({ page }) => {
  await beginJapaneseRecord(page);
  await expect(page.getByRole("button", { name: "EXIT" })).toHaveCount(0);

  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  await catalogue.getByRole("button", { name: /PROLOGUE/ }).click();
  await expect(page.getByRole("button", { name: "次へ" })).toBeVisible();
  await page.keyboard.press("Escape");

  const menu = page.getByRole("dialog", { name: "メニュー" });
  // Opening through Escape must give DOM focus to the same command that is
  // visibly selected; a player should be able to press a direction
  // immediately, without first tabbing or clicking the transparent menu.
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
  await config.getByRole("button", { name: "文字速度を上げる" }).focus();
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
  const opacity = config.getByRole("button", { name: "字幕の濃さを下げる" });
  await opacity.click();
  await expect(page.locator(".umikaze")).toHaveCSS("--subtitle-opacity", "0.96");

  await config.getByRole("button", { name: "DISPLAY" }).click();
  await config.getByRole("group", { name: "背景演出" }).getByRole("button", { name: "OFF" }).click();
  await expect(page.locator(".umikaze")).toHaveClass(/stage-effects-off/);

  await config.getByRole("button", { name: "SYSTEM" }).click();
  await expect(config.getByText("AUTO SAVE", { exact: true })).toHaveCount(0);
  await expect(config.locator(".settings-status")).toHaveCount(0);
});

test("system records remain selectable and copy events are not globally cancelled", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "LOAD" }).click();
  const emptyRecord = page.getByText("この記録には保存されていません。", { exact: true }).first();
  const selectionAndCopy = await emptyRecord.evaluate((element) => {
    const selection = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(element);
    selection?.removeAllRanges();
    selection?.addRange(range);
    const event = new Event("copy", { bubbles: true, cancelable: true });
    element.dispatchEvent(event);
    return { cancelled: event.defaultPrevented, selected: selection?.toString() || "" };
  });
  expect(selectionAndCopy.cancelled).toBe(false);
  expect(selectionAndCopy.selected).toBe("この記録には保存されていません。");
});

test("a rejected fullscreen request restores the VM setting and reports it", async ({ page }) => {
  await page.addInitScript(() => {
    let requests = 0;
    Object.defineProperty(document, "fullscreenElement", { configurable: true, get: () => null });
    Object.defineProperty(HTMLElement.prototype, "requestFullscreen", {
      configurable: true,
      value: () => {
        requests += 1;
        return Promise.reject(new DOMException("blocked", "NotAllowedError"));
      },
    });
    Object.defineProperty(window, "__umikazeFullscreenRequests", { configurable: true, get: () => requests });
  });
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "CONFIG" }).click();
  const config = page.getByRole("dialog", { name: "CONFIG" });
  await config.getByRole("button", { name: "DISPLAY" }).click();
  await config.getByRole("group", { name: "フルスクリーン" }).getByRole("button", { name: "ON" }).click();
  await expect(page.locator(".runtime-status--host")).toHaveText("フルスクリーンに切り替えられませんでした。");
  await page.evaluate(() => document.dispatchEvent(new Event("fullscreenchange")));
  await expect(page.locator(".runtime-status--host")).toHaveText("フルスクリーンに切り替えられませんでした。");
  await expect.poll(() => page.evaluate(() => Number(Reflect.get(window, "__umikazeFullscreenRequests")))).toBe(1);
  await expect(config.getByRole("group", { name: "フルスクリーン" }).getByRole("button", { name: "OFF" })).toHaveAttribute("aria-pressed", "true");
});

test("legacy F5/F9 key values are cancelled before they bubble to browser defaults", async ({ page }) => {
  await beginJapaneseRecord(page);
  const result = await page.evaluate(() => {
    let bubbled = false;
    document.addEventListener("keydown", () => { bubbled = true; }, { once: true });
    const event = new KeyboardEvent("keydown", {
      key: "F5",
      code: "Unidentified",
      bubbles: true,
      cancelable: true,
    });
    const dispatched = document.dispatchEvent(event);
    return { cancelled: event.defaultPrevented, bubbled, dispatched };
  });
  expect(result.cancelled).toBe(true);
  expect(result.bubbled).toBe(false);
  expect(result.dispatched).toBe(false);
  await expect(page.getByRole("heading", { name: "無意味な生にて、なお。" })).toBeVisible();
});

test("host fullscreen changes synchronize the VM setting without another host request", async ({ page }) => {
  await page.addInitScript(() => {
    let fullscreen = false;
    let requests = 0;
    Object.defineProperty(document, "fullscreenElement", { configurable: true, get: () => fullscreen ? document.documentElement : null });
    Object.defineProperty(HTMLElement.prototype, "requestFullscreen", {
      configurable: true,
      value: () => { requests += 1; return Promise.resolve(); },
    });
    Object.defineProperty(window, "__umikazeSetFullscreen", {
      configurable: true,
      value: (next: boolean) => {
        fullscreen = next;
        document.dispatchEvent(new Event("fullscreenchange"));
      },
    });
    Object.defineProperty(window, "__umikazeFullscreenRequests", { configurable: true, get: () => requests });
  });
  await beginJapaneseRecord(page);
  await page.evaluate(() => (Reflect.get(window, "__umikazeSetFullscreen") as (next: boolean) => void)(true));
  await page.getByRole("button", { name: "CONFIG" }).click();
  const config = page.getByRole("dialog", { name: "CONFIG" });
  await config.getByRole("button", { name: "DISPLAY" }).click();
  await expect(config.getByRole("group", { name: "フルスクリーン" }).getByRole("button", { name: "ON" })).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => page.evaluate(() => Number(Reflect.get(window, "__umikazeFullscreenRequests")))).toBe(0);
  await page.evaluate(() => (Reflect.get(window, "__umikazeSetFullscreen") as (next: boolean) => void)(false));
  await expect(config.getByRole("group", { name: "フルスクリーン" }).getByRole("button", { name: "OFF" })).toHaveAttribute("aria-pressed", "true");
});

test("chapter catalogue fails closed: locked chapters cannot preview or receive keyboard focus", async ({ page }) => {
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  const firstPreview = await catalogue.locator(".chapter-preview-image").getAttribute("src");
  const dayOne = catalogue.getByRole("button", { name: /^DAY 1(?:\s|—)/ });
  await expect(dayOne).toBeDisabled();
  await dayOne.focus();
  await expect(dayOne).not.toBeFocused();
  await expect(page.getByRole("button", { name: "次へ" })).toHaveCount(0);
  await expect(catalogue.locator(".chapter-preview-image")).toHaveAttribute("src", firstPreview || "");
  const prologue = catalogue.getByRole("button", { name: "PROLOGUE", exact: true });
  await prologue.focus();
  await page.keyboard.press("ArrowDown");
  await expect(prologue).toBeFocused();
  await prologue.press("Enter");
  const card = page.locator(".day-card");
  await expect(card).toBeVisible();
  await expect(card.locator(".day-card-key")).toHaveText("PROLOGUE");
  await expect(catalogue.getByText(/^CHAPTER \d+$/)).toHaveCount(0);
});

test("gamepad navigation does not focus a locked chapter", async ({ page }) => {
  await page.addInitScript(() => {
    const buttons = Array.from({ length: 16 }, () => ({ pressed: false, value: 0 }));
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: () => [{ connected: true, index: 0, buttons }],
    });
    Object.defineProperty(window, "__umikazeGamepadPress", {
      configurable: true,
      value: (index: number, pressed: boolean) => {
        buttons[index] = { pressed, value: pressed ? 1 : 0 };
      },
    });
  });
  await beginJapaneseRecord(page);
  await page.getByRole("button", { name: "START" }).click();
  const catalogue = page.getByRole("dialog", { name: "CHAPTERS" });
  const prologue = catalogue.getByRole("button", { name: "PROLOGUE", exact: true });
  const dayOne = catalogue.getByRole("button", { name: /^DAY 1(?:\s|—)/ });
  await prologue.focus();
  await page.evaluate(() => (Reflect.get(window, "__umikazeGamepadPress") as (index: number, pressed: boolean) => void)(13, true));
  await page.waitForTimeout(80);
  await page.evaluate(() => (Reflect.get(window, "__umikazeGamepadPress") as (index: number, pressed: boolean) => void)(13, false));
  await expect(dayOne).not.toBeFocused();
  await expect(dayOne).toBeDisabled();
});

test("the first unlocked chapter card uses Core-provided canonical chapter metadata", async ({ page }) => {
  const card = await openChapterCard(page, "PROLOGUE");
  await expect(card).toBeVisible();
  await expect(card.locator(".day-card-kicker")).toHaveCount(0);
  await expect(card.locator(".day-card-key")).toHaveText("PROLOGUE");
  await expect(card.locator(".day-card-date")).toHaveText("春から九月");
  await expect(card.locator(".day-card-synopsis")).toHaveText("季節だけが先に進む窓辺で、まだ名もない願いが揺れている。");
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
  const card = await openChapterCard(page, "PROLOGUE");
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
  test.setTimeout(360_000);
  const chapters = ["PROLOGUE", "DAY 1", "DAY 2", "DAY 3", "DAY 4"];
  const selector = page.getByRole("dialog", { name: "CHAPTERS" });
  const demoEnd = page.locator(".demo-end-screen");
  const expectLocks = async (unlockedThrough: number) => {
    const states = await selector.locator("[data-chapter-index-item]").evaluateAll((items) => items.map((item) => ({
      action: item.getAttribute("data-aria-action"),
      label: item.getAttribute("aria-label"),
      disabled: (item as HTMLButtonElement).disabled,
    })));
    expect(states).toHaveLength(chapters.length);
    expect(states.map((state) => state.action)).toEqual(chapters.map((_, index) => `choice:${index}`));
    states.forEach((state, index) => {
      const expected = chapters[index];
      if (index <= unlockedThrough) {
        expect(state).toEqual({ action: `choice:${index}`, label: expected, disabled: false });
      } else {
        expect(state).toEqual({
          action: `choice:${index}`,
          label: `${expected} — まだ届かない記録`,
          disabled: true,
        });
      }
    });
  };
  const advanceToBoundary = async (chapter: string) => {
    for (let input = 0; input < 4_000; input += 1) {
      if (await selector.isVisible().catch(() => false) || await demoEnd.isVisible().catch(() => false)) return;
      await page.keyboard.press("Enter");
      // Typewriting/holds are VM-owned. This cadence advances only the same
      // public input route a player uses, while leaving each authored wait in
      // charge of its own completion.
      await page.waitForTimeout(18);
    }
    throw new Error(`${chapter} did not reach a chapter boundary within 4,000 Enter inputs`);
  };
  await beginJapaneseRecord(page);
  await expect(page.locator(".title-edition")).toHaveText("DEMO");
  // This is the same player-facing CONFIG control as release play, not a
  // storage fixture. It removes only typewriter pacing; every VM-authored
  // breath and statement is still crossed with ordinary Enter input below.
  await page.getByRole("button", { name: "CONFIG" }).click();
  const config = page.getByRole("dialog", { name: "CONFIG" });
  const decreaseTextSpeed = config.getByRole("button", { name: "文字速度を下げる" });
  for (let step = 0; step < 30; step += 1) await decreaseTextSpeed.click();
  await expect(config.locator(".setting-rail-value").first()).toHaveText("0 ms");
  await config.getByRole("button", { name: "閉じる" }).click();
  await page.getByRole("button", { name: "START" }).click();
  await expect(selector).toBeVisible();
  await expectLocks(0);

  for (let index = 0; index < chapters.length; index += 1) {
    const chapter = chapters[index];
    await selector.getByRole("button", { name: chapter, exact: true }).click();
    const card = page.locator(".day-card");
    await expect(card).toBeVisible();
    await expect(card.locator(".day-card-key")).toHaveText(chapter);
    await card.getByRole("button", { name: "次へ" }).click();
    await advanceToBoundary(chapter);

    if (index < chapters.length - 1) {
      await expect(selector).toBeVisible();
      await expectLocks(index + 1);
      await expect(demoEnd).toBeHidden();
    }
  }

  await expect(demoEnd).toBeVisible();
  await expect(demoEnd.getByRole("button", { name: "もう一度読む" })).toBeVisible();
  await expect(demoEnd.getByRole("button", { name: "タイトルへ戻る" })).toBeVisible();
  await expect(page.getByText("DAY 5", { exact: true })).toHaveCount(0);
});

test("an interlude is a dark logged story beat and every reading input releases it", async ({ page }) => {
  const card = await openChapterCard(page, "PROLOGUE");
  await card.getByRole("button", { name: "次へ" }).click();

  const interlude = page.locator(".interlude-screen");
  await expect(interlude).toBeVisible();
  const interludeText = "春から秋　ミオ";
  await expect(interlude.getByText(interludeText, { exact: true })).toBeVisible();
  // The black field starts over the selected location photograph and yields
  // back to it as the authored interlude ends.
  await expect(page.locator(".scene-photograph")).toHaveCount(1);
  const phase = await interlude.locator(".interlude-line").evaluate((element) =>
    Number.parseFloat(getComputedStyle(element).animationDelay),
  );
  expect(phase).toBeLessThanOrEqual(0);
  expect(phase).toBeGreaterThanOrEqual(-3.6);

  await page.keyboard.press("h");
  const backlog = page.getByRole("dialog", { name: "LOG" });
  await expect(backlog).toBeVisible();
  await expect(backlog.getByText(interludeText, { exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(interlude).toBeVisible();

  await page.keyboard.press("Enter");
  await expect(interlude).toBeHidden();
  await expect(page.getByRole("region", { name: "読書中" })).toBeVisible();
});

test("an automatic statement owns its duration while preserving log and menu escape hatches", async ({ page }) => {
  test.setTimeout(45_000);
  await beginFirstChapter(page);
  const statement = page.locator(".statement-screen");

  // The prologue reaches its one automatic central line through ordinary
  // reading input. Once it arrives, do not send a second kind of control to
  // release it: the story, rather than the reader, owns this interval.
  // Each authored sentence now owns a small semantic breath. Keep sending
  // the same ordinary reading input, but allow the slower default and those
  // breaths to carry the prologue to its central statement.
  for (let input = 0; input < 1_200 && !await statement.isVisible().catch(() => false); input += 1) {
    await page.keyboard.press("Enter");
    await page.waitForTimeout(8);
  }
  await expect(statement).toBeVisible();
  const statementText = "「だって、人が死ぬところなんて……わざわざ見たい人なんて、いないもんね」";
  await expect(statement.getByText(statementText, { exact: true })).toBeVisible();
  await expect(statement.locator("button")).toHaveCount(0);
  await expect(page.locator(".scene-photograph")).toHaveCount(0);

  await page.keyboard.press("Enter");
  await page.mouse.click(14, 14);
  await page.waitForTimeout(90);
  await expect(statement).toBeVisible();

  await page.keyboard.press("h");
  const backlog = page.getByRole("dialog", { name: "LOG" });
  await expect(backlog).toBeVisible();
  await expect(backlog.getByText(statementText, { exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(statement).toBeVisible();

  await expect(page.getByRole("region", { name: "読書中" })).toBeVisible({ timeout: 3_500 });
  await waitForCompletedPage(page);
  await expect(page.locator(".dialogue-text")).toHaveText("私はいつものように、窓のむこうを見る。");
});

test("a story scene selects its deterministic photograph after the chapter interlude", async ({ page }) => {
  const card = await openChapterCard(page, "PROLOGUE");
  await card.getByRole("button", { name: "次へ" }).click();

  await expect(page.locator(".interlude-screen")).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "読書中" })).toBeVisible();
  await expect(page.locator(".scene-photograph--corridor img"))
    .toHaveAttribute("src", /hospital-corridor-overcast-v1-/);
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

test("every ordinary reading surface, Enter, and a downward wheel gesture advance", async ({ page }) => {
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

test("the title supplies its own icon instead of causing a browser fallback request", async ({ page }) => {
  await beginJapaneseRecord(page);
  const icon = page.locator('link[rel="icon"]');
  await expect(icon).toHaveAttribute("href", "./icon.svg");
  const iconResponse = await page.request.get(new URL("icon.svg", page.url()).toString());
  expect(iconResponse.ok()).toBe(true);
  expect(iconResponse.headers()["content-type"]).toContain("image/svg+xml");
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

test("phone reading grids keep a two-line fixture visible through portrait and landscape", async ({ page }) => {
  const viewports = [
    { width: 320, height: 568 },
    { width: 375, height: 812 },
    { width: 430, height: 932 },
    { width: 568, height: 320 },
    { width: 812, height: 375 },
  ];
  await page.setViewportSize(viewports[0]);
  await beginFirstChapter(page);
  await waitForCompletedPage(page);
  let previousColumns = 0;
  for (const viewport of viewports) {
    await page.setViewportSize(viewport);
    await page.waitForTimeout(80);
    const metrics = await page.locator(".reading-band").evaluate((band) => {
      const text = band.querySelector<HTMLElement>(".dialogue-text")!;
      const columns = Number.parseInt(getComputedStyle(band).getPropertyValue("--subtitle-columns"), 10);
      const line = "海".repeat(Math.max(1, Math.floor(columns / 2)));
      const fixture = `${line}\n${line}`;
      text.textContent = fixture;
      const style = getComputedStyle(text);
      const lineHeight = Number.parseFloat(style.lineHeight);
      return {
        documentOverflow: document.documentElement.scrollWidth > window.innerWidth,
        horizontalOverflow: text.scrollWidth > text.clientWidth,
        twoLineHeight: text.scrollHeight <= lineHeight * 2 + 1,
        lastGrapheme: text.textContent?.at(-1),
        columns,
        target: band.querySelector<HTMLElement>(".reading-advance")?.getBoundingClientRect().height || 0,
      };
    });
    expect(metrics.documentOverflow).toBe(false);
    expect(metrics.horizontalOverflow).toBe(false);
    expect(metrics.twoLineHeight).toBe(true);
    expect(metrics.lastGrapheme).toBe("海");
    expect(metrics.target).toBeGreaterThanOrEqual(44);
    if (previousColumns) expect(metrics.columns).not.toBe(previousColumns);
    previousColumns = metrics.columns;
  }
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
