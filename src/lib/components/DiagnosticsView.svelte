<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import type { Report } from "$lib/types";
  import ProblemCard from "./ProblemCard.svelte";
  import { onMount } from "svelte";

  let report = $state<Report | null>(null);
  let running = $state(false);

  async function run() {
    running = true;
    try {
      report = await api.diagnostics();
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      running = false;
    }
  }

  onMount(run);

  const errors = $derived(report?.problems.filter((p) => p.severity === "error").length ?? 0);
  const warnings = $derived(report?.problems.filter((p) => p.severity === "warning").length ?? 0);
  const sorted = $derived(
    [...(report?.problems ?? [])].sort((a, b) => ({ error: 0, warning: 1, info: 2 })[a.severity] - ({ error: 0, warning: 1, info: 2 })[b.severity]),
  );

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
    <button class="ghost" onclick={restartTransport}>{t("diag.restart_transport")}</button>
    <button class="primary" onclick={run} disabled={running}>{running ? t("diag.running") : t("diag.run")}</button>
  </div>
  <p class="hint">{t("diag.intro")}</p>

  {#if report}
    <div class="summary card" class:error={errors > 0} class:warn={errors === 0 && warnings > 0} class:ok={errors === 0 && warnings === 0}>
      <span class="dot {errors > 0 ? 'error' : warnings > 0 ? 'warn' : 'ok'}"></span>
      <div class="grow">
        <strong>
          {#if errors > 0}{t("diag.summary.error", { count: errors })}{:else if warnings > 0}{t("diag.summary.warning", { count: warnings })}{:else}{t("diag.ok.title")}{/if}
        </strong>
        <div class="hint">{errors === 0 && warnings === 0 ? t("diag.ok.text") : t("diag.checks", { list: report.checksRun.join(", ") })}</div>
      </div>
      <small class="muted">{t("diag.last_run", { time: new Date(report.generatedAt).toLocaleTimeString() })}</small>
    </div>

    <div class="stack">
      {#each sorted as problem (problem.code + JSON.stringify(problem.params))}
        <ProblemCard {problem} onfixed={run} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .summary {
    display: flex;
    gap: 1rem;
    align-items: center;
    margin-bottom: 1rem;
    border-left: 4px solid var(--color-text-muted);
  }
  .summary.error {
    border-left-color: var(--color-danger);
  }
  .summary.warn {
    border-left-color: var(--color-warning);
  }
  .summary.ok {
    border-left-color: var(--color-success);
  }
</style>
