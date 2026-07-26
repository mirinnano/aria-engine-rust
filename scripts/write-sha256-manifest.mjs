import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, relative, resolve } from "node:path";

const root = resolve(process.argv[2] || ".");
const outputName = process.argv[3] || "release-manifest.json";
if (!statSync(root).isDirectory()) throw new Error(`not a directory: ${root}`);

function filesUnder(directory) {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) files.push(...filesUnder(path));
    else if (entry.isFile() && entry.name !== outputName && entry.name !== "checksums.sha256") files.push(path);
  }
  return files;
}

const files = filesUnder(root).sort();
const entries = files.map((path) => {
  const bytes = readFileSync(path);
  return {
    name: relative(root, path).replaceAll("\\", "/"),
    size: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
});
const manifestPath = resolve(root, outputName);
writeFileSync(manifestPath, `${JSON.stringify({ schema_version: 1, root: basename(root), artifacts: entries }, null, 2)}\n`);
writeFileSync(resolve(root, "checksums.sha256"), `${entries.map((entry) => `${entry.sha256}  ${entry.name}`).join("\n")}\n`);
console.log(`wrote ${manifestPath} (${entries.length} files)`);
