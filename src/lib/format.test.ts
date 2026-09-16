import { describe, expect, it } from "vitest";
import { formatBytes, formatPercent, formatRevision, percentWidth, placeholderGradient, setByteUnits, stripHtml } from "./format";

describe("format helpers", () => {
  it("keeps the CSS width free of the space the label needs", () => {
    // "23 %" is not a CSS length: the declaration is dropped and the bar
    // renders full, whatever the number next to it says.
    expect(formatPercent(0.234)).toBe("23 %");
    expect(percentWidth(0.234)).toBe("23%");
    expect(percentWidth(0)).toBe("0%");
    expect(percentWidth(1.5)).toBe("100%");
    expect(percentWidth(Number.NaN)).toBe("0%");
  });

  it("formats bytes with decimal units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(910_000_000)).toBe("910 MB");
    expect(formatBytes(16_000_000_000)).toBe("16 GB");
    expect(formatBytes(1_234_567)).toBe("1.2 MB");
  });
  it("uses Explorer's binary units on Windows", () => {
    try {
      setByteUnits("windows");
      // 999.2 decimal GB is a disk Explorer lists as roughly 930 GB
      expect(formatBytes(999_200_000_000)).toBe("930.6 GB");
      expect(formatBytes(16_000_000_000)).toBe("14.9 GB");
      setByteUnits("macos");
      expect(formatBytes(16_000_000_000)).toBe("16 GB");
    } finally {
      setByteUnits("linux");
    }
  });
  it("clamps percentages", () => {
    expect(formatPercent(0.5)).toBe("50 %");
    expect(formatPercent(1.7)).toBe("100 %");
    expect(formatPercent(-1)).toBe("0 %");
  });
  it("formats package revisions per language", () => {
    expect(formatRevision("20250308", "de")).toBe("08.03.2025");
    expect(formatRevision("20250308", "en")).toBe("2025-03-08");
    expect(formatRevision("v2", "de")).toBe("v2");
  });
  it("produces stable gradients", () => {
    expect(placeholderGradient("quake3")).toBe(placeholderGradient("quake3"));
    expect(placeholderGradient("quake3")).not.toBe(placeholderGradient("amongus"));
  });
  it("strips readme html", () => {
    expect(stripHtml("Hallo<br>Welt &amp; <b>du</b>")).toBe("Hallo\nWelt & du");
  });
});
