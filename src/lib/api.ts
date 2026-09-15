// Thin wrapper around Tauri's invoke with a browser mock for development in
// a plain browser (`npm run dev` without Tauri) and for screenshots.

import type {
  BootstrapInfo,
  EventBundle,
  FixAction,
  GameStatus,
  GameView,
  LaunchPlan,
  LibrarySpace,
  Report,
  Settings,
  TransportHealth,
} from "./types";
import { createMock } from "./mock";

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type Listen = (event: string, cb: (payload: unknown) => void) => Promise<() => void>;

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let invokeImpl: Invoke;
let listenImpl: Listen;
let convertSrc: (path: string) => string = (p) => p;

if (inTauri) {
  const core = await import("@tauri-apps/api/core");
  const ev = await import("@tauri-apps/api/event");
  invokeImpl = (cmd, args) => core.invoke(cmd, args);
  listenImpl = async (event, cb) => ev.listen(event, (e) => cb(e.payload));
  convertSrc = (p) => core.convertFileSrc(p);
} else {
  const mock = createMock();
  invokeImpl = mock.invoke as Invoke;
  listenImpl = mock.listen;
}

export const invoke = invokeImpl;
export const listen = listenImpl;
export const coverSrc = (path: string | null): string | null => (path ? convertSrc(path) : null);

export const api = {
  bootstrap: () => invoke<BootstrapInfo>("get_bootstrap"),
  games: () => invoke<GameView[]>("get_games"),
  statuses: () => invoke<GameStatus[]>("get_statuses"),
  install: (gameId: string) => invoke<void>("install_game", { gameId }),
  repair: (gameId: string) => invoke<void>("repair_game", { gameId }),
  pause: (gameId: string, paused: boolean) => invoke<void>("pause_game", { gameId, paused }),
  uninstall: (gameId: string) => invoke<void>("uninstall_game", { gameId }),
  play: (gameId: string, alternative?: number) => invoke<number>("play_game", { gameId, alternative: alternative ?? null }),
  launchPlan: (gameId: string, alternative?: number) => invoke<LaunchPlan>("get_launch_plan", { gameId, alternative: alternative ?? null }),
  listExecutables: (gameId: string) => invoke<string[]>("list_executables", { gameId }),
  setExeOverride: (gameId: string, exe: string) => invoke<void>("set_exe_override", { gameId, exe }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  refreshCatalog: () => invoke<number>("refresh_catalog"),
  diagnostics: () => invoke<Report>("run_diagnostics"),
  applyFix: (fix: FixAction) => invoke<string>("apply_fix", { fix }),
  health: () => invoke<TransportHealth | null>("get_transport_health"),
  refreshEvent: () => invoke<EventBundle>("refresh_event"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  shareKey: (gameId: string) => invoke<string>("get_share_key", { gameId }),
  librarySpace: () => invoke<LibrarySpace[]>("get_library_space"),
  restartTransport: () => invoke<void>("restart_transport"),
};

export async function pickFolder(): Promise<string | null> {
  if (!inTauri) {
    return window.prompt("Ordner (Demo im Browser):", "D:\\LAN") ?? null;
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const res = await open({ directory: true, multiple: false });
  return typeof res === "string" ? res : null;
}

/** File picker; `extensions` (e.g. ["exe"]) is only meaningful on Windows, elsewhere any file may be chosen. */
export async function pickFile(extensions?: string[]): Promise<string | null> {
  if (!inTauri) {
    return window.prompt("Datei (Demo im Browser):", "C:\\Program Files\\eti\\lan launcher\\btsync.exe") ?? null;
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const res = await open({
    directory: false,
    multiple: false,
    ...(extensions?.length ? { filters: [{ name: "Programm", extensions }] } : {}),
  });
  return typeof res === "string" ? res : null;
}

export async function confirmDialog(message: string): Promise<boolean> {
  if (!inTauri) return window.confirm(message);
  const { ask } = await import("@tauri-apps/plugin-dialog");
  return ask(message, { kind: "warning" });
}

export async function copyText(text: string): Promise<void> {
  if (!inTauri) {
    await navigator.clipboard?.writeText(text);
    return;
  }
  const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
  await writeText(text);
}
