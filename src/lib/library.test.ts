import { describe, expect, it } from "vitest";
import { cleanRoots, defaultRootIndex, setupRoots } from "./library";

describe("library setup", () => {
  it("prefills a usable Windows path instead of a placeholder", () => {
    expect(setupRoots([], "windows")).toEqual([{ path: "C:\\LAN", label: "C:\\LAN", isDefault: true }]);
    expect(setupRoots([], "linux")[0].path).toBe("");
  });
  it("marks the effective default in legacy settings without mutating them", () => {
    const roots = [{ path: "D:\\Games", label: "SSD", isDefault: false }];
    expect(defaultRootIndex(roots)).toBe(0);
    expect(setupRoots(roots, "windows")[0].isDefault).toBe(true);
    expect(roots[0].isDefault).toBe(false);
    expect(defaultRootIndex([])).toBe(-1);
  });
  it("keeps multiple paths, trims them and deduplicates Windows aliases", () => {
    const roots = cleanRoots([
      { path: " C:\\LAN ", label: "", isDefault: false },
      { path: "c:/lan/", label: "", isDefault: true },
      { path: "D:\\Games", label: "", isDefault: false },
      { path: " ", label: "", isDefault: false },
    ], "windows");
    expect(roots.map((r) => r.path)).toEqual(["C:\\LAN", "D:\\Games"]);
    expect(roots.map((r) => r.isDefault)).toEqual([true, false]);
  });
  it("respects the chosen default and falls back if an empty row is removed", () => {
    const roots = [{ path: "C:\\LAN", label: "", isDefault: false }, { path: "D:\\LAN", label: "", isDefault: true }];
    expect(defaultRootIndex(setupRoots(roots, "windows"))).toBe(1);
    expect(defaultRootIndex(cleanRoots([{ ...roots[1], path: "" }, roots[0]], "windows"))).toBe(0);
  });
});
