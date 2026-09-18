<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import type { LaunchAttempt, Report } from "$lib/types";
  import ProblemCard from "./ProblemCard.svelte";
  import { onMount } from "svelte";

  let report = $state<Report | null>(null);
  let lastLaunch = $state<LaunchAttempt | null>(null);
  let running = $state(false);

  async function run(background = false) {
    if (running) return;
    running = true;
    try {
      report = await api.diagnostics();
      app.health = await api.health();
      lastLaunch = await api.lastLaunch();
    } catch (e) {
      if (!background) app.toast("error", userText(e));
    } finally {
      running = false;
    }
  }

  // While the game is still running its output keeps growing; the panel is
  // only useful if it follows.
  $effect(() => {
    if (!lastLaunch || lastLaunch.ended) return;
    const timer = setInterval(() => {
      void api
        .lastLaunch()
        .then((l) => (lastLaunch = l))
        .catch(() => {});
    }, 2000);
    return () => clearInterval(timer);
  });

  // "A cmd window opens and nothing happens" is only answerable with the
  // command line in front of you, so the panel spells the whole start out.
  const launchResult = $derived.by(() => {
    const l = lastLaunch;
    if (!l) return "";
    if (l.error) return userText(l.error);
    if (!l.ended) return t("diag.launch.running", { pid: l.pid ?? 0 });
    return l.exitCode === null ? t("diag.launch.ended_unknown") : t("diag.launch.ended", { code: l.exitCode });
  });

  onMount(() => { void run(); });
  const catalogLoading = $derived(report?.problems.some((p) => p.code === "catalog.loading"));
  $effect(() => {
    if (!report?.problems.some((p) => p.code === "catalog.loading" || p.code === "catalog.missing" || p.code === "transport.preparing")) return;
    const timer = setInterval(() => void run(true), 10_000);
    return () => clearInterval(timer);
  });

  const errors = $derived(report?.problems.filter((p) => p.severity === "error").length ?? 0);
  const warnings = $derived(report?.problems.filter((p) => p.severity === "warning").length ?? 0);
  const sorted = $derived(
    [...(report?.problems ?? [])].sort((a, b) => ({ error: 0, warning: 1, info: 2 })[a.severity] - ({ error: 0, warning: 1, info: 2 })[b.severity]),
  );

  // `null` hides the button (no engine of ours: folder mode, demo), `""`
  // shows it greyed out while the engine is starting and has no address yet.
  const webUi = $derived(app.health?.kind === "resilio" ? (app.health.web_ui ?? "") : null);

  /// Start the game again with its output captured. The game's own console
  /// stays empty then, which is the trade for seeing what it printed.
  async function startWithLog(gameId: string | undefined) {
    if (!gameId) return;
    try {
      // The same entry point as the start that failed, not the primary one.
      await api.play(gameId, lastLaunch?.alternative ?? undefined, true);
      app.toast("success", t("diag.launch.retry_started"));
      setTimeout(() => void api.lastLaunch().then((l) => (lastLaunch = l)), 1500);
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  /// The setup keeps its own console so it can ask questions; repeated from
  /// here it writes everything down instead.
  async function rerunSetup(gameId: string | undefined) {
    if (!gameId) return;
    try {
      await api.rerunSetup(gameId);
      app.toast("success", t("toast.fix_done"));
    } catch (e) {
      app.toast("error", userText(e));
    }
    lastLaunch = await api.lastLaunch().catch(() => lastLaunch);
  }

  async function openWebUi() {
    if (!webUi) return;
    try {
      await api.openUrl(webUi);
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  async function restartTransport() {
    try {
      await api.restartTransport();
      app.toast("success", t("toast.fix_done"));
      await run();
    } catch (e) {
      app.toast("error", userText(e));
    }
  }
</script>

<!-- No heading and no introduction: the tab above says where you are, and
     the state of the check belongs next to the buttons, not in a line of its
     own that read like one more problem. -->
<div class="page diagnostics">
  <div class="row">
    {#if report}
      <!-- The time of the check is a detail; the state is the headline, and
           it must not wrap around the buttons next to it. -->
      <span
        class="state grow"
        class:bad={errors > 0}
        class:warn={errors === 0 && warnings > 0}
        title={t("diag.last_run", { time: new Date(report.generatedAt).toLocaleTimeString() })}
      >
        <span class="mark">{errors > 0 || warnings > 0 ? "⚠" : catalogLoading ? "…" : "✓"}</span>
        <strong>
          {#if errors > 0}{t("diag.summary.error", { count: errors })}{:else if warnings > 0}{t("diag.summary.warning", { count: warnings })}{:else if catalogLoading}{t("problem.catalog.loading.title")}{:else}{t("diag.ok.title")}{/if}
        </strong>
      </span>
    {:else}
      <span class="grow"></span>
    {/if}
    <button class="ghost" onclick={() => api.openPath(app.bootstrap?.dirs.logs ?? "")}>{t("diag.logs")}</button>
    {#if webUi !== null}
      <!-- Permanently available, not only when a problem offers it as a fix:
           the engine's own page answers questions the launcher cannot. -->
      <button class="ghost" onclick={openWebUi} disabled={!webUi} title={webUi ? t("diag.web_ui.hint") : t("diag.web_ui.unavailable")}>{t("diag.web_ui")}</button>
    {/if}
    <button class="ghost" onclick={restartTransport}>{t("diag.restart_transport")}</button>
    <button class="primary" onclick={() => run()} disabled={running}>{running ? t("diag.running") : t("diag.run")}</button>
  </div>

  {#if report}
    {#if errors === 0 && warnings === 0 && !catalogLoading}
      <p class="muted ok-text">{t("diag.ok.text")}</p>
    {/if}

    <div class="stack">
      {#each sorted as problem (problem.code + JSON.stringify(problem.params))}
        <ProblemCard {problem} onfixed={run} />
      {/each}
    </div>

    {#if lastLaunch}
      <details class="launch">
        <summary>{t("diag.launch.title", { game: lastLaunch.title })}</summary>
        <dl>
          <dt>{t("diag.launch.when")}</dt>
          <dd>{new Date(lastLaunch.at).toLocaleTimeString()} · {lastLaunch.runner}{lastLaunch.elevated ? ` · ${t("diag.launch.elevated")}` : ""}</dd>
          <dt>{t("diag.launch.program")}</dt>
          <dd><code>{lastLaunch.program}</code></dd>
          <dt>{t("diag.launch.command")}</dt>
          <dd><code>{lastLaunch.commandLine}</code></dd>
          <dt>{t("diag.launch.cwd")}</dt>
          <dd><code>{lastLaunch.cwd}</code></dd>
          <dt>{t("diag.launch.result")}</dt>
          <dd class:bad={!!lastLaunch.error || (lastLaunch.ended && lastLaunch.exitCode !== 0)}>{launchResult}</dd>
          <dt>{t("diag.launch.output")}</dt>
          <dd>
            <!-- Capture is off for a normal start: a script that asks a
                 question (Doom's package offers Heretic, Hexen or the GZDoom
                 launcher) would ask it into a window showing nothing. -->
            {#if lastLaunch.what === "play"}
              <button class="ghost small" onclick={() => startWithLog(lastLaunch?.gameId)}>{t("diag.launch.retry_logged")}</button>
            {:else if lastLaunch.what === "setup"}
              <button class="ghost small" onclick={() => rerunSetup(lastLaunch?.gameId)}>{t("diag.launch.rerun_setup")}</button>
            {/if}
            <!-- The console window of an ETI script is empty because its
                 output goes into this file; without it there is nothing to
                 go on when a game does not start. -->
            {#if lastLaunch.output}
              <pre>{lastLaunch.output}</pre>
            {:else if lastLaunch.captured}
              <span class="muted">{t("diag.launch.output.nothing")}</span>
            {:else if lastLaunch.elevated}
              <span class="muted">{t("diag.launch.output.elevated")}</span>
            {:else}
              <span class="muted">{t("diag.launch.output.empty")}</span>
            {/if}
          </dd>
        </dl>
      </details>
    {/if}

    {#if report.ignored.length}
      <details class="ignored">
        <summary>{t("diag.ignored", { count: report.ignored.length })}</summary>
        <div class="stack">
          {#each report.ignored as problem (problem.code + JSON.stringify(problem.params))}
            <ProblemCard {problem} ignored onfixed={run} />
          {/each}
        </div>
      </details>
    {/if}
  {/if}
</div>

<style>
  .ignored {
    margin-top: 1rem;
  }
  .ignored summary {
    cursor: pointer;
    color: var(--color-text-muted);
    font-size: 0.92rem;
    padding: 0.3rem 0;
  }
  .ignored .stack {
    margin-top: 0.6rem;
    opacity: 0.72;
  }
  .launch {
    margin-top: 1rem;
  }
  .launch summary {
    cursor: pointer;
    color: var(--color-text-muted);
    font-size: 0.92rem;
    padding: 0.3rem 0;
  }
  .launch dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.25rem 0.9rem;
    margin: 0.4rem 0 0;
    font-size: 0.88rem;
  }
  .launch dt {
    color: var(--color-text-muted);
  }
  .launch dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .launch code {
    font-size: 0.85em;
  }
  .launch pre {
    margin: 0;
    max-height: 14rem;
    overflow: auto;
    white-space: pre-wrap;
    font-size: 0.82rem;
    background: var(--color-surface-alt);
    border-radius: var(--radius);
    padding: 0.5rem 0.6rem;
    user-select: text;
  }
  .launch .bad {
    color: var(--color-warning);
  }
  .diagnostics {
    max-width: 900px;
  }
  .state {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    color: var(--color-success);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .state.warn {
    color: var(--color-warning);
  }
  .state.bad {
    color: var(--color-danger);
  }
  .state .mark {
    font-size: 1.1rem;
  }
  .ok-text {
    margin: 0.2rem 0 0.9rem;
  }
</style>
