<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api, pickFolder } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import type { Report, Settings } from "$lib/types";
  import ProblemCard from "./ProblemCard.svelte";
  import { cleanRoots, defaultRootIndex, setupRoots } from "$lib/library";

  let step = $state(1);
  const platform = app.bootstrap?.platform ?? "windows";
  let roots = $state(setupRoots(app.settings?.library.roots ?? [], platform));
  let name = $state(app.settings?.playerName ?? "");
  let report = $state<Report | null>(null);
  let checking = $state(false);
  let checkInFlight = false;

  async function choose(index: number) {
    const p = await pickFolder();
    if (p) roots[index].path = p;
  }

  function removeRoot(index: number) {
    roots = roots.filter((_, i) => i !== index);
    roots = setupRoots(roots, platform);
  }

  function makeDefault(index: number) {
    roots = roots.map((root, i) => ({ ...root, isDefault: i === index }));
  }

  async function toStep3() {
    if (!app.settings) return;
    const next: Settings = structuredClone($state.snapshot(app.settings)) as Settings;
    next.library.roots = cleanRoots(roots, platform);
    next.playerName = name.trim();
    try {
      await app.saveSettings(next);
    } catch (e) {
      app.toast("error", userText(e));
      return;
    }
    step = 3;
    await check();
  }

  async function check(background = false) {
    if (checkInFlight) return;
    checkInFlight = true;
    if (!background) checking = true;
    try {
      report = await api.diagnostics();
    } catch (e) {
      if (!background) app.toast("error", userText(e));
    } finally {
      checking = false;
      checkInFlight = false;
    }
  }

  const blocking = $derived(report?.problems.filter((p) => p.severity !== "info") ?? []);
  const catalogLoading = $derived(report?.problems.find((p) => p.code === "catalog.loading"));
  $effect(() => {
    if (step !== 3 || !report?.problems.some((p) => p.code === "catalog.loading" || p.code === "catalog.missing")) return;
    const timer = setInterval(() => void check(true), 10_000);
    return () => clearInterval(timer);
  });

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
      <div class="stack root-list">
        {#each roots as root, index (index)}
          <div class="root-row">
            <div class="row">
              <input class="grow" bind:value={root.path} aria-label={t("settings.library")} />
              <button onclick={() => choose(index)}>{t("wizard.step1.choose")}</button>
            </div>
            <div class="row">
              {#if index === defaultRootIndex(roots)}
                <span class="badge ready">{t("settings.library.default")}</span>
              {:else}
                <button class="ghost" onclick={() => makeDefault(index)}>{t("settings.library.make_default")}</button>
              {/if}
              <button class="ghost" onclick={() => removeRoot(index)} disabled={roots.length === 1}>{t("settings.library.remove")}</button>
            </div>
          </div>
        {/each}
      </div>
      <button onclick={() => roots.push({ path: "", label: "", isDefault: false })}>+ {t("settings.library.add")}</button>
      <div class="row end"><button class="primary" onclick={() => (step = 2)} disabled={!roots.some((root) => root.path.trim())}>{t("action.next")}</button></div>
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
        {#if catalogLoading}
          <ProblemCard problem={catalogLoading} />
        {/if}
        {#if blocking.length === 0 && !catalogLoading}
          <p class="ok">✔ {t("wizard.step3.all_good")}</p>
        {:else if blocking.length > 0}
          <p class="muted">{t("wizard.step3.problems")}</p>
          <div class="stack problems">
            {#each blocking as problem (problem.code + JSON.stringify(problem.params))}
              <ProblemCard {problem} onfixed={check} />
            {/each}
          </div>
        {/if}
      {/if}
      <div class="row end">
        <button onclick={() => check()} disabled={checking}>{t("action.retry")}</button>
        <button class="primary" onclick={finish}>{t("action.finish")}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .root-list { max-height: 40vh; overflow: auto; }
  .root-row { display: grid; gap: 0.3rem; }
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
