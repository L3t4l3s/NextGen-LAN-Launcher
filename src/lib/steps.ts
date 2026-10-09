import type { GameStatus } from "./types";
import { formatPercent } from "./format";
import { t } from "./i18n";

/**
 * The steps an installation goes through, in order. Each phase used to fill
 * the bar on its own, so one install filled it four times over; the bar now
 * covers all of them and each step owns a section of it.
 */
export type Step = "download" | "verify" | "extract" | "setup";

export const steps: Step[] = ["download", "verify", "extract", "setup"];

/**
 * How much of the bar each step gets, before the floor below. The download
 * is most of the wait: most players are on gigabit Ethernet, while checking
 * and unpacking read a local disk and most setup scripts finish in a second.
 * The other three get the floor.
 */
const weights: Record<Step, number> = { download: 70, verify: 10, extract: 10, setup: 10 };

/** No section is narrower than this, so a quick step can still be seen and hovered. */
export const MIN_SHARE = 0.1;

/**
 * Each step's share of the bar, summing to 1. Steps whose weight would leave
 * them under `min` get exactly `min`; the rest split what is left in the
 * ratio of their weights.
 */
export function shares(w: Record<Step, number> = weights, min = MIN_SHARE): Record<Step, number> {
  const floored = new Set<Step>();
  // Raising one step to the floor takes room from the others, which can push
  // another under it; repeat until nothing changes.
  for (;;) {
    const free = steps.filter((s) => !floored.has(s));
    const room = 1 - floored.size * min;
    const total = free.reduce((sum, s) => sum + Math.max(0, w[s]), 0);
    const low = free.filter((s) => total <= 0 || (Math.max(0, w[s]) / total) * room < min);
    if (low.length === 0 || low.length === free.length) {
      const out = {} as Record<Step, number>;
      for (const s of steps) {
        out[s] = floored.has(s) ? min : low.length ? room / free.length : (Math.max(0, w[s]) / total) * room;
      }
      return out;
    }
    for (const s of low) floored.add(s);
  }
}

const layout = shares();

/** The step a phase belongs to; null for the phases outside an installation. */
export function stepOf(status: GameStatus): Step | null {
  switch (status.phase) {
    case "queued":
    case "syncing":
    case "paused":
      return "download";
    case "verifying":
      return "verify";
    case "extracting":
      return "extract";
    case "setup":
      return "setup";
    case "failed":
      return failedStep(status.problem?.code);
    default:
      return null;
  }
}

/**
 * Which step a failure stopped at, as far as its code tells. Only codes the
 * backend raises together with the failed phase: a full disk while loading is
 * a warning beside a running download, so a failed one comes from unpacking.
 * A receipt that cannot be written fails after unpacking and when adopting an
 * existing install alike, so it names no step; nor does a setup script, whose
 * failure leaves the game ready.
 */
function failedStep(code: string | undefined): Step | null {
  if (!code) return null;
  if (code.startsWith("sync.")) return "download";
  if (code === "install.verify_error" || code === "install.unsafe_archive") return "verify";
  if (code === "install.extract_error" || code === "install.disk_full") return "extract";
  return null;
}

export interface StepSection {
  step: Step;
  /** Share of the whole bar, 0–1. */
  share: number;
  /** How far this section is filled, 0–1. */
  fill: number;
  state: "done" | "active" | "pending";
}

export interface StepProgress {
  sections: StepSection[];
  /** The current step, null when none can be named (an unknown failure). */
  current: Step | null;
  /** 1-based position of the current step, 0 without one. */
  index: number;
  /** The whole installation, 0–1. */
  overall: number;
  /**
   * The current step reports no fraction of its own (a setup script runs),
   * so its section shows motion instead of a fill.
   */
  indeterminate: boolean;
}

export function stepProgress(status: GameStatus): StepProgress {
  const current = stepOf(status);
  const at = current ? steps.indexOf(current) : -1;
  const own = Number.isFinite(status.progress) ? Math.min(1, Math.max(0, status.progress)) : 0;
  // Setup has no measure of its own and stays at 0 until it is done.
  const indeterminate = status.phase === "setup";
  // A failed status carries the download's figure, whatever step stopped;
  // the section that failed is shown whole, in the error colour.
  const failed = status.phase === "failed";
  const sections = steps.map((step, i): StepSection => {
    const state = i < at ? "done" : i === at ? "active" : "pending";
    const fill = state === "done" || (state === "active" && failed) ? 1 : state === "active" && !indeterminate ? own : 0;
    return { step, share: layout[step], fill, state };
  });
  const overall = sections.reduce((sum, s) => sum + s.share * s.fill, 0);
  return { sections, current, index: at + 1, overall, indeterminate };
}

/**
 * The line under a bar: which step of how many, and how far that step is —
 * the bar shows the whole, the text the part the step's own figure covers.
 */
export function stepLabel(status: GameStatus, short = false): string {
  const p = stepProgress(status);
  // A failure without a step: the figure it carries, as before the steps.
  if (!p.current) return formatPercent(status.progress);
  // A tile has room for "2/4", not for the sentence; the bar's hover names the step.
  const where = short ? `${p.index}/${steps.length}` : t("steps.position", { index: p.index, count: steps.length });
  return p.indeterminate || status.phase === "failed" ? where : `${where} · ${formatPercent(status.progress)}`;
}
