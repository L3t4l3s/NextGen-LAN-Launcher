<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api, pickFolder } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import type { Report, Settings } from "$lib/types";
  import ProblemCard from "./ProblemCard.svelte";

  let step = $state(1);
  let folder = $state(app.settings?.library.roots[0]?.path ?? "");
  let name = $state(app.settings?.playerName ?? "");
  let report = $state<Report | null>(null);
  let checking = $state(false);

  async function choose() {
    const p = await pickFolder();
    if (p) folder = p;
  }

  async function toStep3() {
    if (!app.settings) return;
    const next: Settings = structuredClone($state.snapshot(app.settings)) as Settings;
    next.library.roots = [{ path: folder, label: folder, isDefault: true }];
    next.playerName = name;
    try {
      await app.saveSettings(next);
    } catch (e) {
      app.toast("error", userText(e));
      return;
    }
    step = 3;
    await check();
  }

  async function check() {
    checking = true;
    try {
      report = await api.diagnostics();
    } finally {
      checking = false;
    }
  }

  const blocking = $derived(report?.problems.filter((p) => p.severity !== "info") ?? []);

  async function finish() {
    if (!app.settings) return;
    const next = structuredClone($state.snapshot(app.settings)) as Settings;
    next.setupComplete = true;
    await app.saveSettings(next);
    app.showWizard = false;
  }
</script>

<div class="modal-backdrop">
  <div class="modal card wizard">
    <div class="steps">
      {#each [1, 2, 3] as s (s)}<span class:done={s < step} class:current={s === step}>{s}</span>{/each}
    </div>

    {#if step === 1}
      <h2>{t("wizard.title")}</h2>
      <p class="hint">{t("wizard.intro")}</p>
      <h3>{t("wizard.step1.title")}</h3>
      <p class="hint">{t("wizard.step1.text")}</p>
      <div class="row">
        <input class="grow" bind:value={folder} placeholder="D:\LAN" />
        <button onclick={choose}>{t("wizard.step1.choose")}</button>
      </div>
      <div class="row end"><button class="primary" onclick={() => (step = 2)} disabled={!folder.trim()}>{t("action.next")}</button></div>
    {:else if step === 2}
      <h3>{t("wizard.step2.title")}</h3>
      <p class="hint">{t("wizard.step2.text")}</p>
      <input bind:value={name} maxlength="32" placeholder="Player" />
      <div class="row end">
        <button onclick={() => (step = 1)}>{t("action.back")}</button>
        <button class="primary" onclick={toStep3} disabled={!name.trim()}>{t("action.next")}</button>
      </div>
    {:else}
      <h3>{t("wizard.step3.title")}</h3>
      <p class="hint">{t("wizard.step3.text")}</p>
      {#if checking}
        <p class="muted">{t("diag.running")}</p>
      {:else if report}
        {#if blocking.length === 0}
          <p class="ok">✔ {t("wizard.step3.all_good")}</p>
        {:else}
          <p class="muted">{t("wizard.step3.problems")}</p>
          <div class="stack problems">
            {#each blocking as problem (problem.code + JSON.stringify(problem.params))}
              <ProblemCard {problem} compact onfixed={check} />
            {/each}
          </div>
        {/if}
      {/if}
      <div class="row end">
        <button onclick={check} disabled={checking}>{t("action.retry")}</button>
        <button class="primary" onclick={finish}>{t("action.finish")}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .wizard {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .steps {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }
  .steps span {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--color-surface-alt);
    color: var(--color-text-muted);
    font-weight: 700;
    font-size: 0.85rem;
  }
  .steps .current {
    background: var(--color-primary);
    color: var(--color-primary-text);
  }
  .steps .done {
    background: var(--color-success);
    color: var(--color-success-text, #08140c);
  }
  input {
    width: 100%;
  }
  .end {
    justify-content: flex-end;
    margin-top: 0.5rem;
  }
  .ok {
    color: var(--color-success);
    font-weight: 600;
  }
  .problems {
    max-height: 45vh;
    overflow: auto;
  }
</style>
