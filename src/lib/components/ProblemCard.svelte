<script lang="ts">
  import type { Problem } from "$lib/types";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { has, t, userText } from "$lib/i18n";
  import { formatBytes } from "$lib/format";

  let {
    problem,
    compact = false,
    ignored = false,
    onfixed,
  }: { problem: Problem; compact?: boolean; ignored?: boolean; onfixed?: () => void } = $props();

  let fixing = $state(false);

  // Byte parameters are rendered human-readable.
  const params = $derived.by(() => {
    const p: Record<string, string> = { ...problem.params };
    for (const k of Object.keys(p)) {
      if (k.endsWith("_bytes")) p[k] = formatBytes(Number(p[k]));
      if (k === "detail") p[k] = userText(p[k]);
    }
    return p;
  });

  // Hidden warnings stay in the report under "ignored", so the same button
  // brings them back.
  async function setIgnored(value: boolean) {
    if (!problem.dismiss_key) return;
    try {
      await api.setProblemIgnored(problem.dismiss_key, value);
      onfixed?.();
    } catch (e) {
      app.toast("error", t("toast.error", { detail: userText(e) }));
    }
  }

  async function fix() {
    if (!problem.fix) return;
    fixing = true;
    try {
      const msg = await api.applyFix(problem.fix);
      app.toast("success", msg ? userText(msg) : t("toast.fix_done"));
      onfixed?.();
    } catch (e) {
      app.toast("error", t("toast.error", { detail: userText(e) }));
    } finally {
      fixing = false;
    }
  }
</script>

<div class="problem {problem.severity}" class:compact>
  <div class="head">
    <span class="dot {problem.severity === 'error' ? 'error' : problem.severity === 'warning' ? 'warn' : ''}"></span>
    <strong>{t(`problem.${problem.code}.title`, params)}</strong>
    {#if problem.fix && !ignored}
      <button class="primary small" onclick={fix} disabled={fixing}>{fixing ? t("action.working") : t("action.fix_now")}</button>
    {/if}
    {#if problem.dismiss_key}
      <button class="ghost small" onclick={() => setIgnored(!ignored)} title={ignored ? t("action.unignore.hint") : t("action.ignore.hint")}>
        {ignored ? t("action.unignore") : t("action.ignore")}
      </button>
    {/if}
  </div>
  {#if has(`problem.${problem.code}.cause`)}
    <p class="cause">{t(`problem.${problem.code}.cause`, params)}</p>
  {/if}
  {#if problem.steps.length && !compact}
    <ol>
      {#each problem.steps as step (step)}
        <li>{t(`problem.${step}`, params)}</li>
      {/each}
    </ol>
  {/if}
</div>

<style>
  .problem {
    border: 1px solid var(--color-border);
    border-left: 4px solid var(--color-text-muted);
    border-radius: calc(var(--radius) * 0.7);
    padding: 0.8rem 1rem;
    background: var(--color-surface-alt);
  }
  .problem.error {
    border-left-color: var(--color-danger);
  }
  .problem.warning {
    border-left-color: var(--color-warning);
  }
  .problem.info {
    border-left-color: var(--color-primary);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .head strong {
    flex: 1;
  }
  .small {
    padding: 0.35em 0.8em;
    font-size: 0.85rem;
  }
  .cause {
    margin: 0.4rem 0 0;
    color: var(--color-text-muted);
    font-size: 0.92rem;
    user-select: text;
  }
  ol {
    margin: 0.5rem 0 0;
    padding-left: 1.3rem;
    font-size: 0.92rem;
  }
  li {
    margin: 0.2rem 0;
  }
  .compact .cause {
    font-size: 0.85rem;
  }
</style>
