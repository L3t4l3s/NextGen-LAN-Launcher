import type { Theme } from "./types";
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
  radius: 12,
  fontFamily: null,
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

const varMap: Record<keyof Theme["colors"], string> = {
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

const SAFE_COLOR = /^(#[0-9a-fA-F]{3,8}|[a-zA-Z]+|(rgb|rgba|hsl|hsla)\([0-9a-zA-Z ,.%/-]+\))$/;

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

/** Apply theme tokens to :root and inject the optional legacy stylesheet. */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const merged: Theme = { ...defaultTheme, ...theme, colors: { ...defaultTheme.colors, ...(theme.colors ?? {}) } };
  for (const [key, cssVar] of Object.entries(varMap) as [keyof Theme["colors"], string][]) {
    const value = merged.colors[key];
    if (SAFE_COLOR.test(value)) root.style.setProperty(cssVar, value);
  }
  // The Play button paints text on the success colour, which a scheme may
  // choose dark (readable on a light card) or bright (readable on a dark
  // one). Picking the text per theme is the only way both stay legible.
  root.style.setProperty("--color-success-text", readableOn(merged.colors.success));
  root.style.setProperty("--color-warning-text", readableOn(merged.colors.warning));
  root.style.setProperty("--radius", `${Math.min(48, Math.max(0, merged.radius))}px`);
  if (merged.fontFamily) root.style.setProperty("--font-family", merged.fontFamily.replace(/[;{}<>]/g, ""));
  else root.style.removeProperty("--font-family");
  root.dataset.themeMode = merged.mode;
  if (merged.backgroundImage && /^(https?:\/\/|data:image\/)/.test(merged.backgroundImage)) {
    root.style.setProperty("--bg-image", `url("${merged.backgroundImage.replace(/"/g, "")}")`);
  } else {
    root.style.removeProperty("--bg-image");
  }
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
