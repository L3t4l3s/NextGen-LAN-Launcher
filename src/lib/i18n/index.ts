import { de } from "./de";
import { en } from "./en";

export type Messages = Record<string, string>;
const bundles: Record<string, Messages> = { de, en };

let current = "de";

export function setLanguage(lang: string) {
  current = bundles[lang] ? lang : "en";
}

export function currentLanguage(): string {
  return current;
}

export function t(key: string, params: Record<string, string | number> = {}): string {
  const bundle = bundles[current] ?? en;
  let text = bundle[key] ?? en[key] ?? key;
  for (const [k, v] of Object.entries(params)) {
    text = text.replaceAll(`{${k}}`, String(v));
  }
  return text;
}

/** Does a translation exist (used for optional step texts)? */
export function has(key: string): boolean {
  return key in (bundles[current] ?? en) || key in en;
}

export const languages = [
  { id: "de", label: "Deutsch" },
  { id: "en", label: "English" },
];

/**
 * Backend results are stable codes (`err.*` / `msg.*`), optionally followed by
 * `|detail`. Anything else (unexpected errors) is shown verbatim.
 */
export function userText(input: unknown): string {
  const raw = input instanceof Error ? input.message : String(input ?? "");
  const m = /^((?:err|msg)\.[a-z0-9_]+)(?:\|([\s\S]*))?$/.exec(raw);
  if (!m) return raw;
  return has(m[1]) ? t(m[1], { detail: m[2] ?? "" }) : raw;
}
