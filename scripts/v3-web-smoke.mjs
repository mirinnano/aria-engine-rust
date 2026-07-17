import { chromium } from "playwright";

const url = process.argv[2] || "http://127.0.0.1:4173/";
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
const errors = [];
page.on("pageerror", (error) => errors.push(String(error)));
page.on("console", (message) => {
  if (message.type() === "error") errors.push(message.text());
});
await page.addInitScript(() => {
  globalThis.__ariaFrameCount = 0;
  addEventListener("aria-render-frame", () => globalThis.__ariaFrameCount++);
});
await page.goto(url, { waitUntil: "networkidle" });
await page.waitForFunction(() => globalThis.__ariaFrameCount > 0, null, { timeout: 20_000 });
await page.waitForFunction(
  () => ["webgpu", "webgl2"].includes(globalThis.__ariaRendererBackend)
    && Number.isInteger(globalThis.__ariaRenderedFrame),
  null,
  { timeout: 20_000 },
);
const status = await page.locator("#aria-status").textContent();
if (status?.includes("起動失敗")) errors.push(status);
const canvas = await page.locator("#aria-canvas").evaluate((element) => ({
  width: element.width,
  height: element.height,
}));
if (canvas.width < 1 || canvas.height < 1) errors.push("renderer canvas has no drawable size");
const backend = await page.evaluate(() => globalThis.__ariaRendererBackend);
if (errors.length) throw new Error(`V3 Web smoke errors:\n${errors.join("\n")}`);
await browser.close();
console.log(`V3 Web rendered a ${backend} runtime frame at ${url}`);
