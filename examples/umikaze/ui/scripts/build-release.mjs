import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const uiRoot = resolve(here, "..");
const gameRoot = resolve(uiRoot, "..");
const repository = resolve(gameRoot, "../..");
const requestedMode = process.argv[2] || "desktop";
const edition = requestedMode.startsWith("demo-") ? "demo" : "full";
const mode = requestedMode.startsWith("demo-") ? requestedMode.slice("demo-".length) : requestedMode;
const profile = process.env.ARIA_PAK_PROFILE || "signed";

if (!['desktop', 'web'].includes(mode)) {
  console.error("usage: npm run release:desktop | npm run release:web | npm run release:demo:desktop | npm run release:demo:web");
  process.exit(2);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: uiRoot, stdio: "inherit", ...options });
  if (result.status !== 0) process.exit(result.status || 1);
}

if (profile === "dev") {
  throw new Error("release packaging cannot use ARIA_PAK_PROFILE=dev; use signed or protected");
}
if (profile === "protected") {
  throw new Error("Umikaze WebView releases currently support signed PAKs only; use the native aria CLI for protected bundles");
}

const env = {
  ...process.env,
  ARIA_RELEASE: "true",
  ARIA_PAK_PROFILE: profile,
  ARIA_UMIKAZE_EDITION: edition,
};

if (mode === "web") {
  run("node", [resolve(uiRoot, "scripts/prepare-desktop.mjs")], { env });
  const releaseDir = resolve(repository, `dist/releases/${edition === "demo" ? "demo-web" : "web"}`);
  const cargo = process.env.CARGO || (process.platform === "win32" ? "cargo.exe" : "cargo");
  run(cargo, [
    "run", "--release", "--no-default-features", "-p", "aria-cli", "--", "package", resolve(gameRoot, "dist/web"),
    "--format", "web", "--out", releaseDir,
  ], { cwd: repository, env });
  console.log(`Web release ready: ${releaseDir}`);
  process.exit(0);
}

const bundles = process.env.ARIA_TAURI_BUNDLES || (() => {
  if (process.platform === "darwin") return "dmg";
  if (process.platform === "win32") return "nsis";
  return "deb";
})();
if (process.platform === "linux" && bundles.includes("appimage") && !existsSync("/usr/bin/appimagetool")) {
  console.warn("appimage requested; Tauri will require appimagetool on this runner");
}
const tauriArgs = ["run", "tauri", "--", "build"];
if (edition === "demo") tauriArgs.push("--config", "src-tauri/tauri.demo.conf.json");
tauriArgs.push("--bundles", bundles);
run("npm", tauriArgs, { env });
