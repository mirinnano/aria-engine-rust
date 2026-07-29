import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = resolve(fileURLToPath(new URL(".", import.meta.url)));
const uiRoot = resolve(here, "..");
const gameRoot = resolve(uiRoot, "..");
const repository = resolve(gameRoot, "../..");

function option(name, fallback = undefined) {
  const index = process.argv.indexOf(name);
  if (index < 0) return fallback;
  const value = process.argv[index + 1];
  if (!value || value.startsWith("--")) throw new Error(`${name} requires a value`);
  return value;
}

const edition = option("--edition", "full");
if (!["full", "demo"].includes(edition)) {
  throw new Error("--edition must be 'full' or 'demo'");
}

const profile = option("--profile", process.env.ARIA_PAK_PROFILE || "signed");
const releaseDirectory = resolve(option(
  "--dir",
  resolve(repository, "dist", "releases", edition === "demo" ? "demo-web" : "web"),
));

function fail(message) {
  throw new Error(`Invalid ${edition} Web release: ${message}`);
}

function safeRelativePath(value, label) {
  if (typeof value !== "string" || !value || value.startsWith("/") || value.startsWith("\\")) {
    fail(`${label} is not a relative path`);
  }
  const normalized = relative(releaseDirectory, resolve(releaseDirectory, value));
  if (normalized === "" || normalized === ".." || normalized.startsWith(`..${"/"}`) || normalized.startsWith(`..${"\\"}`)) {
    fail(`${label} escapes the release directory`);
  }
  return resolve(releaseDirectory, value);
}

function requiredFile(path, label = path) {
  if (!existsSync(path) || !statSync(path).isFile()) fail(`missing file ${label}`);
  return path;
}

function safeSitePath(value, label) {
  if (typeof value !== "string" || !value || value.startsWith("/") || value.startsWith("\\")) {
    fail(`${label} is not a relative site path`);
  }
  const path = resolve(site, value);
  const normalized = relative(site, path);
  if (normalized === "" || normalized === ".." || normalized.startsWith(`..${"/"}`) || normalized.startsWith(`..${"\\"}`)) {
    fail(`${label} escapes the static site`);
  }
  return path;
}

function readJson(path, label = path) {
  try {
    return JSON.parse(readFileSync(requiredFile(path, label), "utf8"));
  } catch (error) {
    fail(`${label} is not valid JSON (${error instanceof Error ? error.message : String(error)})`);
  }
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

const manifestPath = requiredFile(resolve(releaseDirectory, "release-manifest.json"));
const checksumsPath = requiredFile(resolve(releaseDirectory, "checksums.sha256"));
const contractPath = requiredFile(resolve(releaseDirectory, "web-release.json"));
const site = resolve(releaseDirectory, "site");
if (!existsSync(site) || !statSync(site).isDirectory()) fail("missing static site directory");

const manifest = readJson(manifestPath, "release-manifest.json");
if (manifest.schema_version !== 1) fail(`unsupported release manifest schema ${manifest.schema_version}`);
if (manifest.target !== "web") fail(`expected target web, received ${manifest.target}`);
if (manifest.game_id !== "jp.example.umikaze") fail(`unexpected game id ${manifest.game_id}`);
if (manifest.pak_profile !== profile) fail(`expected ${profile} PAK, received ${manifest.pak_profile}`);
if (!Array.isArray(manifest.artifacts) || manifest.artifacts.length === 0) fail("release manifest has no artifacts");

const manifestArtifacts = new Map();
for (const artifact of manifest.artifacts) {
  if (!artifact || typeof artifact !== "object") fail("release manifest has an invalid artifact");
  const path = safeRelativePath(artifact.name, "artifact name");
  if (manifestArtifacts.has(artifact.name)) fail(`release manifest repeats ${artifact.name}`);
  if (!Number.isInteger(artifact.size) || artifact.size < 0) fail(`artifact size is invalid for ${artifact.name}`);
  if (typeof artifact.sha256 !== "string" || !/^[a-f0-9]{64}$/i.test(artifact.sha256)) {
    fail(`artifact SHA-256 is invalid for ${artifact.name}`);
  }
  const metadata = statSync(requiredFile(path, artifact.name));
  if (metadata.size !== artifact.size) fail(`artifact size drifted for ${artifact.name}`);
  if (sha256(path) !== artifact.sha256) fail(`artifact checksum drifted for ${artifact.name}`);
  manifestArtifacts.set(artifact.name, artifact.sha256);
}

for (const requiredArtifact of ["_headers", "web-release.json"]) {
  if (!manifestArtifacts.has(requiredArtifact)) fail(`release manifest omits ${requiredArtifact}`);
}
if (![...manifestArtifacts.keys()].some((name) => name.endsWith("-web.zip"))) {
  fail("release manifest omits the portable Web archive");
}

const checksumArtifacts = new Map();
for (const line of readFileSync(checksumsPath, "utf8").trim().split("\n").filter(Boolean)) {
  const match = /^([a-f0-9]{64})  (.+)$/i.exec(line);
  if (!match) fail(`malformed checksum line ${JSON.stringify(line)}`);
  const [, digest, name] = match;
  safeRelativePath(name, "checksum artifact name");
  if (checksumArtifacts.has(name)) fail(`checksums repeat ${name}`);
  checksumArtifacts.set(name, digest.toLowerCase());
}
if (checksumArtifacts.size !== manifestArtifacts.size) fail("checksums and release manifest disagree on artifact count");
for (const [name, digest] of manifestArtifacts) {
  if (checksumArtifacts.get(name) !== digest) fail(`checksums and release manifest disagree on ${name}`);
}

const contract = readJson(contractPath, "web-release.json");
if (contract.schema_version !== 1 || contract.mode !== "static-pwa") fail("invalid static Web contract");
if (contract.game_id !== manifest.game_id || contract.game_version !== manifest.game_version) {
  fail("static Web contract does not match the release manifest");
}
if (!Array.isArray(contract.entrypoints) || !Array.isArray(contract.immutable_files)) {
  fail("static Web contract is missing its file lists");
}
for (const file of [...contract.entrypoints, ...contract.immutable_files]) {
  if (typeof file !== "string" || !file) fail("static Web contract contains an invalid path");
  const filePath = safeSitePath(file, "static Web contract path");
  requiredFile(filePath, `site/${file}`);
}
const copiedContractPath = requiredFile(resolve(site, "web-release.json"), "site/web-release.json");
if (!readFileSync(contractPath).equals(readFileSync(copiedContractPath))) {
  fail("site/web-release.json differs from the published contract");
}

const bundle = readJson(resolve(site, "bundle.aria.json"), "site/bundle.aria.json");
if (bundle.game_id !== manifest.game_id || bundle.game_version !== manifest.game_version) {
  fail("bundle metadata does not match the release manifest");
}
if (bundle.pak_profile !== profile) fail(`bundle has ${bundle.pak_profile} PAK profile, expected ${profile}`);
if (bundle.content_root_blake3 !== manifest.bundle_content_root_blake3) {
  fail("bundle content root does not match the release manifest");
}
const expectedSaveNamespace = edition === "demo" ? "umikaze-demo-v1" : "umikaze-v4";
if (bundle.save_namespace !== expectedSaveNamespace) {
  fail(`expected save namespace ${expectedSaveNamespace}, received ${bundle.save_namespace}`);
}
if (!Array.isArray(bundle.pak_packs) || bundle.pak_packs.length === 0) fail("bundle has no PAK packs");
const packagedAssets = [];
const seenPackagedAssets = new Set();
for (const pack of bundle.pak_packs) {
  if (typeof pack?.file !== "string" || !pack.file.endsWith(".ariapak")) fail("bundle contains an invalid PAK record");
  if (!Array.isArray(pack.assets)) fail(`bundle PAK ${pack.file} has no asset inventory`);
  for (const asset of pack.assets) {
    if (typeof asset !== "string" || !asset) fail(`bundle PAK ${pack.file} contains an invalid asset path`);
    if (seenPackagedAssets.has(asset)) fail(`bundle repeats asset ${asset} across PAKs`);
    seenPackagedAssets.add(asset);
    packagedAssets.push(asset);
  }
  requiredFile(safeSitePath(pack.file, "PAK path"), `site/${pack.file}`);
}
if (edition === "demo") {
  // This is a deliberate content boundary, not merely a runtime lock. Any
  // new opening-arc asset must be reviewed and added here; later-game media
  // must never be recoverable from the public static download.
  const expectedDemoAssets = [
    "assets/audio/bgm/umk.rail.departure.ogg",
    "assets/audio/bgm/umk.recording.trace.ogg",
    "assets/audio/bgm/umk.ward.first-light.ogg",
    "assets/bg/scenes/hospital-corridor-overcast-v1.webp",
    "assets/bg/scenes/hotel-corridor-blue-v1.webp",
    "assets/bg/scenes/neon-alley-v1.webp",
    "assets/bg/scenes/okayama-rail-window-v1.webp",
    "assets/bg/scenes/platform-sea-dawn-v1.webp",
    "assets/bg/scenes/rail-window-sunset-v1.webp",
    "assets/bg/scenes/rain-street-evening-v1.webp",
    "assets/bg/scenes/sannomiya-rain-platform-v1.webp",
    "assets/bg/scenes/shore-storm-sunset-v1.webp",
    "assets/fonts/MPLUS1Code-wght.ttf",
    "assets/fonts/NotoSansJP-Regular.ttf",
  ].sort();
  const actualDemoAssets = [...packagedAssets].sort();
  if (JSON.stringify(actualDemoAssets) !== JSON.stringify(expectedDemoAssets)) {
    const unexpected = actualDemoAssets.filter((asset) => !expectedDemoAssets.includes(asset));
    const missing = expectedDemoAssets.filter((asset) => !actualDemoAssets.includes(asset));
    fail(`demo PAK asset boundary drifted (unexpected: ${unexpected.join(", ") || "none"}; missing: ${missing.join(", ") || "none"})`);
  }
}
requiredFile(resolve(site, "game.ariac"), "site/game.ariac");

console.log(`Verified ${edition} Web release: ${releaseDirectory}`);
