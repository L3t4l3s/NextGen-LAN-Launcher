// Raises the launcher's version everywhere it is written down, and turns the
// changelog's "Unreleased" section into the new version's section.
//
//   node tools/bump-version.mjs minor ["Title"]   # new feature: 0.3.4 → 0.4.0
//   node tools/bump-version.mjs patch ["Title"]   # fixes, profiles: 0.3.0 → 0.3.1
//   node tools/bump-version.mjs 1.0.0 ["Title"]   # an explicit version
//
// The major number stays 0 until the launcher is called finished; that step
// is taken by hand with an explicit version. `src/lib/version.test.ts` fails
// when the files disagree, so a hand edit that misses one shows in CI.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const file = (p) => join(root, p);
// Every change is made in memory first and written only once all of them
// matched: a pattern that misses one file must not leave the others bumped.
const edited = new Map();
const read = (p) => edited.get(p) ?? readFileSync(file(p), "utf8");
const write = (p, s) => edited.set(p, s);

/** The crates whose version follows the workspace (`version.workspace = true`). */
const crates = ["lanlauncher-core", "nextgen-lan-launcher"];

const [kind, title] = process.argv.slice(2);
const current = JSON.parse(read("package.json")).version;
const next = bump(current, kind);

function bump(v, how) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(v);
  if (!m) throw new Error(`package.json has no plain x.y.z version: ${v}`);
  const [maj, min, pat] = m.slice(1).map(Number);
  if (how === "minor") return `${maj}.${min + 1}.0`;
  if (how === "patch") return `${maj}.${min}.${pat + 1}`;
  if (/^\d+\.\d+\.\d+$/.test(how ?? "")) return how;
  console.error("usage: node tools/bump-version.mjs minor|patch|x.y.z [title]");
  process.exit(2);
}

function replaceOnce(path, pattern, replacement) {
  const before = read(path);
  const after = before.replace(pattern, replacement);
  if (after === before) throw new Error(`${path}: version not found (${pattern})`);
  write(path, after);
}

replaceOnce("package.json", /("version":\s*")[^"]+(")/, `$1${next}$2`);
// The lockfile names the package's version twice: at the top and under "".
replaceOnce("package-lock.json", /^(\{\s*"name":\s*"[^"]+",\s*"version":\s*")[^"]+(")/, `$1${next}$2`);
replaceOnce("package-lock.json", /("packages":\s*\{\s*"":\s*\{\s*"name":\s*"[^"]+",\s*"version":\s*")[^"]+(")/, `$1${next}$2`);
replaceOnce("src-tauri/tauri.conf.json", /("version":\s*")[^"]+(")/, `$1${next}$2`);
replaceOnce("Cargo.toml", /(\[workspace\.package\]\s*\r?\nversion = ")[^"]+(")/, `$1${next}$2`);
// Cargo would rewrite these on the next build; doing it here keeps the
// commit whole and `cargo build --locked` happy.
for (const name of crates) {
  replaceOnce("Cargo.lock", new RegExp(`(name = "${name}"\\r?\\nversion = ")[^"]+(")`), `$1${next}$2`);
}

const changelog = read("CHANGELOG.md");
const heading = `## ${next}${title ? ` – ${title}` : ""}`;
if (/\n## Unreleased\r?\n/.test(changelog)) {
  write("CHANGELOG.md", changelog.replace(/\n## Unreleased(\r?\n)/, `\n## Unreleased$1$1${heading}$1`));
} else {
  console.warn("CHANGELOG.md has no '## Unreleased' section; add the heading by hand.");
}

for (const [p, s] of edited) writeFileSync(file(p), s);
console.log(`${current} → ${next}`);
