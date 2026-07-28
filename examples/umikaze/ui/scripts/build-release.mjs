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
const buildDir = resolve(gameRoot, "dist", "build", edition, "web");

if (!['desktop', 'web'].includes(mode)) {
  console.error("usage: npm run release:desktop | npm run release:web | npm run release:demo:desktop | npm run release:demo:web");
  process.exit(2);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: uiRoot, stdio: "inherit", ...options });
  if (result.status !== 0) process.exit(result.status || 1);
}

function releaseConfigurationError(message) {
  console.error(`Release configuration error: ${message}`);
  process.exit(2);
}

function requiredReleaseValue(name) {
  const value = process.env[name]?.trim();
  if (!value) {
    releaseConfigurationError(`${name} is required; keep its value in CI secrets and do not print it to logs`);
  }
  return value;
}

if (profile === "dev") {
  releaseConfigurationError("ARIA_PAK_PROFILE=dev cannot be released; use signed or protected");
}
if (profile === "protected") {
  releaseConfigurationError("Umikaze WebView releases currently support signed PAKs only; use the native aria CLI for protected bundles");
}

const env = {
  ...process.env,
  ARIA_RELEASE: "true",
  ARIA_PAK_PROFILE: profile,
  ARIA_UMIKAZE_EDITION: edition,
};

// A signed PAK is only useful when the WebView receives the matching public
// verification key. Validate presence (and the public key's shape) before an
// expensive frontend/WASM build, without ever echoing secret material.
if (profile === "signed") {
  const signingKey = requiredReleaseValue("ARIA_PAK_SIGNING_KEY");
  const verificationKeyId = requiredReleaseValue("ARIA_PAK_VERIFICATION_KEY_ID");
  const verificationKeyHex = requiredReleaseValue("ARIA_PAK_VERIFICATION_KEY_HEX");
  const keySeparator = signingKey.indexOf(":");
  const signingKeyHex = keySeparator >= 0 ? signingKey.slice(keySeparator + 1) : signingKey;
  if (!/^[a-f0-9]{64}$/i.test(signingKeyHex)) {
    releaseConfigurationError("ARIA_PAK_SIGNING_KEY must contain a 32-byte hexadecimal Ed25519 private key");
  }
  if (!/^[a-f0-9]{64}$/i.test(verificationKeyHex)) {
    releaseConfigurationError("ARIA_PAK_VERIFICATION_KEY_HEX must be a 32-byte hexadecimal Ed25519 public key");
  }
  const signingKeyId = keySeparator >= 0 ? signingKey.slice(0, keySeparator) : "publisher";
  if (signingKeyId && signingKeyId !== verificationKeyId) {
    releaseConfigurationError("ARIA_PAK_SIGNING_KEY key id must match ARIA_PAK_VERIFICATION_KEY_ID");
  }
}

if (mode === "web") {
  run("node", [resolve(uiRoot, "scripts/prepare-desktop.mjs")], { env });
  const releaseDir = resolve(repository, `dist/releases/${edition === "demo" ? "demo-web" : "web"}`);
  const cargo = process.env.CARGO || (process.platform === "win32" ? "cargo.exe" : "cargo");
  run(cargo, [
    "run", "--release", "--locked", "--no-default-features", "-p", "aria-cli", "--", "package", buildDir,
    "--format", "web", "--out", releaseDir,
  ], { cwd: repository, env });
  run("node", [
    resolve(uiRoot, "scripts/verify-release.mjs"),
    "--edition", edition,
    "--dir", releaseDir,
    "--profile", profile,
  ], { env });
  console.log(`Web release ready: ${releaseDir}`);
  process.exit(0);
}

const bundles = process.env.ARIA_TAURI_BUNDLES || (() => {
  if (process.platform === "darwin") return "dmg";
  if (process.platform === "win32") return "nsis";
  // Debian packages are only useful on one family of distributions.  The
  // game payload has already been staged by `aria build`; AppImage wraps that
  // exact payload in one runnable file for the broad Linux download.
  return "appimage";
})();
const tauriArgs = ["run", "tauri", "--", "build"];
if (edition === "demo") tauriArgs.push("--config", "src-tauri/tauri.demo.conf.json");
tauriArgs.push("--bundles", bundles);
run("npm", tauriArgs, { env });

// Keep a portable integrity record adjacent to every native installer. This
// makes a locally built candidate as inspectable as the CI artifact and keeps
// the publishing workflow from relying on a separate, easy-to-forget step.
const cargoTarget = resolve(env.CARGO_TARGET_DIR || resolve(repository, "target"));
const bundleDirectory = resolve(cargoTarget, "release", "bundle");
run("node", [resolve(repository, "scripts/write-sha256-manifest.mjs"), bundleDirectory], { env });
