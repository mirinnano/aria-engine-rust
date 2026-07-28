import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { dirname, relative, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const uiRoot = resolve(here, "..");
const gameRoot = resolve(uiRoot, "..");
const repository = resolve(gameRoot, "../..");
const editionArgument = process.argv.indexOf("--edition");
const edition = editionArgument >= 0
  ? process.argv[editionArgument + 1]
  : (process.env.ARIA_UMIKAZE_EDITION || "full");

if (!['full', 'demo'].includes(edition)) {
  throw new Error("edition must be 'full' or 'demo'");
}

const outputArgument = process.argv.indexOf("--out");
if (outputArgument >= 0 && !process.argv[outputArgument + 1]) {
  throw new Error("--out requires a directory");
}

// A full game and its demo are distinct distributable products.  Keep their
// generated Web bundles side by side instead of making local testing or a
// Tauri build silently replace the other edition's output.
const output = outputArgument >= 0
  ? resolve(process.argv[outputArgument + 1])
  : resolve(gameRoot, "dist", "build", edition, "web");
const runtime = resolve(repository, "target/aria-web-runtime-tauri");
const runtimeStamp = resolve(repository, "target/aria-web-runtime-tauri.fingerprint");
const presentationCache = resolve(repository, "target", "aria-presentation-tauri", edition);
const presentationStamp = resolve(repository, "target", `aria-presentation-tauri-${edition}.fingerprint`);
const release = process.env.ARIA_RELEASE === "true";
const profile = process.env.ARIA_PAK_PROFILE || (release ? "signed" : "dev");
const force = process.env.ARIA_FORCE_REBUILD === "true";

// npm does not necessarily retain ~/.cargo/bin on PATH. Prefer the user's
// rustup proxies so this script honors rust-toolchain.toml and can find the
// matching WASM target instead of accidentally invoking a system Cargo.
function rustTool(name, configured = undefined) {
  if (configured) return configured;
  const candidate = resolve(process.env.HOME ?? "", ".cargo", "bin", name);
  return existsSync(candidate) ? candidate : name;
}

const cargo = rustTool("cargo", process.env.CARGO);
const wasmBindgen = rustTool("wasm-bindgen");
// Tauri may set CARGO_TARGET_DIR to keep full and demo desktop bundles apart.
// The preparatory Aria/Web compiler has deliberate, repository-relative cache
// paths below `target/`; do not let the desktop shell's final-binary directory
// redirect those intermediate artifacts and make its own expected paths lie.
const ariaCargoEnvironment = { ...process.env };
delete ariaCargoEnvironment.CARGO_TARGET_DIR;

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: repository, stdio: "inherit", ...options });
  if (result.status !== 0) process.exit(result.status || 1);
}

function sourceFiles(root, relativeRoot = "") {
  const absolute = resolve(root, relativeRoot);
  if (!existsSync(absolute)) return [];
  const stat = statSync(absolute);
  if (stat.isFile()) return [absolute];
  const files = [];
  for (const entry of readdirSync(absolute, { withFileTypes: true })) {
    if ([
      "node_modules",
      "dist",
      "target",
      ".git",
      ".aria-presentation",
      "test-results",
      "playwright-report",
      ".vite",
    ].includes(entry.name)) continue;
    const child = relativeRoot ? `${relativeRoot}/${entry.name}` : entry.name;
    if (entry.isDirectory()) files.push(...sourceFiles(root, child));
    else if (entry.isFile()) files.push(resolve(root, child));
  }
  return files;
}

function fingerprint(label, roots) {
  const hash = createHash("sha256").update(label);
  const files = roots.flatMap(([root, relativeRoot]) => sourceFiles(root, relativeRoot));
  files.sort();
  for (const file of files) {
    hash.update(relative(repository, file));
    hash.update(readFileSync(file));
  }
  return hash.digest("hex");
}

function stampMatches(path, value) {
  return !force && existsSync(path) && readFileSync(path, "utf8").trim() === value;
}

function ensureFrontend() {
  const publicKeyId = process.env.ARIA_PAK_VERIFICATION_KEY_ID || "";
  const publicKeyHex = process.env.ARIA_PAK_VERIFICATION_KEY_HEX || "";
  const value = fingerprint(`umikaze-presentation:${edition}:${process.env.ARIA_PRESENTATION_SOURCEMAP === "true"}:${publicKeyId}:${publicKeyHex}`, [
    [uiRoot, "src"],
    [uiRoot, "public"],
    [uiRoot, "index.html"],
    [uiRoot, "package.json"],
    [uiRoot, "package-lock.json"],
    [repository, "ui/packages/aria-ui-sdk"],
  ]);
  if (stampMatches(presentationStamp, value) && existsSync(resolve(presentationCache, "index.html"))) {
    console.log(`  Reusing presentation cache: ${presentationCache}`);
    return;
  }
  rmSync(presentationCache, { recursive: true, force: true });
  mkdirSync(presentationCache, { recursive: true });
  console.log("  Building presentation (cache miss)...");
  run("npm", ["run", "build"], {
    cwd: uiRoot,
    env: {
      ...process.env,
      ARIA_PRESENTATION_OUT_DIR: presentationCache,
      VITE_ARIA_PAK_VERIFICATION_KEY_ID: publicKeyId,
      VITE_ARIA_PAK_VERIFICATION_KEY_HEX: publicKeyHex,
      VITE_UMIKAZE_EDITION: edition,
    },
  });
  if (!existsSync(resolve(presentationCache, "index.html"))) {
    throw new Error(`presentation build did not produce ${presentationCache}/index.html`);
  }
  writeFileSync(presentationStamp, `${value}\n`);
}

// A demo build is a publishing boundary, not merely a title-screen label.
// Keep a small, independent assertion here because `prepare:demo` is the
// common path for Pages, WebView previewing, and the signed release package.
// This catches an accidental static import of an unreleased chapter before it
// can become a recoverable string or image URL in the browser artifact.
function assertDemoPresentationBoundary() {
  if (edition !== "demo") return;
  const assets = resolve(presentationCache, "assets");
  const files = sourceFiles(presentationCache);
  const contains = (text) => files.some((file) => readFileSync(file).includes(Buffer.from(text)));
  const forbiddenText = [
    "DAY 5",
    "DAY 6",
    "DAY 7",
    "DAY 8",
    "DAY 9",
    "DAY 10",
    "強い雨が、進む理由を足止めする。",
    "終点を知らない列車",
  ];
  // Treat the demo as an allowlisted product. A blacklist eventually misses
  // a newly added late-game photograph; this list makes a new visual an
  // explicit publishing decision before it can enter the public archive.
  const allowedAssetPrefixes = [
    "coast-road-dawn-v1-",
    "hospital-corridor-overcast-v1-",
    "rain-window-dusk-v1-",
    "train-window-summer-v1-",
    "train-motion-summer-v1-",
    "station-night-pass-v1-",
    "rail-window-sunset-v1-",
    "shore-storm-sunset-v1-",
    "platform-sea-dawn-v1-",
    "hotel-corridor-blue-v1-",
  ];
  for (const text of forbiddenText) {
    if (contains(text)) throw new Error(`demo presentation leaks later chapter text: ${text}`);
  }
  if (!contains("9月24日・松江")) {
    throw new Error("demo presentation omitted the final playable chapter preview");
  }
  const assetNames = existsSync(assets) ? readdirSync(assets) : [];
  for (const name of assetNames.filter((name) => /\.(?:avif|jpe?g|png|webp)$/i.test(name))) {
    if (!allowedAssetPrefixes.some((prefix) => name.startsWith(prefix))) {
      throw new Error(`demo presentation contains an unapproved scene asset: ${name}`);
    }
  }
}

// The view-model schema is compiled into this WASM module. A content
// fingerprint lets local Tauri launches stay fast while still invalidating
// the cache when the VM, renderer, toolchain, or UI changes.
mkdirSync(runtime, { recursive: true });
const runtimeFingerprint = fingerprint("umikaze-web-runtime", [
  [repository, "Cargo.toml"],
  [repository, "Cargo.lock"],
  [repository, "rust-toolchain.toml"],
  [repository, "crates/aria-core/src"],
  [repository, "crates/aria-protection/src"],
  [repository, "crates/aria-render/src"],
  [repository, "crates/aria-web/src"],
]);
const runtimeReady = existsSync(resolve(runtime, "aria_web.js")) && existsSync(resolve(runtime, "aria_web_bg.wasm"));
if (stampMatches(runtimeStamp, runtimeFingerprint) && runtimeReady) {
  console.log(`  Reusing WASM runtime cache: ${runtime}`);
} else {
  rmSync(runtime, { recursive: true, force: true });
  mkdirSync(runtime, { recursive: true });
  run(cargo, ["build", "--release", "--locked", "-p", "aria-web", "--target", "wasm32-unknown-unknown"], {
    env: ariaCargoEnvironment,
  });
  run(wasmBindgen, [
    "--target", "web",
    "--out-dir", runtime,
    "--out-name", "aria_web",
    resolve(repository, "target/wasm32-unknown-unknown/release/aria_web.wasm"),
  ]);
  writeFileSync(runtimeStamp, `${runtimeFingerprint}\n`);
}

ensureFrontend();
assertDemoPresentationBoundary();

console.log(`  Preparing ${edition} Web bundle: ${relative(repository, output)}`);

const buildArgs = [
  // The Tauri shell consumes a Web data bundle, not Aria's standalone native
  // Player. Avoid linking WGPU/audio/windowing just to compile scripts and
  // package a PAK; it makes local iteration and CI materially slower.
  "run", "--release", "--locked", "--no-default-features", "-p", "aria-cli", "--", "build", gameRoot,
  "--target", "web", "--out", output, "--profile", profile,
];
if (edition === "demo") {
  // Keep the full scenario modules out of the compiled import closure and
  // save independently from the commercial edition. The manifest on disk is
  // never modified by a build invocation.
  buildArgs.push("--entry", "scripts/main-demo.aria", "--save-namespace", "umikaze-demo-v1");
}
if (release) buildArgs.push("--release");
run(cargo, buildArgs, {
  env: {
    ...ariaCargoEnvironment,
    ARIA_WEB_RUNTIME_DIR: runtime,
    ARIA_PRESENTATION_PREBUILT_DIR: presentationCache,
  },
});

// Font licenses are part of every distributable copy, not merely source-tree
// documentation.  Keeping them next to the staged Web payload means the
// static archive exposes them at /licenses/ and the Tauri bundle can carry
// the same directory as a native resource.
const licenseSource = resolve(gameRoot, "licenses");
const licenseOutput = resolve(output, "licenses");
if (!existsSync(licenseSource)) {
  throw new Error(`missing required distribution licenses: ${licenseSource}`);
}
rmSync(licenseOutput, { recursive: true, force: true });
cpSync(licenseSource, licenseOutput, { recursive: true, dereference: false });
for (const required of ["NotoSansCJKJP-OFL.txt", "MPLUS1Code-OFL.txt", "ShipporiMincho-OFL.txt"]) {
  if (!existsSync(resolve(licenseOutput, required))) {
    throw new Error(`distribution license was not staged: ${required}`);
  }
}
