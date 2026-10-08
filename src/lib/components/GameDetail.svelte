<script lang="ts">
  import type { Extra, GameView, LaunchPlan, RunnerChoices } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { api, chooseDialog, confirmDialog, copyText, coverSrc } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import { formatBytes, formatPercent, formatRevision, formatSpeed, placeholderGradient, runnerKey, stripHtml, percentWidth } from "$lib/format";
  import { isBusy, isPlayable, phaseBadge } from "$lib/phase";
  import ProblemCard from "./ProblemCard.svelte";
  import GameConfigDialog, { componentRuns } from "./GameConfigDialog.svelte";
  import ReportDialog from "./ReportDialog.svelte";
  import Icon from "./Icon.svelte";
  import { TEST_PHASE } from "$lib/testphase";

  let { game, onclose }: { game: GameView; onclose: () => void } = $props();

  const status = $derived(app.statusOf(game.id));
  const badge = $derived(phaseBadge(status));
  const busy = $derived(isBusy(status));
  const playable = $derived(isPlayable(status));
  const lang = $derived(app.settings?.language ?? "de");
  const platform = $derived(app.bootstrap?.platform ?? "windows");
  // What is running, not what is configured: the launcher falls back to
  // folder mode by itself when no engine starts, and that is when the key
  // has to be readable.
  const folderMode = $derived(app.health ? app.health.kind === "folder" : !!app.bootstrap?.transportError);
  let working = $state(false);
  let showExeChooser = $state(false);
  let showConfig = $state(false);
  let showReport = $state(false);
  let executables = $state<string[]>([]);
  let plan = $state<LaunchPlan | null>(null);
  let runnerChoices = $state<RunnerChoices | null>(null);
  let runnerRevision = $state(0);
  // A version chosen in the start configuration that is gone (Proton
  // removed): said here, changed there.
  const selectedRunnerMissing = $derived.by(() => {
    const chosen = runnerKey(runnerChoices?.selectedKind, runnerChoices?.selected);
    return (
      !!chosen &&
      game.manifest?.runner !== "native" &&
      !(runnerChoices?.options.some((runner) => runnerKey(runner.kind, runner.program) === chosen) ?? false)
    );
  });
  let shareKey = $state<string | null>(null);
  let copied = $state(false);
  // A video file may exist but not decode (missing codecs, damaged file);
  // then the cover image and its aspect ratio take over.
  let videoFailed = $state(false);
  $effect(() => {
    game.id;
    videoFailed = false;
  });
  const showVideo = $derived(!!game.video && !videoFailed);
  let alternative = $state<number | null>(null);

  let runnerRequest = 0;
  $effect(() => {
    const id = game.id;
    const currentPlatform = platform;
    runnerRevision;
    const token = ++runnerRequest;
    runnerChoices = null;
    if (currentPlatform === "windows") return;
    api
      .runnerOptions(id)
      .then((choices) => {
        if (token !== runnerRequest) return;
        runnerChoices = choices;
      })
      .catch(() => {
        if (token === runnerRequest) runnerChoices = null;
      });
  });

  $effect(() => {
    // reset per game
    game.id;
    plan = null;
    shareKey = null;
    alternative = null;
    showExeChooser = false;
    if (folderMode) api.shareKey(game.id).then((k) => (shareKey = k)).catch(() => (shareKey = null));
  });

  async function run(label: string, fn: () => Promise<unknown>, toast?: string, kind: "success" | "info" = "success") {
    working = true;
    try {
      await fn();
      if (toast) app.toast(kind, toast);
    } catch (e) {
      app.toast("error", t("toast.error", { detail: userText(e) }));
    } finally {
      working = false;
    }
  }

  const install = () => {
    // Without a sync server the download is still queued (Resilio picks the
    // share up once a peer appears), but the user is told there is no source yet.
    const noSource = app.health?.kind === "resilio" && app.health.server_found === false;
    const [key, kind] = noSource ? (["toast.install_no_server", "info"] as const) : (["toast.install_started", "success"] as const);
    return run("install", () => api.install(game.id), t(key, { title: game.title }), kind);
  };
  const repair = () => run("repair", () => api.repair(game.id), t("toast.repair_started", { title: game.title }));
  const pause = (p: boolean) => run("pause", () => api.pause(game.id, p));
  const openFolder = () => run("open", () => api.openPath(game.shareDir ?? ""));

  async function uninstall() {
    if (!(await confirmDialog(t("action.uninstall.confirm", { title: game.title })))) return;
    await run("uninstall", () => api.uninstall(game.id), t("toast.uninstalled", { title: game.title }));
    await app.reloadGames();
  }

  // What has to be on this computer: the layer that runs Windows games
  // and the Windows components the profile names (the launcher offers
  // those before the first start).
  const needs = $derived.by(() => {
    const m = game.manifest;
    if (!m || m.runner === "native") return null;
    const layer =
      m.runner === "auto" ? t(platform === "macos" ? "detail.needs.auto_macos" : "detail.needs.auto_linux") : t(`detail.needs.${m.runner}`);
    if (!m.components.length) return layer;
    // Not through CrossOver: it installs them itself ("Install Software").
    const offered = platform === "linux" || m.runner === "wine" || m.runner === "proton";
    const key = offered ? "detail.needs.components" : "detail.needs.components_plain";
    return `${layer}; ${t(key, { verbs: m.components.join(", ") })}`;
  });

  // Why the profile is not confirmed, and what helps: which package it was
  // checked with, and — where the button is — that a tester for whom it
  // runs can report it, so the profile gets updated.
  const unverified = $derived.by(() => {
    const m = game.manifest;
    if (!m || m.verifiedForRevision) return null;
    const current = formatRevision(m.packageRevision, lang);
    if (m.ownConfig) {
      return m.configRevision
        ? t("detail.manifest.unverified_config", { saved: formatRevision(m.configRevision, lang), current })
        : t("detail.manifest.unverified_config_unknown");
    }
    // The newest package it was checked with; a revision is a date.
    const newest = [...m.checkedRevisions].sort().at(-1);
    const reason =
      newest && !m.checkedRevisions.includes(m.packageRevision)
        ? t("detail.manifest.unverified_revision", { checked: formatRevision(newest, lang), current })
        : t("detail.manifest.unverified_script");
    return playable && platform !== "windows" ? `${reason} ${t("detail.manifest.unverified_confirm")}` : reason;
  });

  async function play(skipComponents = false) {
    if (status?.needsExeChoice) {
      await openExeChooser();
      return;
    }
    let needed: string | null = null;
    await run("play", async () => {
      try {
        const pid = await api.play(game.id, alternative ?? undefined, false, skipComponents);
        app.toast("success", t("detail.launched", { pid }));
      } catch (e) {
        const missing = /^err\.components_needed\|(.*)$/.exec(e instanceof Error ? e.message : String(e));
        if (!missing) throw e;
        needed = missing[1];
      }
    });
    if (needed) await offerComponents(needed);
  }

  // The profile names Windows components the game's prefix lacks (FlatOut 2
  // shows only an error without d3dx9_30): install them, then start — or
  // start without, e.g. at a LAN without internet.
  async function offerComponents(verbs: string) {
    const id = game.id;
    const title = game.title;
    const chosen = alternative ?? undefined;
    const choice = await chooseDialog(t("detail.components.ask", { title, verbs }), {
      yes: t("detail.components.install"),
      no: t("detail.components.without"),
      cancel: t("detail.components.cancel"),
    });
    if (choice === "cancel") return;
    if (choice === "no") {
      await play(true);
      return;
    }
    componentRuns.running = id;
    delete componentRuns.results[id];
    app.toast("info", t("detail.components.installing", { title, verbs }));
    // The game's buttons wait meanwhile: a second start or a repair would
    // only meet the busy prefix.
    working = true;
    try {
      const result = await api.installComponents(id, false, chosen);
      componentRuns.results[id] = result;
      if (result.failed.length) {
        app.toast("error", t("config.components.failed_toast", { title, verbs: result.failed.join(" ") }));
        return;
      }
      app.toast("success", t("config.components.done_toast", { title }));
    } catch (e) {
      app.toast("error", userText(e));
      return;
    } finally {
      componentRuns.running = null;
      working = false;
    }
    if (game.id === id) await play(true);
  }

  async function openExeChooser() {
    executables = await api.listExecutables(game.id).catch(() => []);
    showExeChooser = true;
  }

  async function chooseExe(exe: string) {
    await run("exe", () => api.setExeOverride(game.id, exe));
    showExeChooser = false;
    await app.reloadGames();
  }

  // The command line is shown as soon as the game is playable; no button
  // needed. A plan that cannot be built (missing script) just stays empty.
  const gameId = $derived(game.id);
  let planRequest = 0;
  $effect(() => {
    const id = gameId;
    const alt = alternative;
    runnerRevision;
    const token = ++planRequest;
    if (!playable) {
      plan = null;
      return;
    }
    // Only the newest request may set the plan; a slow answer for a game
    // the user has already left is dropped.
    api
      .launchPlan(id, alt ?? undefined)
      .then((p) => {
        if (token === planRequest) plan = p;
      })
      .catch(() => {
        if (token === planRequest) plan = null;
      });
  });

  async function runExtra(extra: Extra) {
    await run(extra, async () => {
      const pid = await api.runExtra(game.id, extra);
      app.toast("success", t("action.extra_started", { what: t(`action.${extra}`), pid }));
    });
  }


  async function copyKey() {
    if (!shareKey) return;
    await copyText(shareKey);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="detail">
  <div class="hero" class:video={showVideo} style:background={game.cover ? undefined : placeholderGradient(game.id)}>
    {#if showVideo}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video src={coverSrc(game.video)} autoplay muted loop playsinline poster={coverSrc(game.cover) ?? undefined} onerror={() => (videoFailed = true)}></video>
    {:else if game.cover}<img src={coverSrc(game.cover)} alt="" />{:else}<span class="initials">{game.title.slice(0, 2)}</span>{/if}
    <button class="close ghost" data-gamepad-back onclick={onclose} title={t("action.close")}>✕</button>
  </div>

  <div class="body">
    <div class="row title-row">
      <h2 class="grow">{game.title}</h2>
      {#if badge}<span class="badge {badge.cls}">{t(badge.label)}</span>{/if}
    </div>

    <div class="primary-row">
      {#if playable}
        <button class="success big" onclick={() => play()} disabled={working || app.bootstrap?.demo} title={app.bootstrap?.demo ? t("action.play_demo") : ""}>▶ {t("action.play")}</button>
        {#if status?.phase === "update_available"}
          <button class="primary" onclick={repair} disabled={working}>{t("action.update")}</button>
        {/if}
      {:else if busy}
        <button class="primary big" disabled>{t(`phase.${status?.phase}`)} …</button>
      {:else if status?.phase === "paused"}
        <button class="primary big" onclick={() => pause(false)} disabled={working}>{t("action.resume")}</button>
      {:else if status?.phase === "failed"}
        <button class="primary big" onclick={repair} disabled={working}>{t("action.retry")}</button>
      {:else}
        <button class="primary big" onclick={install} disabled={working || game.disabledByEvent}>{t("action.install")}</button>
      {/if}
    </div>

    {#if status && status.phase !== "not_installed"}
      <div class="progress-block">
        {#if busy || status.phase === "paused"}
          <div class="progress" class:stalled={status.stalled} class:working={status.phase !== "syncing" && status.phase !== "paused"}>
            <span style:width={percentWidth(status.progress)}></span>
          </div>
          <div class="row small muted">
            <span>{formatPercent(status.progress)}</span>
            {#if status.phase === "syncing" || status.phase === "paused"}
              <span>{t("detail.total", { total: formatBytes(status.bytesTotal) })}</span>
              {#if status.downloadBps}<span>{formatSpeed(status.downloadBps)}</span>{/if}
              <span>{t("detail.peers", { count: status.peers })}</span>
            {/if}
            {#if status.stalled}<span class="warn-text">{t("detail.stalled")}</span>{/if}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Secondary actions: always visible, whatever the launcher is doing. -->
    <div class="secondary">
      <button onclick={repair} disabled={working || (!status || status.phase === "not_installed")} title={t("action.repair.hint")}>🛠 {t("action.repair")}</button>
      {#if status?.phase === "paused"}
        <button onclick={() => pause(false)} disabled={working}>{t("action.resume")}</button>
      {:else}
        <button onclick={() => pause(true)} disabled={working || !busy}>{t("action.pause")}</button>
      {/if}
      <button onclick={openFolder} disabled={!game.shareDir}>📁 {t("action.open_folder")}</button>
      <!-- ETI's keygen.exe and server_start.cmd are Windows programs. -->
      {#if platform === "windows" && game.hasKeygen}
        <button onclick={() => runExtra("keygen")} disabled={working || !playable || app.bootstrap?.demo}>🔑 {t("action.keygen")}</button>
      {/if}
      {#if platform === "windows" && game.hasServerScript}
        <button onclick={() => runExtra("server")} disabled={working || !playable || app.bootstrap?.demo}>🖥 {t("action.server")}</button>
      {/if}
      <button class="danger" onclick={uninstall} disabled={working || !status || status.phase === "not_installed"}>{t("action.uninstall")}</button>
    </div>

    {#if status?.problem}
      <ProblemCard problem={status.problem} />
    {/if}

    {#if game.disabledByEvent}<p class="hint">{t("detail.disabled")}</p>{/if}
    {#if game.needsMasterServer}<p class="hint">⚑ {t("detail.master_server")}</p>{/if}

    <dl class="meta">
      <dt>{t("detail.size")}</dt><dd>{formatBytes(game.sizeBytes)}</dd>
      {#if game.maxPlayers}<dt>{t("detail.players")}</dt><dd>{game.maxPlayers}</dd>{/if}
      {#if game.genre}<dt>{t("detail.genre")}</dt><dd>{game.genre}</dd>{/if}
      {#if game.releaseYear}<dt>{t("detail.release")}</dt><dd>{game.releaseYear}</dd>{/if}
      {#if game.publisher}<dt>{t("detail.publisher")}</dt><dd>{game.publisher}</dd>{/if}
      <dt>{t("detail.revision")}</dt><dd>{formatRevision(game.revision, lang)}</dd>
      {#if status?.installedRevision}<dt>{t("detail.installed_revision")}</dt><dd>{formatRevision(status.installedRevision, lang)}</dd>{/if}
      {#if status?.installedRevision && game.shareDir}<dt>{t("detail.install_path")}</dt><dd class="install-path">{game.shareDir}{game.shareDir.endsWith("\\") || game.shareDir.endsWith("/") ? "" : game.shareDir.includes("\\") ? "\\" : "/"}local</dd>{/if}
    </dl>

    {#if game.readme}
      <p class="readme">{stripHtml(game.readme)}</p>
    {/if}

    <section class="launch-info">
      {#if selectedRunnerMissing}
        <p class="hint warn-text">{t("detail.runner_missing_hint")}</p>
      {/if}
      {#if platform !== "windows" && game.manifest}
        <p class="hint">
          {#if game.manifest.exe}
            <strong>{t("detail.starts_with")}:</strong> {game.manifest.exe} {game.manifest.args.join(" ")}
          {:else}
            <!-- Guidance-only profile: notes, but no verified entry point. -->
            <strong>{t("detail.manifest.autodetect")}</strong>
          {/if}
          <br /><span>{t(`detail.manifest.${game.manifest.origin}`)}</span>
          {#if game.manifest.ownConfig}<br /><span>{t("detail.manifest.own_config")}</span>{/if}
          {#if unverified}<br /><span class="warn-text">{unverified}</span>{/if}
          {#if needs}<br /><strong>{t("detail.needs")}:</strong> {needs}{/if}
          {#if game.manifest.testedWith.length && game.manifest.verifiedForRevision}<br /><strong>{t("detail.tested_with")}:</strong> {game.manifest.testedWith.join(", ")}{/if}
          {#if game.manifest.notes}<br />{game.manifest.notes}{/if}
        </p>
        {#if game.manifest.alternatives.length}
          <label for="alt">{t("action.play_alt")}</label>
          <select id="alt" bind:value={alternative}>
            <option value={null}>{game.manifest.exe}</option>
            {#each game.manifest.alternatives as alt, i (alt)}
              <option value={i}>{alt}</option>
            {/each}
          </select>
        {/if}
      {:else if platform !== "windows"}
        <p class="hint">{t("detail.manifest.none", { platform })}</p>
      {/if}
      {#if (playable && platform !== "windows") || TEST_PHASE}
        <div class="row">
          {#if playable && platform !== "windows"}
            <button class="ghost" onclick={openExeChooser}>{t("action.choose_exe")}</button>
            <button class="ghost" onclick={() => (showConfig = true)}>{t("action.launch_config")}</button>
          {/if}
          {#if TEST_PHASE}
            <button class="ghost" onclick={() => (showReport = true)}><Icon name="bug" /> {t("report.game")}</button>
          {/if}
        </div>
      {/if}
      {#if plan}
        <pre class="plan">{`${plan.runner}\n${plan.wrapper?.length ? `${plan.wrapper.join(" ")} ` : ""}${plan.program} ${plan.args.join(" ")}\ncwd: ${plan.cwd}`}</pre>
      {/if}
    </section>

    {#if folderMode && shareKey}
      <section class="card key">
        <h3>{t("detail.folder_key.title")}</h3>
        <p class="hint">{t("detail.folder_key.text", { dir: game.shareDir ?? "" })}</p>
        <div class="row">
          <code class="grow">{shareKey}</code>
          <button onclick={copyKey}>{copied ? t("action.copied") : t("action.copy")}</button>
        </div>
      </section>
    {/if}
  </div>
</div>

{#if showReport}
  <ReportDialog
    gameId={game.id}
    title={game.title}
    confirmedProfile={platform !== "windows" && game.manifest?.origin === "bundled" && game.manifest.verifiedForRevision}
    onclose={() => (showReport = false)}
  />
{/if}

{#if showConfig}
  <GameConfigDialog
    {game}
    onclose={() => (showConfig = false)}
    onsaved={() => {
      runnerRevision += 1;
      void app.reloadGames();
    }}
  />
{/if}

{#if showExeChooser}
  <div class="modal-backdrop" role="presentation" onclick={() => (showExeChooser = false)} onkeydown={(e) => e.key === "Escape" && (showExeChooser = false)}>
    <div class="modal card" role="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <h2>{t("detail.exe_choice.title")}</h2>
      <p class="hint">{t("detail.exe_choice.text")}</p>
      {#if executables.length === 0}
        <p class="muted">{t("detail.exe_choice.empty")}</p>
      {:else}
        <ul class="exes">
          {#each executables as exe (exe)}
            <li><button class="ghost" onclick={() => chooseExe(exe)}>{exe}</button></li>
          {/each}
        </ul>
      {/if}
      <div class="row end"><button data-gamepad-back onclick={() => (showExeChooser = false)}>{t("action.cancel")}</button></div>
    </div>
  </div>
{/if}

<style>
  .detail {
    display: flex;
    flex-direction: column;
  }
  .hero {
    position: relative;
    aspect-ratio: var(--cover-ratio);
    overflow: hidden;
    display: grid;
    place-items: center;
  }
  .hero.video {
    /* Preview videos are 16:9. */
    aspect-ratio: 16 / 9;
  }
  .initials {
    font-size: 3.5rem;
    font-weight: 800;
    color: rgba(255, 255, 255, 0.55);
    letter-spacing: -0.03em;
  }
  .hero img,
  .hero video {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .close {
    position: absolute;
    top: 0.6rem;
    right: 0.6rem;
    background: rgba(0, 0, 0, 0.45);
    border-color: transparent;
  }
  .body {
    padding: 1rem 1.25rem 2rem;
    display: flex;
    flex-direction: column;
    gap: 0.9rem;
  }
  .title-row h2 {
    margin: 0;
  }
  .primary-row {
    display: flex;
    gap: 0.6rem;
    align-items: center;
  }
  .primary-row .big {
    flex: 1;
  }
  .secondary {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
  }
  .secondary button {
    font-size: 0.9rem;
    padding: 0.5em 0.6em;
  }
  .progress-block {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .small {
    font-size: 0.82rem;
  }
  .warn-text {
    color: var(--color-warning);
  }
  .meta {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 1rem;
    margin: 0;
    font-size: 0.9rem;
  }
  dt {
    color: var(--color-text-muted);
  }
  dd {
    margin: 0;
  }
  .install-path { overflow-wrap: anywhere; user-select: text; }
  .readme {
    white-space: pre-line;
    font-size: 0.92rem;
    user-select: text;
  }
  .plan {
    font-size: 0.78rem;
    background: var(--color-bg);
    padding: 0.6rem;
    border-radius: 6px;
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
  }
  .key code {
    font-size: 0.85rem;
    user-select: text;
    word-break: break-all;
  }
  .exes {
    list-style: none;
    padding: 0;
    margin: 0 0 1rem;
    max-height: 50vh;
    overflow: auto;
  }
  .exes button {
    width: 100%;
    text-align: left;
  }
  .end {
    justify-content: flex-end;
  }
</style>
