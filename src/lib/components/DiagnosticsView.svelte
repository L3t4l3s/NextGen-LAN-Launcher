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

  async function run() {
    running = true;
    try {
      report = await api.diagnostics();
      lastLaunch = await api.lastLaunch();
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      running = false;
    }
  }

  // "A cmd window opens and nothing happens" is only answerable with the
  // command line in front of you, so the panel spells the whole start out.
  const launchResult = $derived.by(() => {
    const l = lastLaunch;
    if (!l) return "";
    if (l.error) return userText(l.error);
    if (!l.ended) return t("diag.launch.running", { pid: l.pid ?? 0 });
    return l.exitCode === null ? t("diag.launch.ended_unknown") : t("diag.launch.ended", { code: l.exitCode });
  });

  onMount(run);

  const errors = $derived(report?.problems.filter((p) => p.severity === "error").length ?? 0);
  const warnings = $derived(report?.problems.filter((p) => p.severity === "warning").length ?? 0);
  const sorted = $derived(
    [...(report?.problems ?? [])].sort((a, b) => ({ error: 0, warning: 1, info: 2 })[a.severity] - ({ error: 0, warning: 1, info: 2 })[b.severity]),
  );

  // `null` hides the button (no engine of ours: folder mode, demo), `""`
  // shows it greyed out while the engine is starting and has no address yet.
  const webUi = $derived(app.health?.kind === "resilio" ? (app.health.web_ui ?? "") : null);

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

<div class="page">
  <div class="row">
    <h1 class="grow">{t("diag.title")}</h1>
    <button class="ghost" onclick={() => api.openPath(app.bootstrap?.dirs.logs ?? "")}>{t("diag.logs")}</button>
    {#if webUi !== null}
      <!-- Permanently available, not only when a problem offers it as a fix:
           the engine's own page answers questions the launcher cannot. -->
      <button class="ghost" onclick={openWebUi} disabled={!webUi} title={webUi ? t("diag.web_ui.hint") : t("diag.web_ui.unavailable")}>{t("diag.web_ui")}</button>
    {/if}
    <button class="ghost" onclick={restartTransport}>{t("diag.restart_transport")}</button>
    <button class="primary" onclick={run} disabled={running}>{running ? t("diag.running") : t("diag.run")}</button>
  </div>
  <p class="hint">{t("diag.intro")}</p>

  {#if report}
    <!-- A plain line, not a card: styled like the cards below it, the
         summary read as one more problem. -->
    <div class="summary">
      <strong>
        {#if errors > 0}{t("diag.summary.error", { count: errors })}{:else if warnings > 0}{t("diag.summary.warning", { count: warnings })}{:else}{t("diag.ok.title")}{/if}
      </strong>
      {#if errors === 0 && warnings === 0}<span class="muted">{t("diag.ok.text")}</span>{/if}
      <span class="grow"></span>
      <small class="muted">{t("diag.last_run", { time: new Date(report.generatedAt).toLocaleTimeString() })}</small>
    </div>

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
  .launch .bad {
    color: var(--color-warning);
  }
  .summary {
    display: flex;
    gap: 0.6rem;
    align-items: baseline;
    margin: 0.4rem 0 0.9rem;
  }
</style>
