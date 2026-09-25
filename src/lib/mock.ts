// Browser-only mock of the Tauri backend. Simulates the demo catalog with a
// download that gets "stuck at 99 %" in the sync engine but completes through
// local verification – the scenario this launcher exists to fix.

import type { BootstrapInfo, GameStatus, GameView, Phase, Report, Settings } from "./types";

const demoGames: Omit<GameView, "status">[] = [
  g("amongus", 1, "Among Us", "20250308", 0.45, "2018", "Innersloth", "15", "Casual", "Wer ist der Impostor? Bis zu 15 Spieler im lokalen Netzwerk."),
  g("quake3", 2, "Quake 3 Arena", "20160922", 0.91, "1999", "id Software", "16", "Ego-Shooter", "Der Arena-Shooter-Klassiker.<br>Läuft auf jedem Toaster."),
  g("cod4", 3, "Call of Duty 4: Modern Warfare", "20220110", 8.2, "2007", "Activision", "32", "Ego-Shooter", "LAN-Klassiker mit Promod-Support."),
  g("wc3", 4, "Warcraft III: The Frozen Throne", "20201021", 1.6, "2003", "Blizzard Entertainment", "12", "Echtzeit-Strategie", "Inklusive DotA und Tower-Defense-Maps."),
  g("rocket", 5, "Rocket League", "20260410", 7.1, "2015", "Psyonix", "8", "Sport", "Autoball mit Raketenantrieb."),
  g("goldsrc", 6, "Counter-Strike 1.6 (GoldSrc)", "20240623", 1.0, "2003", "Valve", "32", "Ego-Shooter", "CS 1.6, CS 1.5 und Half-Life in einem Paket."),
  g("l4d2", 7, "Left 4 Dead 2", "20240115", 10.0, "2009", "Valve", "8", "Ego-Shooter", "Koop-Zombie-Shooter für 4 Spieler (8 im Versus)."),
  g("factorio", 8, "Factorio", "20250801", 2.3, "2020", "Wube Software", "65535", "Echtzeit-Strategie", "Die Fabrik muss wachsen."),
  g("flat2", 9, "FlatOut 2", "20190502", 3.1, "2006", "Bugbear", "8", "Racing", "Zerstörungsrennen mit Ragdoll-Physik."),
  g("bfbc2", 10, "Battlefield: Bad Company 2", "20210416", 16, "2010", "EA DICE", "32", "Ego-Shooter", "Benötigt den Battlefield-Masterserver im LAN.", true),
  g("7days", 11, "7 Days to Die", "20241201", 12.5, "2013", "The Fun Pimps", "8", "Survival", "Survival-Crafting mit Zombies."),
  g("aoe2", 12, "Age of Empires II: HD", "20230917", 4.4, "2013", "Microsoft", "8", "Echtzeit-Strategie", "Wololo."),
  g("trackmania", 13, "TrackMania Nations Forever", "20180303", 0.6, "2008", "Nadeo", "200", "Racing", "Kostenloser Arcade-Racer."),
  g("supra", 14, "Supraball", "20200606", 3.0, "2016", "Supra Games", "10", "Sport", "Fußball aus der Ego-Perspektive."),
];

function g(
  id: string,
  order: number,
  title: string,
  revision: string,
  gb: number,
  year: string,
  publisher: string,
  players: string,
  genre: string,
  readme: string,
  master = false,
): Omit<GameView, "status"> {
  return {
    id,
    order,
    title,
    revision,
    sizeBytes: Math.round(gb * 1e9),
    releaseYear: year,
    publisher,
    maxPlayers: players,
    needsMasterServer: master,
    genre,
    readme,
    cover: null,
    video: null,
    manifest:
      id === "amongus" || id === "goldsrc" || id === "quake3"
        ? {
            origin: "bundled",
            exe: id === "goldsrc" ? "hl-cs16/SmartSteamLoader.exe" : id === "amongus" ? "Among Us.exe" : "quake3.exe",
            args: id === "goldsrc" ? ["-game", "cstrike"] : [],
            runner: "auto",
            alternatives: id === "goldsrc" ? ["Counter-Strike 1.5", "Half-Life"] : [],
            notes: null,
            verifiedForRevision: true,
          }
        : { origin: "derived_from_script", exe: `${id}.exe`, args: [], runner: "auto", alternatives: [], notes: null, verifiedForRevision: false },
    disabledByEvent: false,
    shareDir: `D:\\LAN\\${id}`,
    hasKeygen: id === "cod4",
    hasServerScript: id === "goldsrc" || id === "quake3",
  };
}

interface Sim {
  phase: Phase;
  started: number;
  duration: number;
  stuck: boolean;
  paused: boolean;
  pausedAt: number;
  problem: GameStatus["problem"];
}

export function createMock() {
  const sims = new Map<string, Sim>();
  const listeners = new Map<string, Set<(p: unknown) => void>>();
  let settings: Settings = {
    version: 1,
    library: { roots: [{ path: "D:\\LAN", label: "SSD D:", isDefault: true }, { path: "E:\\LAN", label: "HDD E:", isDefault: false }] },
    playerName: "DemoPlayer",
    language: "de",
    gameLanguage: "de",
    transport: "demo",
    lanpageHost: "launcher.lan",
    lanMode: true,
    theme: null,
    setupComplete: true,
    allowElevation: true,
    runnerPaths: { wine: null, crossoverApp: null, proton: null },
    gameRunners: {},
    syncPort: 0,
    catalogKey: null,
    resilioBinary: null,
    resilioApiKey: null,
  };
  // pre-seeded states for a lively screenshot
  // A playable game with a leftover setup warning, as seen with adopted ETI installs.
  sims.set("quake3", {
    phase: "ready",
    started: 0,
    duration: 1,
    stuck: false,
    paused: false,
    pausedAt: 0,
    problem: {
      code: "install.setup_failed",
      severity: "warning",
      params: { detail: "launch error: game_setup.cmd exited with exit code: 1" },
      steps: [],
    },
  });
  sims.set("wc3", { phase: "update_available", started: 0, duration: 1, stuck: false, paused: false, pausedAt: 0, problem: null });
  sims.set("cod4", { phase: "syncing", started: Date.now() - 20_000, duration: 90_000, stuck: true, paused: false, pausedAt: 0, problem: null });
  sims.set("l4d2", { phase: "syncing", started: Date.now() - 5_000, duration: 400_000, stuck: false, paused: false, pausedAt: 0, problem: null });

  function status(id: string): GameStatus | null {
    const s = sims.get(id);
    if (!s) return null;
    const game = demoGames.find((x) => x.id === id)!;
    const total = game.sizeBytes;
    const elapsed = s.paused ? s.pausedAt - s.started : Date.now() - s.started;
    let frac = Math.min(1, elapsed / s.duration);
    let phase = s.phase;
    if (phase === "syncing" || phase === "verifying" || phase === "extracting" || phase === "setup") {
      if (frac >= 1) {
        const over = elapsed - s.duration;
        phase = over < 3000 ? "verifying" : over < 8000 ? "extracting" : over < 9500 ? "setup" : "ready";
        if (phase === "ready") s.phase = "ready";
        frac = phase === "verifying" ? Math.min(1, over / 3000) : phase === "extracting" ? Math.min(1, (over - 3000) / 5000) : 1;
      } else if (s.stuck) {
        frac = Math.min(frac, 0.99);
      }
    }
    if (s.paused) phase = "paused";
    const done = phase === "ready" || phase === "update_available" ? total : Math.round(total * frac);
    return {
      gameId: id,
      phase,
      progress: phase === "ready" || phase === "update_available" ? 1 : frac,
      bytesDone: done,
      bytesTotal: total,
      peers: s.paused ? 0 : 3,
      downloadBps: phase === "syncing" && !s.paused ? Math.round(total / (s.duration / 1000)) : 0,
      installedRevision: phase === "ready" ? game.revision : phase === "update_available" ? "20190101" : null,
      catalogRevision: game.revision,
      stalled: s.stuck && phase === "syncing" && frac >= 0.99,
      problem:
        s.problem ??
        (s.stuck && phase === "syncing" && frac >= 0.99
          ? {
              code: "sync.stalled",
              severity: "warning",
              params: { minutes: "3" },
              steps: ["sync.stalled.step.1", "sync.stalled.step.2"],
              fix: { kind: "repair_game", game_id: id },
            }
          : null),
      needsExeChoice: false,
      updatedAt: new Date().toISOString(),
    };
  }

  function emit(event: string, payload: unknown) {
    listeners.get(event)?.forEach((cb) => cb(payload));
  }

  setInterval(() => {
    emit("install-status", [...sims.keys()].map(status).filter(Boolean));
    // The real backend sends these once a second too, so the rates in the
    // status bar move here as well.
    emit("transport-rates", {
      download_bps: 7_000_000 + Math.round(Math.random() * 3_000_000),
      upload_bps: Math.round(Math.random() * 500_000),
    });
  }, 1000);

  const noServer = new URLSearchParams(location.search).has("noserver");
  const bootstrap: BootstrapInfo = {
    settings,
    demo: true,
    platform: "windows",
    version: "0.1.0-browser",
    event: {
      config: {
        title: "Beispiel-LAN 2026",
        tag: "BLP",
        website: "http://myparty.lan",
        force_lan_mode: true,
        lan_upload_limit_kbs: null,
        stats_url: "http://launcher.lan/stats.php",
        ts3_server: "ts3server://192.168.1.10?port=9987",
        discord_url: "https://discord.gg/example",
        dc_hub: "dchub://192.168.1.11:411",
        links: [
          { label: "Turnierplan", url: "http://myparty.lan/turnier" },
          { label: "Pizza bestellen", url: "http://myparty.lan/pizza" },
        ],
        disabled_games: [],
        extra: {},
      },
      legacy_css: null,
      logo: null,
      theme: null,
      fetched: ["launcher.ini"],
      errors: [],
      server_time: new Date().toISOString(),
    },
    transportMode: "demo",
    transportError: null,
    needsSetup: new URLSearchParams(location.search).has("wizard"),
    builtinCatalogKey: true,
    dirs: { config: "~/.config/nll", data: "~/.local/share/nll", cache: "~/.cache/nll", logs: "~/.local/share/nll/logs" },
  };

  const report: Report = {
    problems: [
      {
        code: "network.public_profile",
        severity: "error",
        params: { adapter: "Ethernet", network: "Netzwerk 3" },
        steps: ["network.public_profile.step.fix", "network.public_profile.step.manual"],
        fix: { kind: "set_network_profile_private", interface_index: 12 },
      },
      {
        code: "disk.low_space",
        severity: "warning",
        params: { path: "E:\\LAN", free_bytes: "9000000000", total_bytes: "2000000000000" },
        steps: ["disk.low_space.step.free", "disk.low_space.step.add_root"],
        fix: { kind: "open_folder", path: "E:\\LAN" },
      },
      {
        code: "network.public_profile_secondary",
        severity: "warning",
        params: { adapter: "WLAN", network: "Gast-WLAN", trusted_adapter: "Ethernet" },
        steps: ["network.public_profile.step.fix", "network.public_profile.step.manual"],
        fix: { kind: "set_network_profile_private", interface_index: 14 },
        dismiss_key: "network.public_profile_secondary:WLAN",
      },
      { code: "transport.demo_mode", severity: "info", params: {}, steps: [] },
    ],
    ignored: [],
    checksRun: ["library", "network_profile", "transport", "orphans", "lanpage", "clock", "catalog"],
    generatedAt: new Date().toISOString(),
  };

  const invoke = async (cmd: string, args: Record<string, unknown> = {}): Promise<unknown> => {
    await new Promise((r) => setTimeout(r, 60));
    const id = args.gameId as string;
    switch (cmd) {
      case "get_bootstrap":
        return { ...bootstrap, settings };
      case "get_games":
        return demoGames.map((x) => ({ ...x, status: status(x.id) }));
      case "get_statuses":
        return [...sims.keys()].map(status).filter(Boolean);
      case "install_game":
        sims.set(id, { phase: "syncing", started: Date.now(), duration: 25_000, stuck: true, paused: false, pausedAt: 0, problem: null });
        return;
      case "repair_game": {
        const s = sims.get(id);
        if (s) {
          s.phase = "syncing";
          s.started = Date.now() - 24_500;
          s.duration = 25_000;
          s.stuck = false;
          s.paused = false;
          s.problem = null;
        }
        return;
      }
      case "pause_game": {
        const s = sims.get(id);
        if (s) {
          if (args.paused) {
            s.paused = true;
            s.pausedAt = Date.now();
          } else if (s.paused) {
            s.started += Date.now() - s.pausedAt;
            s.paused = false;
          }
        }
        return;
      }
      case "cancel_download":
        sims.delete(id);
        return false;
      case "uninstall_game":
        sims.delete(id);
        return;
      case "run_extra":
      case "run_prereq_installer":
        return 4242;
      case "get_prereq_installer":
        return "D:\\LAN\\eti_launcher\\bin\\preqsetup.exe";
      case "play_game":
        throw new Error("Im Demo-Modus werden keine Spiele gestartet.");
      case "get_launch_plan":
        return { program: "C:\\Windows\\System32\\cmd.exe", args: ["/C", `"D:\\LAN\\${id}\\game_start.cmd"`, `"D:\\LAN\\${id}"`, id, "de", '"DemoPlayer"'], cwd: `D:\\LAN\\${id}`, env: {}, runner: "game_start.cmd", needsElevation: true };
      case "get_runner_options":
        return { selected: settings.gameRunners[id]?.program ?? null, selectedKind: settings.gameRunners[id]?.runner ?? null, options: [] };
      case "set_game_runner": {
        const program = args.program as string | null;
        const runner = (args.kind as "wine" | "crossover" | "proton" | null) ?? "wine";
        if (program) settings.gameRunners[id] = { program, runner, label: "Wine", steamRoot: null };
        else delete settings.gameRunners[id];
        return;
      }
      case "list_executables":
        return ["Game.exe", "bin/Launcher.exe", "tools/Config.exe"];
      case "set_exe_override":
        return;
      case "get_settings":
        return settings;
      case "save_settings": {
        const next = args.settings as Settings;
        const key = next.catalogKey?.trim() ?? "";
        if (key && !/^B[A-Z2-7]{32}$/.test(key)) throw new Error("err.invalid_catalog_key");
        settings = { ...next, catalogKey: key || null };
        return settings;
      }
      case "run_diagnostics":
        return report;
      case "frontend_ready":
        return;
      case "get_last_launch":
        // A failed start is the interesting case for the panel's layout.
        return {
          gameId: "doom",
          title: "Doom",
          what: "play",
          at: Date.now() - 45_000,
          runner: "game_start.cmd",
          program: "C:\\Windows\\System32\\cmd.exe",
          commandLine: '/S /C ""E:\\LAN\\doom\\game_start.cmd" "E:\\LAN\\doom" doom de "Player""',
          cwd: "E:\\LAN\\doom",
          elevated: false,
          alternative: null,
          pid: 4711,
          error: null,
          exitCode: 1,
          ended: true,
          captured: true,
          output: "Der Befehl \"fnr.exe\" ist entweder falsch geschrieben oder\nkonnte nicht gefunden werden.\nDrücken Sie eine beliebige Taste . . .",
        };
      case "set_problem_ignored": {
        const key = args.key as string;
        if (args.ignored) {
          report.ignored = [...report.ignored, ...report.problems.filter((p) => p.dismiss_key === key)];
          report.problems = report.problems.filter((p) => p.dismiss_key !== key);
        } else {
          report.problems = [...report.problems, ...report.ignored.filter((p) => p.dismiss_key === key)];
          report.ignored = report.ignored.filter((p) => p.dismiss_key !== key);
        }
        return;
      }
      case "apply_fix": {
        const fix = args.fix as { kind: string; game_id?: string };
        if (fix.kind === "set_network_profile_private") report.problems = report.problems.filter((p) => p.code !== "network.public_profile");
        if (fix.kind === "repair_game" && fix.game_id) await invoke("repair_game", { gameId: fix.game_id });
        return "Erledigt (Demo)";
      }
      case "get_transport_health":
        // `?noserver` shows the "no sync server" state of a managed Resilio.
        return noServer
          ? {
              kind: "resilio",
              running: true,
              api_reachable: true,
              version: "2.8.1",
              peers: 1,
              catalog_peers: 0,
              server_found: false,
              lan_mode: true,
              peer_details: true,
              detail: null,
              download_bps: 0,
              upload_bps: 0,
              web_ui: "http://127.0.0.1:8888/gui/",
            }
          : {
              kind: "demo",
              running: true,
              api_reachable: true,
              version: "demo",
              peers: 3,
              catalog_peers: 3,
              server_found: true,
              lan_mode: true,
              peer_details: false,
              detail: "simulated",
              download_bps: 8_400_000,
              upload_bps: 240_000,
              web_ui: "http://127.0.0.1:8888/gui/",
            };
      case "open_path":
      case "open_url":
        console.info("open", args);
        return;
      case "get_share_peers":
        return [
          { name: "sync-server", connection: "direct", synced: true, downloadBps: 8_400_000, uploadBps: 0 },
          { name: "PC-MAX", connection: "direct", synced: false, downloadBps: 1_250_000, uploadBps: 240_000 },
        ];
      case "get_share_key":
        return "BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAA2";
      case "get_library_space":
        return [
          { path: "D:\\LAN", label: "SSD D:", isDefault: true, freeBytes: 412e9, totalBytes: 1e12, games: 4 },
          { path: "E:\\LAN", label: "HDD E:", isDefault: false, freeBytes: 9e9, totalBytes: 2e12, games: 0 },
        ];
      case "restart_transport":
        return;
      default:
        throw new Error(`mock: unknown command ${cmd}`);
    }
  };

  const listen = async (event: string, cb: (p: unknown) => void) => {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event)!.add(cb);
    return () => listeners.get(event)?.delete(cb);
  };

  return { invoke, listen };
}
