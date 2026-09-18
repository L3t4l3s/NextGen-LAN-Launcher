import type { LibraryRoot } from "./types";

/** Match the backend's fallback for older settings without a default flag. */
export function defaultRootIndex(roots: LibraryRoot[]): number {
  const index = roots.findIndex((root) => root.isDefault);
  return index >= 0 ? index : roots.length ? 0 : -1;
}

export function setupRoots(roots: LibraryRoot[], platform: string): LibraryRoot[] {
  if (!roots.length) {
    const path = platform === "windows" ? "C:\\LAN" : "";
    return [{ path, label: path, isDefault: true }];
  }
  const index = defaultRootIndex(roots);
  return roots.map((root, i) => ({ ...root, isDefault: i === index }));
}

export function cleanRoots(roots: LibraryRoot[], platform: string): LibraryRoot[] {
  const unique = new Map<string, LibraryRoot>();
  for (const root of roots) {
    const path = root.path.trim();
    if (!path) continue;
    const key = platform === "windows" ? path.replaceAll("/", "\\").replace(/\\+$/, "").toLowerCase() : path.replace(/\/+$/, "");
    const existing = unique.get(key);
    if (existing) existing.isDefault ||= root.isDefault;
    else unique.set(key, { ...root, path, label: path });
  }
  const result = [...unique.values()];
  const index = defaultRootIndex(result);
  return result.map((root, i) => ({ ...root, isDefault: i === index }));
}
