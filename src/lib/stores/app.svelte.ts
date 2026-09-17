// Global reactive state (Svelte 5 runes).

import { api, listen } from "$lib/api";
import { setByteUnits } from "$lib/format";
import { setLanguage } from "$lib/i18n";
import { applyTheme, defaultTheme, builtinThemes } from "$lib/theme";
import type { BootstrapInfo, EventBundle, GameStatus, GameView, Settings, TransportHealth, Theme } from "$lib/types";

export type View = "library" | "downloads" | "diagnostics" | "settings";

/** Rate samples kept per game: one per status event (2 s), about two minutes. */
const SPEED_SAMPLES = 60;

export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  text: string;
}

class AppStore {
  ready = $state(false);
  view = $state<View>("library");
  bootstrap = $state<BootstrapInfo | null>(null);
  settings = $state<Settings | null>(null);
  games = $state<GameView[]>([]);
  statuses = $state<Record<string, GameStatus>>({});
  health = $state<TransportHealth | null>(null);
  /** What the library is showing: search, filters and sort order live here so
   *  a look at Downloads and back does not reset them. */
  libraryView = $state({
    query: "",
    filter: "all" as "all" | "installed" | "active",
    genre: "",
    sort: "title" as "title" | "players" | "size" | "year",
  });
  event = $state<EventBundle | null>(null);
  selectedId = $state<string | null>(null);
  toasts = $state<Toast[]>([]);
  showWizard = $state(false);
  loadError = $state<string | null>(null);
  /** Download rate per game, one sample per status event, newest last. */
  speedHistory = $state<Record<string, number[]>>({});
  /** Logo for the top bar, decided together with the theme in applyThemeFor. */
  logo = $state<string | null>(null);
  private toastSeq = 0;

  get selected(): GameView | null {
    return this.games.find((g) => g.id === this.selectedId) ?? null;
  }

  statusOf(id: string): GameStatus | null {
    return this.statuses[id] ?? this.games.find((g) => g.id === id)?.status ?? null;
  }

  get activeGames(): GameView[] {
    return this.games.filter((g) => {
      const s = this.statusOf(g.id);
      return s && s.phase !== "not_installed" && s.phase !== "ready" && s.phase !== "update_available";
    });
  }

  /** Playable games that still carry a hint (e.g. a setup-script warning). */
  get hintGames(): GameView[] {
    return this.games.filter((g) => {
      const s = this.statusOf(g.id);
      return !!s?.problem && (s.phase === "ready" || s.phase === "update_available");
    });
  }

  get eventTitle(): string {
    return this.event?.config?.title ?? "";
  }

  async init() {
    try {
      const b = await api.bootstrap();
      this.bootstrap = b;
      this.settings = b.settings;
      this.event = b.event;
      setLanguage(b.settings.language);
      setByteUnits(b.platform);
      this.applyThemeFor(b.settings, b.event);
      this.showWizard = b.needsSetup;
      // Listeners first: the backend emits catalog-updated as soon as the
      // startup load finishes, which may be before the first get_games.
      await listen("install-status", (payload) => {
        const list = payload as GameStatus[];
        const next: Record<string, GameStatus> = {};
        for (const s of list) next[s.gameId] = s;
        this.statuses = next;
        this.recordSpeeds(list);
      });
      await listen("transport-health", (payload) => {
        this.health = payload as TransportHealth;
      });
      // Rates arrive every second, the full health every 15: merging keeps
      // the status bar moving while something is moving.
      await listen("transport-rates", (payload) => {
        const rates = payload as { download_bps: number; upload_bps: number };
        if (this.health) this.health = { ...this.health, ...rates };
      });
      await listen("catalog-updated", () => {
        void this.reloadGames();
      });
      await listen("event-updated", (payload) => {
        this.event = payload as EventBundle;
        if (this.settings) this.applyThemeFor(this.settings, this.event);
      });
      await this.reloadGames();
      this.health = await api.health();
      this.ready = true;
    } catch (e) {
      this.loadError = String(e);
      this.ready = true;
    }
  }

  applyThemeFor(settings: Settings, event: EventBundle | null) {
    let theme: Theme = defaultTheme;
    if (settings.theme && builtinThemes[settings.theme]) {
      theme = builtinThemes[settings.theme];
    } else if (settings.theme === null && event?.theme) {
      theme = event.theme;
    }
    // The event's logo is branding, not a colour scheme: it stays in the
    // corner whichever scheme the user picked. Only the LANPage's stylesheet
    // is tied to automatic mode, because it paints.
    const auto = settings.theme === null;
    this.logo = theme.logo ?? event?.theme?.logo ?? event?.logo ?? null;
    applyTheme({ ...theme, legacyCss: auto ? (event?.legacy_css ?? null) : null });
  }

  /** Keep the last SPEED_SAMPLES rates of every downloading game for the
   *  chart; a game that stops downloading drops out. */
  private recordSpeeds(list: GameStatus[]) {
    const next = { ...this.speedHistory };
    let changed = false;
    for (const s of list) {
      if (s.phase === "syncing" || s.phase === "paused") {
        next[s.gameId] = [...(next[s.gameId] ?? []), s.downloadBps].slice(-SPEED_SAMPLES);
        changed = true;
      } else if (next[s.gameId]) {
        delete next[s.gameId];
        changed = true;
      }
    }
    if (changed) this.speedHistory = next;
  }

  async reloadGames() {
    this.games = await api.games();
    const next: Record<string, GameStatus> = { ...this.statuses };
    for (const g of this.games) if (g.status) next[g.id] = g.status;
    this.statuses = next;
    if (this.selectedId && !this.games.some((g) => g.id === this.selectedId)) this.selectedId = null;
  }

  async saveSettings(next: Settings) {
    const saved = await api.saveSettings(next);
    this.settings = saved;
    setLanguage(saved.language);
    this.applyThemeFor(saved, this.event);
    await this.reloadGames();
    return saved;
  }

  toast(kind: Toast["kind"], text: string) {
    const id = ++this.toastSeq;
    this.toasts = [...this.toasts, { id, kind, text }];
    setTimeout(() => {
      this.toasts = this.toasts.filter((t) => t.id !== id);
    }, kind === "error" ? 8000 : 4000);
  }
}

export const app = new AppStore();
