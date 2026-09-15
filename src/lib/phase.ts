import type { GameStatus, Phase } from "./types";

export const busyPhases: Phase[] = ["queued", "syncing", "verifying", "extracting", "setup"];

export function phaseBadge(status: GameStatus | null): { label: string; cls: string } | null {
  if (!status || status.phase === "not_installed") return null;
  switch (status.phase) {
    case "ready":
      return { label: "phase.ready", cls: "ready" };
    case "update_available":
      return { label: "phase.update_available", cls: "update" };
    case "failed":
      return { label: "phase.failed", cls: "error" };
    case "paused":
      return { label: "phase.paused", cls: "warn" };
    default:
      return { label: `phase.${status.phase}`, cls: status.stalled ? "warn" : "busy" };
  }
}

export function isBusy(status: GameStatus | null): boolean {
  return !!status && busyPhases.includes(status.phase);
}

export function isPlayable(status: GameStatus | null): boolean {
  return !!status && (status.phase === "ready" || status.phase === "update_available");
}
