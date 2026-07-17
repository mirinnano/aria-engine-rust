import { chromium } from "playwright";

const url = process.argv[2] || "http://127.0.0.1:4173/";
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
const errors = [];
page.on("pageerror", (error) => errors.push(String(error)));
page.on("console", (message) => {
  if (message.type() === "error") errors.push(message.text());
});

await page.goto(url, { waitUntil: "networkidle" });
await page.waitForFunction(() => Number.isInteger(globalThis.__ariaRenderedFrame), null, {
  timeout: 20_000,
});

const result = await page.evaluate(async () => {
  const { IndexedDbSaveStore } = await import("./save-store.js");
  const { default: init, WebRuntime } = await import("./pkg/aria_web.js");
  await init();

  const bundle = await (await fetch("./bundle.aria.json")).json();
  const bytecode = new Uint8Array(
    await (await fetch("./game.ariac")).arrayBuffer(),
  );
  const runtime = new WebRuntime(
    bytecode,
    bundle.logical_width,
    bundle.logical_height,
  );
  const validPayload = runtime.save_envelope_json(1n);
  const databaseName = `aria-v3-save-smoke-${crypto.randomUUID()}`;
  const store = new IndexedDbSaveStore(databaseName, 3);
  await store.open();

  await store.put(bundle.save_namespace, 7, validPayload);
  const secondRuntime = new WebRuntime(
    bytecode,
    bundle.logical_width,
    bundle.logical_height,
  );
  const secondPayload = secondRuntime.save_envelope_json(2n);
  await store.put(bundle.save_namespace, 7, secondPayload);
  const beforeCorruption = await store.generations(bundle.save_namespace, 7);

  const newest = beforeCorruption[0];
  const corruptedEnvelope = JSON.parse(newest.payload);
  corruptedEnvelope.payload = null;
  await new Promise((resolve, reject) => {
    const transaction = store.database.transaction("generations", "readwrite");
    transaction.objectStore("generations").put({
      ...newest,
      payload: JSON.stringify(corruptedEnvelope),
    });
    transaction.oncomplete = resolve;
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error);
  });

  const generations = await store.generations(bundle.save_namespace, 7);
  let skipped = 0;
  let recoveredGeneration = null;
  for (const generation of generations) {
    const probe = new WebRuntime(
      bytecode,
      bundle.logical_width,
      bundle.logical_height,
    );
    try {
      probe.restore_envelope_json(generation.payload);
      recoveredGeneration = generation.generation;
      break;
    } catch {
      skipped += 1;
    }
  }

  await new Promise((resolve, reject) => {
    const request = indexedDB.deleteDatabase(databaseName);
    request.onsuccess = resolve;
    request.onerror = () => reject(request.error);
    request.onblocked = resolve;
  });
  return {
    generationCount: beforeCorruption.length,
    skipped,
    recoveredGeneration,
  };
});

if (errors.length) {
  await browser.close();
  throw new Error(`V3 Web save smoke errors:\n${errors.join("\n")}`);
}
if (
  result.generationCount !== 2 ||
  result.skipped !== 1 ||
  result.recoveredGeneration !== 1
) {
  await browser.close();
  throw new Error(`unexpected save recovery result: ${JSON.stringify(result)}`);
}

await browser.close();
console.log(
  `V3 Web IndexedDB recovered generation ${result.recoveredGeneration} after skipping ${result.skipped} corrupt generation`,
);
