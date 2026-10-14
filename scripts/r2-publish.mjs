#!/usr/bin/env node
// Describes private game builds for the ponkotsu-soft download page.
//
// The full game is never attached to a public GitHub Release or a workflow
// artifact (this repository is public). Each desktop job uploads its
// installer to R2 itself and writes a small per-platform fragment; the final
// job merges fragments into `<work>/latest.json`, which the site's
// access-code endpoint reads to offer one signed link per platform.
//
//   node scripts/r2-publish.mjs fragment <bundle-dir> <platform> <key-prefix> <out.json>
//   node scripts/r2-publish.mjs manifest <fragments-dir> <version> <commit> <out.json>

import { createHash } from "node:crypto";
import { createReadStream, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";

const INSTALLERS = {
  windows: { label: "Windows", pattern: /-setup\.exe$/i },
  macos: { label: "macOS", pattern: /\.dmg$/i },
  linux: { label: "Linux (AppImage)", pattern: /\.AppImage$/ },
};

function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

function sha256(path) {
  return new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    createReadStream(path)
      .on("data", (chunk) => hash.update(chunk))
      .on("error", reject)
      .on("end", () => resolve(hash.digest("hex")));
  });
}

async function fragment([bundleDir, platform, keyPrefix, out]) {
  const installer = INSTALLERS[platform];
  if (!installer) throw new Error(`unknown platform '${platform}'`);
  const files = walk(bundleDir).filter((path) => installer.pattern.test(basename(path)));
  if (files.length !== 1) {
    throw new Error(`expected one ${platform} installer in ${bundleDir}, found ${files.length}`);
  }
  const [path] = files;
  const name = basename(path);
  const entry = {
    platform,
    label: installer.label,
    name,
    key: `${keyPrefix}/${platform}/${name}`,
    size: statSync(path).size,
    sha256: await sha256(path),
    local: path,
  };
  writeFileSync(out, `${JSON.stringify(entry, null, 2)}\n`);
  console.log(`${entry.label}: ${name} (${entry.size} bytes) -> ${entry.key}`);
}

function manifest([fragmentsDir, version, commit, out]) {
  const order = Object.keys(INSTALLERS);
  const files = readdirSync(fragmentsDir)
    .filter((name) => name.endsWith(".json"))
    .map((name) => JSON.parse(readFileSync(join(fragmentsDir, name), "utf8")))
    .map(({ local: _local, ...entry }) => entry)
    .sort((left, right) => order.indexOf(left.platform) - order.indexOf(right.platform));
  if (!files.length) throw new Error(`no platform fragments in ${fragmentsDir}`);
  const document = { version, commit, built_at: new Date().toISOString(), files };
  writeFileSync(out, `${JSON.stringify(document, null, 2)}\n`);
  console.log(`manifest ${version}: ${files.map((file) => file.platform).join(", ")}`);
}

const [command, ...args] = process.argv.slice(2);
if (command === "fragment") await fragment(args);
else if (command === "manifest") manifest(args);
else {
  console.error("usage: r2-publish.mjs fragment|manifest …");
  process.exit(2);
}
