import type { FontFace, SurfacePattern, Theme, ThemeColors } from "./types";
import lightJson from "../../themes/light.json";
import blueJson from "../../themes/blue.json";
import greenJson from "../../themes/green.json";
import orangeJson from "../../themes/orange.json";
import pinkJson from "../../themes/pink.json";
import redJson from "../../themes/red.json";

export const defaultTheme: Theme = {
  version: 1,
  name: "NextGen Dark",
  mode: "dark",
  colors: {
    background: "#0f1218",
    surface: "#171c25",
    surfaceAlt: "#1f2632",
    text: "#f2f4f8",
    textMuted: "#9aa4b5",
    primary: "#4f8cff",
    primaryText: "#ffffff",
    accent: "#7ee8ff",
    success: "#2ecc71",
    warning: "#f1c40f",
    danger: "#ff5c5c",
    border: "#2a3140",
  },
  logo: null,
  backgroundImage: null,
  backgroundOverlay: null,
  radius: 12,
  surfacePattern: null,
  fontFamily: null,
  headingFontFamily: null,
  fontFaces: [],
  icons: {},
  legacyCss: null,
};

/** The JSON files under themes/ are the single source; missing optional fields fall back to the default theme. */
function fromJson(json: Partial<Theme>): Theme {
  return { ...defaultTheme, ...json, colors: { ...defaultTheme.colors, ...(json.colors ?? {}) } } as Theme;
}

/** Themes shipped with the launcher, selectable in Settings by id. */
export const builtinThemes: Record<string, Theme> = {
  default: defaultTheme,
  light: fromJson(lightJson as Partial<Theme>),
  blue: fromJson(blueJson as Partial<Theme>),
  green: fromJson(greenJson as Partial<Theme>),
  orange: fromJson(orangeJson as Partial<Theme>),
  pink: fromJson(pinkJson as Partial<Theme>),
  red: fromJson(redJson as Partial<Theme>),
};

/** The colours every theme has; the chrome ones below are optional. */
type BaseColorKey = Exclude<keyof ThemeColors, "header" | "headerText" | "footer" | "footerText">;

const varMap: Record<BaseColorKey, string> = {
  background: "--color-bg",
  surface: "--color-surface",
  surfaceAlt: "--color-surface-alt",
  text: "--color-text",
  textMuted: "--color-text-muted",
  primary: "--color-primary",
  primaryText: "--color-primary-text",
  accent: "--color-accent",
  success: "--color-success",
  warning: "--color-warning",
  danger: "--color-danger",
  border: "--color-border",
};

// Case-insensitive like the Rust check, which lowercases before testing; a
// value is trimmed before it is tested and before it is used, so nothing can
// pass one side and fail the other.
const SAFE_COLOR = /^(#[0-9a-f]{3,8}|[a-z]+|(rgb|rgba|hsl|hsla)\([0-9a-z ,.%/-]+\))$/i;

/** The colour if the launcher may paint with it, else null. */
function safeColor(value: string | null | undefined): string | null {
  const v = value?.trim() ?? "";
  return v && SAFE_COLOR.test(v) ? v : null;
}

/** The optional chrome colours and what they fall back to. */
const chromeMap: [keyof ThemeColors, string, keyof ThemeColors][] = [
  ["header", "--color-header", "surface"],
  ["headerText", "--color-header-text", "text"],
  ["footer", "--color-footer", "surface"],
  ["footerText", "--color-footer-text", "textMuted"],
];

/** The two text colours the UI paints on a coloured surface. */
const INK = { dark: "#08140c", light: "#f2f4f8" };

/** `#rgb`, `#rrggbb(aa)` and `rgb()/rgba()` as 0–255 triples; `null` otherwise. */
function parseColor(value: string): [number, number, number] | null {
  const hex = value.trim().replace(/^#/, "");
  if (/^[0-9a-f]{3,4}$/i.test(hex)) {
    return [0, 1, 2].map((i) => parseInt(hex[i] + hex[i], 16)) as [number, number, number];
  }
  if (/^[0-9a-f]{6}$/i.test(hex) || /^[0-9a-f]{8}$/i.test(hex)) {
    return [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16)) as [number, number, number];
  }
  const rgb = /^rgba?\(([^)]+)\)$/i.exec(value.trim());
  if (rgb) {
    const parts = rgb[1].split(/[\s,/]+/).filter(Boolean).slice(0, 3).map(Number);
    if (parts.length === 3 && parts.every((n) => Number.isFinite(n))) return parts as [number, number, number];
  }
  return null;
}

function luminance(rgb: [number, number, number]): number {
  const [r, g, b] = rgb.map((v) => {
    const c = Math.min(255, Math.max(0, v)) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * Every notation the colour validator accepts, resolved to a triple. Named
 * colours and `hsl()` need the browser to do it; without a document (tests)
 * only the notations `parseColor` knows work.
 */
function resolveColor(value: string): [number, number, number] | null {
  const direct = parseColor(value);
  if (direct || typeof document === "undefined") return direct;
  const probe = document.createElement("span");
  probe.style.color = value;
  if (!probe.style.color) return null;
  probe.style.display = "none";
  document.body.appendChild(probe);
  const computed = getComputedStyle(probe).color;
  probe.remove();
  return parseColor(computed);
}

/**
 * Whichever of the two inks stands out more on `background`. Comparing the
 * two contrasts beats a luminance threshold: a mid-tone green sits close
 * enough to the middle that a guessed cut-off picks the worse one.
 */
export function readableOn(background: string): string {
  const rgb = resolveColor(background);
  if (!rgb) return INK.dark;
  const base = luminance(rgb);
  const ratio = (ink: string) => {
    const [a, b] = [base, luminance(parseColor(ink) as [number, number, number])].sort((x, y) => y - x);
    return (a + 0.05) / (b + 0.05);
  };
  return ratio(INK.light) > ratio(INK.dark) ? INK.light : INK.dark;
}

// Same rules as `FontFace::is_valid` in crates/lanlauncher-core/src/theme.rs:
// a value that passes there must render here, or a font disappears without a
// word. The two quote characters and the CSS escape are what must not get in.
const FONT_URL = /^https?:\/\/[^\s"'<>\\]+$/;
const FONT_DATA = /^data:font\/[A-Za-z0-9;,/+=._-]+$/;
// `\p{M}` covers what Rust's `char::is_alphanumeric` lets through beyond
// letters and digits (combining marks), so neither side accepts a family the
// other one silently drops.
const FONT_FAMILY = /^[\p{L}\p{M}\p{N} ._-]{1,64}$/u;
const FONT_WEIGHT = /^([1-9]\d{0,2}|normal|bold|lighter|bolder)$/;
const FONT_STYLE = /^(normal|italic|oblique)$/;

/**
 * The `@font-face` rules for the event's fonts, written here from the checked
 * declarations — the launcher never takes a stylesheet from a LANPage, so a
 * theme can bring a font but not restyle the window.
 */
function fontFaceCss(faces: FontFace[]): string {
  const srcOk = (src: string) => (FONT_URL.test(src) && src.length <= 512) || (FONT_DATA.test(src) && src.length <= 2_000_000);
  return faces
    .filter(
      (f) =>
        FONT_FAMILY.test(f.family?.trim() ?? "") &&
        srcOk(f.src?.trim() ?? "") &&
        (!f.weight || FONT_WEIGHT.test(f.weight.trim())) &&
        (!f.style || FONT_STYLE.test(f.style.trim())),
    )
    .map((f) => {
      const parts = [`font-family: "${f.family.trim()}"`, `src: url("${f.src.trim()}")`];
      if (f.weight) parts.push(`font-weight: ${f.weight.trim()}`);
      if (f.style) parts.push(`font-style: ${f.style.trim()}`);
      parts.push("font-display: swap");
      return `@font-face { ${parts.join("; ")}; }`;
    })
    .join("\n");
}

// Same figures, defaults and bounds as `SurfacePattern` in
// crates/lanlauncher-core/src/theme.rs. A theme that draws its cards there and
// not here is a theme nobody can explain.
const PATTERN_COLOR = "rgba(128, 128, 128, 0.07)";
const PATTERN_SIZE = 4;
const PATTERN_ANGLE = 45;
const PATTERN_MIN = 2;
const PATTERN_MAX = 64;

/**
 * The `background-image` and `background-size` for the figure a theme asks
 * for, or `null` for a flat surface. The colour is checked and the numbers are
 * clamped here: this is the last place before the value stands in a
 * declaration, so nothing that could end it may get through.
 */
export function surfacePatternCss(pattern: SurfacePattern | null | undefined): { image: string; size: string } | null {
  if (!pattern || !pattern.kind || pattern.kind === "none") return null;
  const color = safeColor(pattern.color ?? PATTERN_COLOR);
  if (!color) return null;
  const raw = Number(pattern.size ?? PATTERN_SIZE);
  const size = Math.round(Math.min(PATTERN_MAX, Math.max(PATTERN_MIN, Number.isFinite(raw) ? raw : PATTERN_SIZE)));
  const degrees = Math.round(Number(pattern.angle ?? PATTERN_ANGLE));
  const angle = (((Number.isFinite(degrees) ? degrees : PATTERN_ANGLE) % 360) + 360) % 360;
  // One hairline at `deg`, then nothing until the next one.
  const lines = (deg: number) =>
    `repeating-linear-gradient(${deg}deg, ${color} 0, ${color} 1px, transparent 1px, transparent ${size}px)`;
  switch (pattern.kind) {
    case "scanlines":
      return { image: lines(0), size: "auto" };
    case "grid":
      return { image: `${lines(0)}, ${lines(90)}`, size: "auto" };
    // A dot needs its distance on the box: the radial gradient draws one dot
    // per tile of `background-size`.
    case "dots":
      return { image: `radial-gradient(${color} 1px, transparent 1px)`, size: `${size}px ${size}px` };
    case "diagonal":
      return { image: lines(angle), size: "auto" };
    case "gradient":
      return { image: `linear-gradient(${angle}deg, ${color}, transparent)`, size: "auto" };
    // A figure a newer launcher knows: flat here, and no reason to complain.
    default:
      return null;
  }
}

/** Put the event's font rules into the document, or take them out again. */
function applyFont(css: string | null | undefined) {
  let style = document.getElementById("theme-font") as HTMLStyleElement | null;
  if (css) {
    if (!style) {
      style = document.createElement("style");
      style.id = "theme-font";
      document.head.appendChild(style);
    }
    if (style.textContent !== css) style.textContent = css;
  } else if (style) {
    style.remove();
  }
}

/** Apply theme tokens to :root and inject the optional legacy stylesheet. */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const merged: Theme = { ...defaultTheme, ...theme, colors: { ...defaultTheme.colors, ...(theme.colors ?? {}) } };
  for (const [key, cssVar] of Object.entries(varMap) as [BaseColorKey, string][]) {
    const value = safeColor(merged.colors[key]);
    if (value) root.style.setProperty(cssVar, value);
  }
  // The bars may have colours of their own; a theme that says nothing about
  // them keeps the surface, so old themes look exactly as they did.
  for (const [key, cssVar, fallback] of chromeMap) {
    // Text on a bar the theme coloured: whoever names only the bar gets the
    // ink that reads on it, the way `primary` works. The bar's colour has to
    // be one we would paint with, or the ink would be picked for a colour
    // that never reaches the bar.
    const bar = key === "headerText" ? safeColor(merged.colors.header) : key === "footerText" ? safeColor(merged.colors.footer) : null;
    const value = safeColor(merged.colors[key]) ?? (bar ? readableOn(bar) : safeColor(merged.colors[fallback]));
    if (value) root.style.setProperty(cssVar, value);
  }
  // The Play button paints text on the success colour, which a scheme may
  // choose dark (readable on a light card) or bright (readable on a dark
  // one). Picking the text per theme is the only way both stay legible.
  root.style.setProperty("--color-success-text", readableOn(merged.colors.success));
  root.style.setProperty("--color-warning-text", readableOn(merged.colors.warning));
  root.style.setProperty("--radius", `${Math.min(48, Math.max(0, merged.radius))}px`);
  // Nothing set means a flat card, exactly as before this key existed.
  const pattern = surfacePatternCss(merged.surfacePattern);
  if (pattern) {
    root.style.setProperty("--surface-pattern", pattern.image);
    root.style.setProperty("--surface-pattern-size", pattern.size);
  } else {
    root.style.removeProperty("--surface-pattern");
    root.style.removeProperty("--surface-pattern-size");
  }
  // A stack ends up in a declaration of its own, so what could close it goes.
  // The headings fall back to the body font in CSS, so leaving the property
  // unset is how a theme without a headline face keeps looking as it did.
  for (const [cssVar, stack] of [
    ["--font-family", merged.fontFamily],
    ["--font-family-heading", merged.headingFontFamily],
  ] as const) {
    if (stack) root.style.setProperty(cssVar, stack.replace(/[;{}<>]/g, ""));
    else root.style.removeProperty(cssVar);
  }
  root.dataset.themeMode = merged.mode;
  const hasImage = !!merged.backgroundImage && /^(https?:\/\/|data:image\/)/.test(merged.backgroundImage);
  if (hasImage) {
    root.style.setProperty("--bg-image", `url("${(merged.backgroundImage as string).replace(/"/g, "")}")`);
  } else {
    root.style.removeProperty("--bg-image");
  }
  // The background layer lies behind the body, which is opaque by default;
  // without this the image and its overlay are never seen.
  root.toggleAttribute("data-bg-image", hasImage);
  // A photo behind the content eats the text; the overlay is how an organiser
  // keeps their picture and the launcher readable at the same time.
  const overlay = safeColor(merged.backgroundOverlay);
  if (overlay) {
    root.style.setProperty("--bg-overlay", overlay);
  } else {
    root.style.removeProperty("--bg-overlay");
  }
  applyFont(fontFaceCss(merged.fontFaces ?? []));
  // ETI's launcher.css paints `html`; scoped to #bg_layer it only shows when
  // the body lets it through (see app.css, [data-legacy]).
  root.toggleAttribute("data-legacy", !!merged.legacyCss);
  let legacy = document.getElementById("legacy-launcher-css") as HTMLStyleElement | null;
  if (merged.legacyCss) {
    if (!legacy) {
      legacy = document.createElement("style");
      legacy.id = "legacy-launcher-css";
      document.head.appendChild(legacy);
    }
    // Scope the old ETI stylesheet so it only tints the background layer.
    legacy.textContent = merged.legacyCss.replace(/(^|\})\s*html\s*\{/g, "$1 #bg_layer {");
  } else if (legacy) {
    legacy.remove();
  }
}
