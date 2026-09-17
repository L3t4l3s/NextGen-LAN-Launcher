// Mirrors the Rust types serialised by lanlauncher-core / src-tauri.

export type Phase =
  | "not_installed"
  | "queued"
  | "syncing"
  | "verifying"
  | "extracting"
  | "setup"
  | "ready"
  | "update_available"
  | "paused"
  | "failed";

export type Severity = "info" | "warning" | "error";

export type FixAction =
  | { kind: "set_network_profile_private"; interface_index: number }
  | { kind: "add_firewall_rules" }
  | { kind: "restart_transport" }
  | { kind: "repair_game"; game_id: string }
  | { kind: "open_folder"; path: string }
  | { kind: "open_url"; url: string }
  | { kind: "add_defender_exclusion"; path: string };

export interface Problem {
  code: string;
  severity: Severity;
  params: Record<string, string>;
  steps: string[];
  fix?: FixAction;
  /** Set when this warning can be hidden; the id the choice is stored under. */
  dismiss_key?: string;
}

export interface GameStatus {
  gameId: string;
  phase: Phase;
  progress: number;
  bytesDone: number;
  bytesTotal: number;
  peers: number;
  downloadBps: number;
  installedRevision: string | null;
  catalogRevision: string;
  stalled: boolean;
  problem: Problem | null;
  needsExeChoice: boolean;
  updatedAt: string;
}

export type ManifestOrigin = "bundled" | "user_override" | "share_overlay" | "derived_from_script";

export interface ManifestInfo {
  origin: ManifestOrigin;
  exe: string;
  args: string[];
  runner: "auto" | "wine" | "crossover" | "proton" | "native";
  alternatives: string[];
  notes: string | null;
  verifiedForRevision: boolean;
}

export interface GameView {
  id: string;
  order: number;
  title: string;
  revision: string;
  sizeBytes: number;
  releaseYear: string | null;
  publisher: string | null;
  maxPlayers: string | null;
  needsMasterServer: boolean;
  genre: string | null;
  readme: string | null;
  cover: string | null;
  video: string | null;
  status: GameStatus | null;
  manifest: ManifestInfo | null;
  disabledByEvent: boolean;
  shareDir: string | null;
  hasKeygen: boolean;
  hasServerScript: boolean;
}

export type Extra = "keygen" | "server";

export interface LibraryRoot {
  path: string;
  label: string;
  isDefault: boolean;
}

export type TransportMode = "managed" | "folder" | "demo";

export interface Settings {
  version: number;
  library: { roots: LibraryRoot[] };
  playerName: string;
  language: string;
  gameLanguage: string;
  transport: TransportMode;
  lanpageHost: string;
  lanMode: boolean;
  theme: string | null;
  setupComplete: boolean;
  allowElevation: boolean;
  runnerPaths: { wine: string | null; crossoverApp: string | null; proton: string | null };
  syncPort: number;
  /** Overrides the built-in key of the catalog share (eti_launcher). */
  catalogKey: string | null;
  /** Explicit Resilio binary (e.g. the ETI launcher's btsync.exe). */
  resilioBinary: string | null;
  resilioApiKey: string | null;
}

export interface Link {
  label: string;
  url: string;
}

export interface LanConfig {
  title: string | null;
  tag: string | null;
  website: string | null;
  force_lan_mode: boolean;
  lan_upload_limit_kbs: number | null;
  stats_url: string | null;
  ts3_server: string | null;
  discord_url: string | null;
  dc_hub: string | null;
  links: Link[];
  disabled_games: string[];
  extra: Record<string, string>;
}

export interface ThemeColors {
  background: string;
  surface: string;
  surfaceAlt: string;
  text: string;
  textMuted: string;
  primary: string;
  primaryText: string;
  accent: string;
  success: string;
  warning: string;
  danger: string;
  border: string;
  /** Top bar; missing keeps `surface`. */
  header?: string | null;
  /** Text on the top bar; missing keeps `text`. */
  headerText?: string | null;
  /** Status bar; missing keeps `surface`. */
  footer?: string | null;
  /** Text on the status bar; missing keeps `textMuted`. */
  footerText?: string | null;
}

export interface FontFace {
  family: string;
  /** http(s) or a data: URI. */
  src: string;
  weight?: string | null;
  style?: string | null;
}

export interface Theme {
  version: number;
  name: string;
  mode: "dark" | "light";
  colors: ThemeColors;
  logo: string | null;
  backgroundImage: string | null;
  /** Colour over the background image so text stays readable on a photo. */
  backgroundOverlay?: string | null;
  radius: number;
  fontFamily: string | null;
  /** Font stack of the headings; falls back to `fontFamily`. */
  headingFontFamily?: string | null;
  /** Font files of the event; the launcher writes the @font-face rules itself. */
  fontFaces?: FontFace[];
  icons: Record<string, string>;
  legacyCss: string | null;
}

export interface EventBundle {
  config: LanConfig | null;
  legacy_css: string | null;
  theme: Theme | null;
  logo: string | null;
  fetched: string[];
  errors: string[];
  server_time: string | null;
}

export interface SharePeer {
  name: string;
  connection: string | null;
  synced: boolean;
  downloadBps: number;
  uploadBps: number;
}

export interface TransportHealth {
  kind: "resilio" | "folder" | "demo";
  running: boolean;
  api_reachable: boolean;
  version: string | null;
  peers: number;
  /** Peers on the catalog share (eti_launcher) only. */
  catalog_peers: number;
  /** true: a sync server serves the catalog; false: nobody does; null: unknown (folder mode, key missing, API down). */
  server_found: boolean | null;
  lan_mode: boolean;
  /** The transport can name the peers of a share (Resilio with an API key). */
  peer_details: boolean;
  detail: string | null;
  /** Current totals over all shares, for the status bar; shown even at zero. */
  download_bps?: number;
  upload_bps?: number;
  /** Engine web interface including credentials, for "open in browser". */
  web_ui?: string | null;
}

/** What the launcher started last, for the diagnostics page. */
export interface LaunchAttempt {
  gameId: string;
  title: string;
  /** "play", "Keygen", "Server". */
  what: string;
  /** Milliseconds since the epoch. */
  at: number;
  runner: string;
  program: string;
  commandLine: string;
  cwd: string;
  elevated: boolean;
  /** Which entry point was started; null is the primary one. */
  alternative: number | null;
  pid: number | null;
  /** Error code when the start itself failed. */
  error: string | null;
  exitCode: number | null;
  ended: boolean;
  /** What the program printed (tail of the captured output). */
  output: string;
  /** Output was captured at all; a normal start keeps its own console. */
  captured: boolean;
}

export interface BootstrapInfo {
  settings: Settings;
  demo: boolean;
  platform: "windows" | "macos" | "linux";
  version: string;
  event: EventBundle;
  transportMode: TransportMode;
  transportError: string | null;
  needsSetup: boolean;
  /** The built-in catalog share key parses; a fork may ship without one. */
  builtinCatalogKey: boolean;
  dirs: { config: string; data: string; cache: string; logs: string };
}

export interface Report {
  problems: Problem[];
  /** Warnings the user has hidden; kept out of `problems`. */
  ignored: Problem[];
  checksRun: string[];
  generatedAt: string;
}

export interface LaunchPlan {
  program: string;
  args: string[];
  cwd: string;
  env: Record<string, string>;
  runner: string;
  needsElevation: boolean;
  rawCommandLine?: string;
}

export interface LibrarySpace {
  path: string;
  label: string;
  isDefault: boolean;
  freeBytes: number | null;
  totalBytes: number | null;
  games: number;
}
