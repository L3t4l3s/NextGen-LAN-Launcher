<script lang="ts">
  import { t } from "$lib/i18n";

  // One live series (the download rate of a single game), so no legend: the
  // caption names it. Only the current value and the peak are labelled; the
  // rest is read by hovering, which also keeps the chart free of numbers.
  let {
    values,
    format,
    label,
    sampleSeconds = 2,
  }: {
    values: number[];
    format: (v: number) => string;
    label: string;
    /** Seconds between two samples, for the "x seconds ago" readout. */
    sampleSeconds?: number;
  } = $props();

  const WIDTH = 260;
  const HEIGHT = 46;
  const PAD = 3;

  let hovered = $state<number | null>(null);

  const peak = $derived(values.reduce((a, b) => Math.max(a, b), 0));
  const current = $derived(values.length ? values[values.length - 1] : 0);
  // Headroom above the peak so a constant series reads as a line, not as a
  // filled box; a flat zero series would divide by zero, hence the floor.
  const scale = $derived(Math.max(peak * 1.15, 1));
  const x = (i: number) => PAD + (values.length < 2 ? 0 : (i * (WIDTH - 2 * PAD)) / (values.length - 1));
  const y = (v: number) => HEIGHT - PAD - (v / scale) * (HEIGHT - 2 * PAD);
  const line = $derived(values.map((v, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join(" "));
  const area = $derived(
    values.length < 2 ? "" : `${line} L${x(values.length - 1).toFixed(1)} ${HEIGHT - PAD} L${x(0).toFixed(1)} ${HEIGHT - PAD} Z`,
  );
  // The series grows and is trimmed while the pointer rests on it, so the
  // index is clamped on every read instead of only on pointer move.
  const cursor = $derived(hovered === null ? null : Math.min(hovered, values.length - 1));
  const shown = $derived(cursor !== null && values[cursor] !== undefined ? values[cursor] : current);
  const age = $derived(cursor === null ? 0 : (values.length - 1 - cursor) * sampleSeconds);

  function track(event: PointerEvent) {
    if (values.length < 2) return;
    const box = (event.currentTarget as SVGElement).getBoundingClientRect();
    const share = (event.clientX - box.left) / box.width;
    hovered = Math.min(values.length - 1, Math.max(0, Math.round(share * (values.length - 1))));
  }
</script>

<figure class="spark">
  <figcaption>
    <span class="muted">{label}</span>
    <strong>{format(shown)}</strong>
    {#if cursor !== null && age > 0}<span class="muted">{t("chart.ago", { seconds: age })}</span>{/if}
    {#if peak > 0}<span class="muted grow">{t("chart.peak", { value: format(peak) })}</span>{/if}
  </figcaption>
  <svg
    viewBox="0 0 {WIDTH} {HEIGHT}"
    preserveAspectRatio="none"
    role="img"
    aria-label="{label}: {format(current)}, {t('chart.peak', { value: format(peak) })}"
    onpointermove={track}
    onpointerleave={() => (hovered = null)}
  >
    {#if values.length >= 2}
      <path class="area" d={area} />
      <path class="line" d={line} />
      {#if cursor !== null}
        <line class="cursor" x1={x(cursor)} y1={PAD} x2={x(cursor)} y2={HEIGHT - PAD} />
        <circle class="dot" cx={x(cursor)} cy={y(values[cursor])} r="3.5" />
      {:else}
        <circle class="dot" cx={x(values.length - 1)} cy={y(current)} r="3.5" />
      {/if}
    {:else}
      <line class="baseline" x1={PAD} y1={HEIGHT - PAD} x2={WIDTH - PAD} y2={HEIGHT - PAD} />
    {/if}
  </svg>
</figure>

<style>
  .spark {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  figcaption {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
    font-size: 0.8rem;
  }
  .grow {
    margin-left: auto;
  }
  svg {
    width: 100%;
    height: 46px;
    display: block;
    touch-action: none;
  }
  /* One series, one hue; thin marks and no gridlines. */
  .line {
    fill: none;
    stroke: var(--color-primary);
    stroke-width: 2;
    stroke-linejoin: round;
    stroke-linecap: round;
    vector-effect: non-scaling-stroke;
  }
  .area {
    fill: color-mix(in srgb, var(--color-primary) 18%, transparent);
    stroke: none;
  }
  .dot {
    fill: var(--color-primary);
    stroke: var(--color-surface);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  .cursor,
  .baseline {
    stroke: var(--color-border);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
</style>
