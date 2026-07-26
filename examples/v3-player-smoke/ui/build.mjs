// This fixture deliberately uses the checked-in engine Web presentation.  It
// has no package dependencies; `aria build` supplies the matching runtime,
// package reader, and save adapter after this template is staged.
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const output = process.env.ARIA_PRESENTATION_OUT_DIR;
const template = resolve(here, "../../../crates/aria-web/pwa");

if (!output) {
  throw new Error("ARIA_PRESENTATION_OUT_DIR must be set by aria build");
}
if (!existsSync(template)) {
  throw new Error(`missing checked-in Web presentation template: ${template}`);
}

rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
cpSync(template, output, { recursive: true, dereference: false });
