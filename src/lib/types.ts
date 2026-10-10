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
  | { kind: "add_chat_firewall_rules" }
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

export type RunnerKind = "auto" | "wine" | "crossover" | "proton" | "native";

export interface ManifestInfo {
  origin: ManifestOrigin;
  exe: string;
  args: string[];
  runner: RunnerKind;
  alternatives: string[];
  notes: string | null;
  verifiedForRevision: boolean;
  /** The tester's own launch configuration is laid over the profile. */
  ownConfig: boolean;
  /** Windows components (winetricks verbs) the game needs on this platform. */
  components: string[];
  /** What the profile was tested with on this platform ("Proton 11.0"). */
  testedWith: string[];
  /** Package revisions the profile was checked with (empty: any). */
  checkedRevisions: string[];
  /** The package revision it is compared with (installed, else the catalog's). */
  packageRevision: string;
  /** The revision the own configuration was saved with. */
  configRevision: string | null;
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
  /** Exact Wine/Proton/CrossOver program selected per game; absent means automatic. */
  gameRunners: Record<
    string,
    { program: string; runner: "wine" | "crossover" | "proton"; label: string; steamRoot: string | null; sharesDefaultPrefix: boolean }
  >;
  syncPort: number;
  /** Overrides the built-in key of the catalog share (eti_launcher). */
  catalogKey: string | null;
  /** Explicit Resilio binary (e.g. the ETI launcher's btsync.exe). */
  resilioBinary: string | null;
  resilioApiKey: string | null;
  /** Take part in the LAN chat. */
  chatEnabled: boolean;
  /** Play a sound for new chat messages. */
  chatSound: boolean;
}

export interface RunnerOption {
  program: string;
  label: string;
  kind: "wine" | "crossover" | "proton";
}

export interface RunnerChoices {
  selected: string | null;
  selectedKind: RunnerOption["kind"] | null;
  options: RunnerOption[];
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

/** The figures a theme may ask for on its surfaces; the launcher draws them. */
export type SurfacePatternKind = "none" | "scanlines" | "grid" | "dots" | "diagonal" | "gradient";

export interface SurfacePattern {
  kind: SurfacePatternKind;
  /** Ink of the figure; usually an `rgba()` well under 0.1. */
  color?: string;
  /** Distance between the lines or dots in pixels. */
  size?: number;
  /** Direction of `diagonal` and `gradient` in degrees. */
  angle?: number;
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
  /** Texture on the cards, tiles and the detail panel; flat without it. */
  surfacePattern?: SurfacePattern | null;
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
  /** The LANPage itself, once launcher.ini answered; shown as a tab. */
  page: string | null;
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
  activity?: "discovering" | "indexing" | "busy" | null;
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
  /** Programs in front of `program` (`gamemoderun`, `gamescope … --`). */
  wrapper?: string[];
}

/** A game's launch settings on this platform, as the editor shows them. */
export interface GameConfig {
  exe: string;
  /** As typed; split like a shell when saved. */
  args: string;
  workdir: string;
  runner: RunnerKind;
  env: { name: string; value: string }[];
  dllOverrides: string;
  wrapper: string;
  /** Windows components for the prefix, winetricks verbs separated by spaces. */
  winetricks: string;
}

export interface ComponentsReport {
  installed: string[];
  failed: string[];
  offline: boolean;
  missingTool: string | null;
  timedOut: boolean;
  log: string;
}

export interface GameConfigView {
  config: GameConfig;
  own: boolean;
  /** The saved configuration does not read; a reset removes it. */
  configError: string | null;
  executables: string[];
  platform: string;
  reportEmail: string;
  /** The starts the profile offers (its own first), to fill the fields from; empty without alternatives. */
  entryPoints: EntryPoint[];
}

export interface EntryPoint {
  /** The alternative's name; null for the profile's own start. */
  name: string | null;
  exe: string;
  args: string;
  workdir: string;
}

export interface ConfigReport {
  subject: string;
  body: string;
  toml: string;
  mailto: string;
  issueUrl: string;
  fileName: string;
}

export interface LibrarySpace {
  path: string;
  label: string;
  isDefault: boolean;
  freeBytes: number | null;
  totalBytes: number | null;
  games: number;
}

// LAN chat (`lanlauncher_core::chat`).

export type PollKind = "single" | "multiple";

export interface ChatReaction {
  emoji: string;
  nicks: string[];
  mine: boolean;
}

export interface ChatPollOption {
  id: string;
  text: string;
  /** Catalog id when the option is a game. */
  game: string | null;
  voters: string[];
  mine: boolean;
  /** Nickname of whoever added it to an open poll. */
  addedBy: string | null;
}

export interface ChatPoll {
  question: string;
  kind: PollKind;
  /** Everyone may add answers. */
  open: boolean;
  closed: boolean;
  options: ChatPollOption[];
  participants: number;
}

export interface ChatItem {
  id: string;
  from: string;
  nick: string;
  /** Milliseconds since the epoch, by the author's clock (clamped to arrival). */
  ts: number;
  /** When this launcher got it, by its own clock. */
  received: number;
  /** null: the public room; "#<id>" a topic; otherwise the other person's peer id. */
  conversation: string | null;
  mine: boolean;
  deleted: boolean;
  text: string | null;
  /** The text was changed after sending. */
  edited: boolean;
  /** Set on the item that opened a topic: its name. */
  topicName: string | null;
  /** A linked game's catalog id; `text` is its title. */
  game: string | null;
  reply: { id: string; nick: string | null; text: string | null; deleted: boolean } | null;
  reactions: ChatReaction[];
  poll: ChatPoll | null;
}

export interface ChatPeer {
  id: string;
  nick: string;
  os: string;
  online: boolean;
  address: string;
  /** A chat relay (nll-chat-relay) keeping the history, not a person. */
  relay: boolean;
  /** Title of the game running there, as that launcher reports it. */
  playing: string | null;
  /** About that computer (what the LANPage gets too), once known. */
  info: { host: string; system: string; cpu: string; gpu: string; version: string } | null;
}

/** Someone the LANPage shows as online who is not in the chat (the ETI
 *  launcher, mostly): from its stats beacon, `stats.php?online=1`. */
export interface LanPagePlayer {
  player: string;
  host: string;
  address: string;
  system: string;
  cpu: string;
  gpu: string;
  game: string | null;
  gameTitle: string | null;
  seen: number;
}

export interface ChatSnapshot {
  me: string;
  nick: string;
  items: ChatItem[];
  peers: ChatPeer[];
  /** `err.chat_*` code when the chat cannot reach the LAN as it should. */
  problem: string | null;
}

export type ChatUpdate = { kind: "items"; items: ChatItem[] } | { kind: "peers"; peers: ChatPeer[] } | { kind: "reset" };
