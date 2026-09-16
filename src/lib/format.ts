import type { BootstrapInfo } from "$lib/types";

/**
 * Windows Explorer labels 2^30 bytes as "GB", Finder and most Linux desktops
 * use 10^9. Follow the platform so the numbers match what users see next to
 * the launcher; the bootstrap sets this once the platform is known.
 */
let byteBase: 1000 | 1024 = 1000;

export function setByteUnits(platform: BootstrapInfo["platform"]): void {
  byteBase = platform === "windows" ? 1024 : 1000;
}

export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "–";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= byteBase && i < units.length - 1) {
    v /= byteBase;
    i++;
  }
  const d = i === 0 ? 0 : digits;
  return `${v.toFixed(d).replace(/\.0+$/, "")} ${units[i]}`;
}

export function formatSpeed(bps: number): string {
  if (!bps) return "";
  return `${formatBytes(bps)}/s`;
}

/// Like `formatSpeed`, but a standing zero is an answer too: the status bar
/// shows "0 B/s" rather than an empty gap when nothing is moving.
export function formatRate(bps: number): string {
  return `${formatBytes(bps || 0)}/s`;
}

export function formatPercent(fraction: number): string {
  return `${percentValue(fraction)} %`;
}

/**
 * The same number as a CSS length. `formatPercent` puts a space before the
 * sign, as German typography wants — and CSS drops a declaration with one,
 * which left every progress bar looking full whatever the number said.
 */
export function percentWidth(fraction: number): string {
  return `${percentValue(fraction)}%`;
}

function percentValue(fraction: number): number {
  const f = Number.isFinite(fraction) ? fraction : 0;
  return Math.round(Math.min(1, Math.max(0, f)) * 100);
}

/** `20250308` → `08.03.2025` (de) / `2025-03-08` (en). */
export function formatRevision(rev: string, lang: string): string {
  const m = /^(\d{4})(\d{2})(\d{2})$/.exec(rev);
  if (!m) return rev;
  return lang === "de" ? `${m[3]}.${m[2]}.${m[1]}` : `${m[1]}-${m[2]}-${m[3]}`;
}

/** Stable pastel gradient from a game id for cover placeholders. */
export function placeholderGradient(id: string): string {
  let h = 0;
  for (const ch of id) h = (h * 31 + ch.charCodeAt(0)) % 360;
  const h2 = (h + 40) % 360;
  return `linear-gradient(135deg, hsl(${h} 55% 38%), hsl(${h2} 60% 22%))`;
}

export function stripHtml(html: string): string {
  return html
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .trim();
}
