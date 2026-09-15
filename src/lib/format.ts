export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "–";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  const d = i === 0 ? 0 : digits;
  return `${v.toFixed(d).replace(/\.0+$/, "")} ${units[i]}`;
}

export function formatSpeed(bps: number): string {
  if (!bps) return "";
  return `${formatBytes(bps)}/s`;
}

export function formatPercent(fraction: number): string {
  return `${Math.round(Math.min(1, Math.max(0, fraction)) * 100)} %`;
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
