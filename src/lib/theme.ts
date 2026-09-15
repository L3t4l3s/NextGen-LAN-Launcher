import type { Theme } from "./types";

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
    accent: "#ff9f43",
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

/** Apply theme tokens to :root and inject the optional legacy stylesheet. */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const merged: Theme = { ...defaultTheme, ...theme, colors: { ...defaultTheme.colors, ...(theme.colors ?? {}) } };
  for (const [key, cssVar] of Object.entries(varMap) as [keyof Theme["colors"], string][]) {
    const value = merged.colors[key];
    if (SAFE_COLOR.test(value)) root.style.setProperty(cssVar, value);
  }
  root.style.setProperty("--radius", `${Math.min(48, Math.max(0, merged.radius))}px`);
  if (merged.fontFamily) root.style.setProperty("--font-family", merged.fontFamily.replace(/[;{}<>]/g, ""));
  else root.style.removeProperty("--font-family");
  root.dataset.themeMode = merged.mode;
  if (merged.backgroundImage && /^(https?:\/\/|data:image\/)/.test(merged.backgroundImage)) {
    root.style.setProperty("--bg-image", `url("${merged.backgroundImage.replace(/"/g, "")}")`);
  } else {
    root.style.removeProperty("--bg-image");
  }
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
