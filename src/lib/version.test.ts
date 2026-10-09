import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// The version is written down in five files; `tools/bump-version.mjs` raises
// them together. This catches a hand edit that missed one.
const read = (p: string) => readFileSync(new URL(`../../${p}`, import.meta.url), "utf8");

describe("version", () => {
  it("is the same in every file that names it", () => {
    const version = JSON.parse(read("package.json")).version as string;
    const lock = JSON.parse(read("package-lock.json"));
    expect(lock.version).toBe(version);
    expect(lock.packages[""].version).toBe(version);
    expect(JSON.parse(read("src-tauri/tauri.conf.json")).version).toBe(version);
    expect(/\[workspace\.package\]\s*\r?\nversion = "([^"]+)"/.exec(read("Cargo.toml"))?.[1]).toBe(version);
    const cargoLock = read("Cargo.lock");
    for (const name of ["lanlauncher-core", "nextgen-lan-launcher"]) {
      expect(new RegExp(`name = "${name}"\\r?\\nversion = "([^"]+)"`).exec(cargoLock)?.[1], name).toBe(version);
    }
  });

  it("has a changelog section", () => {
    const version = JSON.parse(read("package.json")).version as string;
    expect(read("CHANGELOG.md")).toContain(`\n## ${version}`);
  });
});
