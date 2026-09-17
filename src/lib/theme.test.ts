import { describe, expect, it } from "vitest";
import { builtinThemes, readableOn, surfacePatternCss } from "./theme";

/**
 * Distance in OKLab, which is perceptual: two oranges that differ by a lot of
 * sRGB still look the same, and that is exactly the mistake to catch here.
 */
function distance(a: string, b: string): number {
  const channel = (hex: string, i: number) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const oklab = (hex: string) => {
    const [r, g, b] = [1, 3, 5].map((i) => channel(hex, i));
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    return [
      0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
      1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
      0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
    ];
  };
  const [x, y] = [oklab(a), oklab(b)];
  return Math.hypot(x[0] - y[0], x[1] - y[1], x[2] - y[2]);
}

// The five colours a badge or a problem card can carry. Below this they read
// as the same colour at the 22 % tint the badges use — roughly seven times
// the threshold at which a difference is visible at all.
const STATUS = ["primary", "accent", "success", "warning", "danger"] as const;
const MIN_DISTANCE = 0.15;

/** WCAG contrast ratio, for the badge text against its card. */
function contrast(a: string, b: string): number {
  const relative = (hex: string) =>
    [1, 3, 5]
      .map((i) => {
        const c = parseInt(hex.slice(i, i + 2), 16) / 255;
        return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
      })
      .reduce((sum, c, i) => sum + c * [0.2126, 0.7152, 0.0722][i], 0);
  const [light, dark] = [relative(a), relative(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}

describe("built-in themes", () => {
  it("keeps the status colours apart", () => {
    // Badges carry their meaning in the colour alone: a scheme whose primary
    // is also its danger makes "running" and "failed" look the same.
    for (const [id, theme] of Object.entries(builtinThemes)) {
      for (let i = 0; i < STATUS.length; i++) {
        for (let j = i + 1; j < STATUS.length; j++) {
          const [a, b] = [theme.colors[STATUS[i]], theme.colors[STATUS[j]]];
          expect(distance(a, b), `${id}: ${STATUS[i]} ${a} vs ${STATUS[j]} ${b}`).toBeGreaterThanOrEqual(MIN_DISTANCE);
        }
      }
    }
  });

  it("keeps the status colours readable on the surface they sit on", () => {
    // A badge paints its colour as text over a tint of itself on the card.
    // Separation alone is not enough: a dark crimson on a dark card is
    // unmistakable and unreadable at the same time.
    for (const [id, theme] of Object.entries(builtinThemes)) {
      for (const token of STATUS) {
        const ratio = contrast(theme.colors[token], theme.colors.surface);
        expect(ratio, `${id}.${token} ${theme.colors[token]} on ${theme.colors.surface}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("keeps text on a coloured surface legible whichever way the colour goes", () => {
    for (const [id, theme] of Object.entries(builtinThemes)) {
      // The UI derives the ink for these two (Play button, completed wizard
      // step, the warning marker on a cover).
      for (const token of ["success", "warning"] as const) {
        const ratio = contrast(theme.colors[token], readableOn(theme.colors[token]));
        expect(ratio, `${id}: text on ${token} ${theme.colors[token]}`).toBeGreaterThanOrEqual(4.5);
      }
      // The primary button brings its own text colour from the theme, and it
      // is large and bold: the AA bar for that is 3:1.
      const primary = contrast(theme.colors.primary, theme.colors.primaryText);
      expect(primary, `${id}: primaryText on primary`).toBeGreaterThanOrEqual(3);
    }
  });

  it("picks the ink by contrast, not by a guessed cut-off", () => {
    // A mid-tone green: the dark ink wins here by a wide margin.
    expect(readableOn("#1f9d5a")).toBe("#08140c");
    expect(readableOn("#0f1218")).toBe("#f2f4f8");
    // Other notations the colour validator accepts.
    expect(readableOn("rgb(255, 255, 255)")).toBe("#08140c");
    expect(readableOn("#fff")).toBe("#08140c");
    // Percentages and alpha in rgb() still resolve without a document.
    expect(readableOn("rgba(10, 10, 10, 0.5)")).toBe("#f2f4f8");
    expect(readableOn("nonsense")).toBe("#08140c");
  });

  it("defines every colour token as a hex value", () => {
    for (const [id, theme] of Object.entries(builtinThemes)) {
      for (const [token, value] of Object.entries(theme.colors)) {
        expect(value, `${id}.${token}`).toMatch(/^#[0-9a-f]{6}$/i);
      }
    }
  });

  // The figures are built in two places — here and in
  // crates/lanlauncher-core/src/theme.rs, whose tests assert the same strings.
  // This side is the one that paints, so it is the one that must not let a
  // value through that could end the declaration it stands in.
  it("draws the figure a theme asks for on its surfaces", () => {
    expect(surfacePatternCss({ kind: "scanlines", color: "rgba(0, 212, 255, 0.06)", size: 4 })).toEqual({
      image:
        "repeating-linear-gradient(0deg, rgba(0, 212, 255, 0.06) 0, rgba(0, 212, 255, 0.06) 1px, transparent 1px, transparent 4px)",
      size: "auto",
    });
    // A dot carries its distance on the box, not in the figure.
    expect(surfacePatternCss({ kind: "dots", size: 6 })?.size).toBe("6px 6px");
    expect(surfacePatternCss({ kind: "grid" })?.image.match(/repeating-linear/g)).toHaveLength(2);
    expect(surfacePatternCss({ kind: "diagonal", angle: 135 })?.image).toContain("(135deg,");
    expect(surfacePatternCss({ kind: "gradient", angle: 135 })?.image).toContain("linear-gradient(135deg,");
  });

  it("leaves the surface flat where it cannot draw", () => {
    expect(surfacePatternCss(null)).toBeNull();
    expect(surfacePatternCss({ kind: "none" })).toBeNull();
    // A figure only a newer launcher knows.
    expect(surfacePatternCss({ kind: "hexagons" as never })).toBeNull();
    // A colour that could close the declaration and open a rule of its own.
    expect(surfacePatternCss({ kind: "scanlines", color: "red; } body { display: none" })).toBeNull();
  });

  it("keeps the numbers of a figure inside what is still a figure", () => {
    // Under 2px scanlines are a solid block, over 64 there is one line per
    // card; the angle wraps rather than reaching CSS as -45 or 900.
    expect(surfacePatternCss({ kind: "scanlines", size: 0 })?.image).toContain("transparent 2px");
    expect(surfacePatternCss({ kind: "scanlines", size: 4000 })?.image).toContain("transparent 64px");
    expect(surfacePatternCss({ kind: "diagonal", angle: 405 })?.image).toContain("(45deg,");
    expect(surfacePatternCss({ kind: "diagonal", angle: -45 })?.image).toContain("(315deg,");
    expect(surfacePatternCss({ kind: "scanlines", size: Number.NaN })?.image).toContain("transparent 4px");
  });

  it("gives every theme a name and a radius the UI can use", () => {
    for (const [id, theme] of Object.entries(builtinThemes)) {
      expect(theme.name.trim(), id).not.toBe("");
      expect(theme.radius, id).toBeLessThanOrEqual(48);
    }
  });
});
