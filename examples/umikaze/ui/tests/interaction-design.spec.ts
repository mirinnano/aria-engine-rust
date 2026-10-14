import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";

async function title(page: import("@playwright/test").Page) {
  await page.goto("./");
  await expect(page.getByRole("heading", { name: "HesPhoros", exact: true })).toBeVisible();
}

for (const viewport of [{ width: 320, height: 568 }, { width: 390, height: 844 }, { width: 844, height: 390 }, { width: 1280, height: 600 }]) {
  test(`unframed title commands have separate 44px hit areas at ${viewport.width}x${viewport.height}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await title(page);
    const measurements = await page.locator(".title-command-list .focus-menu-item").evaluateAll((buttons) => buttons.map((button) => {
      const box = button.getBoundingClientRect();
      return { width: box.width, height: box.height, top: box.top, bottom: box.bottom,
        minimum: parseFloat(getComputedStyle(button).minHeight) };
    }));
    expect(measurements).toHaveLength(5);
    for (const box of measurements) {
      expect(box.width).toBeGreaterThanOrEqual(44);
      expect(box.minimum).toBeGreaterThanOrEqual(44);
      // Fractional ancestor transforms can report 43.999969px for a 44px box.
      expect(box.height).toBeGreaterThanOrEqual(44 - 0.01);
    }
    for (let index = 1; index < measurements.length; index++) {
      expect(measurements[index].top).toBeGreaterThanOrEqual(measurements[index - 1].bottom);
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await expect(page.locator(".record-stage-quotation")).toHaveCount(360);
  });
}

test("all sourced anchor quotations remain verbatim and all 360 fragments are present", async ({ page }) => {
  await title(page);
  const source = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  const quotations = source.slice(source.indexOf("const titleQuotations = ["), source.indexOf("] as const;", source.indexOf("const titleQuotations = [")));
  const expected = [...quotations.matchAll(/\s+text: "([^"]+)",/g)].map((match) => match[1]);
  expect(expected).toHaveLength(45);
  expect(await page.locator(".record-stage-quotation.is-anchor").allTextContents()).toEqual(expected);
  await expect(page.locator(".record-stage-quotation")).toHaveCount(360);
  await expect(page.locator(".record-stage-backdrop--title")).toHaveAttribute("aria-hidden", "true");
});

test("chapter dates stay legible and narrow propositions wrap without horizontal overflow", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await title(page);
  await page.getByRole("button", { name: "START" }).click();
  await expect(page.getByRole("dialog", { name: "CHAPTERS" })).toBeVisible();
  const metrics = await page.locator(".chapter-index-date").evaluateAll((dates) => dates.map((date) => ({
    size: parseFloat(getComputedStyle(date).fontSize),
    whiteSpace: getComputedStyle(date).whiteSpace,
  })));
  expect(metrics.length).toBeGreaterThan(0);
  for (const metric of metrics) {
    expect(metric.size).toBeGreaterThanOrEqual(14);
    expect(metric.whiteSpace).toBe("normal");
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test("a short viewport follows keyboard focus through the entire reading menu", async ({ page }) => {
  await page.setViewportSize({ width: 844, height: 390 });
  await title(page);
  await page.getByRole("button", { name: "START" }).click();
  await page.getByRole("button", { name: "PROLOGUE", exact: true }).click();
  await page.getByRole("button", { name: "次へ", exact: true }).click();
  await expect(page.locator(".reading-band")).toBeVisible();
  await page.keyboard.press("Escape");
  const menu = page.locator(".rmenu-command-list");
  await expect(menu).toBeVisible();
  const resume = menu.locator('[data-aria-action="dismiss"]');
  await resume.focus();
  for (let index = 0; index < 9; index++) await page.keyboard.press("ArrowDown");
  const exit = menu.locator('[data-aria-action="menu.quit"]');
  await expect(exit).toBeFocused();
  const metrics = await exit.evaluate((button) => {
    const box = button.getBoundingClientRect();
    const panel = button.closest(".stage-rmenu")!.getBoundingClientRect();
    const command = button.querySelector(".focus-menu-command")!;
    return { top: box.top, bottom: box.bottom, panelTop: panel.top, panelBottom: panel.bottom,
      size: box.height, underline: getComputedStyle(command).textDecorationLine };
  });
  expect(metrics.size).toBeGreaterThanOrEqual(44);
  expect(metrics.top).toBeGreaterThanOrEqual(metrics.panelTop);
  expect(metrics.bottom).toBeLessThanOrEqual(metrics.panelBottom);
  expect(metrics.underline).toContain("underline");
});

test("launch cards fade the maker mark, then the title mark, and a press skips ahead", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto("./?splash=1");
  const splash = page.locator(".brand-splash");
  const maker = splash.getByRole("img", { name: "PONKOTSU software（ぽんこつソフト）" });
  await expect(maker).toHaveClass(/is-visible/);
  await expect.poll(() => maker.evaluate((el) => Number(getComputedStyle(el).opacity))).toBeGreaterThan(0.8);
  // A key press ends the maker card without reaching the title menu below.
  await page.keyboard.press("Enter");
  const game = splash.getByRole("img", { name: "HesPhoros" });
  await expect(game).toHaveClass(/is-visible/, { timeout: 3_000 });
  await page.mouse.click(640, 360);
  await expect(splash).toHaveCount(0, { timeout: 3_000 });
  await expect(page.getByRole("heading", { name: "HesPhoros" })).toBeVisible();
  await expect(page.getByRole("dialog", { name: "CHAPTERS" })).toHaveCount(0);
});
