import { describe, expect, it } from "vitest";
import { de } from "./de";
import { en } from "./en";

describe("i18n", () => {
  it("has the same keys in both languages", () => {
    // A key missing on one side shows its raw id in the UI, which is only
    // ever noticed on the language nobody tests with.
    expect(Object.keys(en).sort()).toEqual(Object.keys(de).sort());
  });

  it("has no empty texts", () => {
    for (const [lang, msgs] of [
      ["de", de],
      ["en", en],
    ] as const) {
      for (const [key, text] of Object.entries(msgs)) {
        expect(text.trim(), `${lang}: ${key}`).not.toBe("");
      }
    }
  });
});
