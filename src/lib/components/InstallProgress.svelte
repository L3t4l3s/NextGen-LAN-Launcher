<script lang="ts">
  import type { GameStatus } from "$lib/types";
  import { t } from "$lib/i18n";
  import { formatPercent, percentWidth } from "$lib/format";
  import { stepProgress, steps } from "$lib/steps";

  let { status }: { status: GameStatus } = $props();

  const p = $derived(stepProgress(status));

  /** What a section says on hover: its place, its name, what happens there. */
  function hint(i: number): string {
    const step = steps[i];
    return `${t("steps.position", { index: i + 1, count: steps.length })} · ${t(`steps.${step}`)}\n${t(`steps.${step}.hint`)}`;
  }
</script>

<!-- One bar for the whole installation: every step has its own section,
     visible from the start, so the bar fills once instead of once per step. -->
<div
  class="steps"
  class:stalled={status.stalled}
  class:paused={status.phase === "paused"}
  class:failed={status.phase === "failed"}
  role="progressbar"
  aria-valuemin="0"
  aria-valuemax="100"
  aria-valuenow={Math.round(p.overall * 100)}
  aria-valuetext={p.current ? `${t(`steps.${p.current}`)} · ${formatPercent(p.overall)}` : formatPercent(p.overall)}
>
  {#each p.sections as s, i (s.step)}
    <span class="section {s.state}" class:indeterminate={s.state === "active" && p.indeterminate} style:flex-grow={s.share} title={hint(i)}>
      <span class="track"><span class="fill" style:width={percentWidth(s.fill)}></span></span>
    </span>
  {/each}
</div>

<style>
  .steps {
    display: flex;
    gap: 3px;
  }
  /* The section is the hover target, taller than the 8px it draws. */
  .section {
    flex-basis: 0;
    min-width: 0;
    padding: 5px 0;
    margin: -5px 0;
    cursor: help;
  }
  .track {
    display: block;
    position: relative;
    height: 8px;
    background: var(--color-surface-alt);
    overflow: hidden;
    transition: box-shadow 0.15s;
  }
  .section:first-child .track {
    border-radius: 999px 2px 2px 999px;
  }
  .section:last-child .track {
    border-radius: 2px 999px 999px 2px;
  }
  .section:not(:first-child):not(:last-child) .track {
    border-radius: 2px;
  }
  .section:hover .track {
    box-shadow: 0 0 0 1px var(--color-border);
  }
  .fill {
    display: block;
    height: 100%;
    background: var(--color-primary);
    transition: width 0.6s ease;
  }
  .section.done .fill {
    background: color-mix(in srgb, var(--color-primary) 70%, var(--color-success));
  }
  .section.active .track {
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-primary) 45%, transparent);
  }
  /* Checking, unpacking and setup are local work; same colours as before. */
  .section.active:not(:first-child) .fill {
    background: linear-gradient(90deg, var(--color-primary), var(--color-accent));
  }
  .steps.stalled .section.active .fill {
    background: var(--color-warning);
  }
  .steps.paused .section.active .fill {
    background: var(--color-text-muted);
  }
  .steps.failed .section.active .fill {
    background: var(--color-danger);
  }
  /* No figure to show (a setup script runs): a sweep says it is working. */
  .section.indeterminate .track::after {
    content: "";
    position: absolute;
    inset: 0;
    width: 40%;
    background: linear-gradient(90deg, transparent, color-mix(in srgb, var(--color-accent) 75%, transparent), transparent);
    animation: sweep 1.4s ease-in-out infinite;
  }
  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(250%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .section.indeterminate .track::after {
      animation: none;
      width: 100%;
      opacity: 0.5;
    }
  }
</style>
