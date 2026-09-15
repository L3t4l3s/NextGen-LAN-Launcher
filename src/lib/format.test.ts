import { describe, expect, it } from "vitest";
import { formatBytes, formatPercent, formatRevision, placeholderGradient, stripHtml } from "./format";

describe("format helpers", () => {
  it("formats bytes with decimal units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(910_000_000)).toBe("910 MB");
    expect(formatBytes(16_000_000_000)).toBe("16 GB");
    expect(formatBytes(1_234_567)).toBe("1.2 MB");
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
