import { describe, expect, it } from "vitest";
import { MIN_SHARE, shares, stepLabel, stepProgress, steps } from "./steps";
import type { GameStatus, Phase } from "./types";

function status(phase: Phase, progress = 0, code?: string): GameStatus {
  return {
    gameId: "g",
    phase,
    progress,
    bytesDone: 0,
    bytesTotal: 0,
    peers: 0,
    downloadBps: 0,
    installedRevision: null,
    catalogRevision: "1",
    stalled: false,
    problem: code ? { code, severity: "error", params: {}, steps: [] } : null,
    needsExeChoice: false,
    updatedAt: "",
  };
}

const sum = (r: Record<string, number>) => Object.values(r).reduce((a, b) => a + b, 0);

describe("install steps", () => {
  it("gives the download the most room and every step at least the floor", () => {
    const s = shares();
    expect(sum(s)).toBeCloseTo(1);
    for (const step of steps) expect(s[step]).toBeGreaterThanOrEqual(MIN_SHARE - 1e-9);
    for (const step of steps) if (step !== "download") expect(s.download).toBeGreaterThan(s[step]);
    expect(s.download).toBeCloseTo(0.7);
    expect(s.verify).toBeCloseTo(0.1);
    expect(s.extract).toBeCloseTo(0.1);
    expect(s.setup).toBeCloseTo(0.1);
  });

  it("lifts small steps to the floor and keeps the ratio among the rest", () => {
    const s = shares({ download: 80, verify: 11, extract: 5, setup: 4 }, 0.1);
    expect(s.extract).toBeCloseTo(0.1);
    expect(s.setup).toBeCloseTo(0.1);
    // Verify starts at 0.11, but the 0.8 left after lifting the other two,
    // split 80:11, puts it at 0.097: under the floor in the second round.
    expect(s.verify).toBeCloseTo(0.1);
    expect(s.download).toBeCloseTo(0.7);
    expect(sum(s)).toBeCloseTo(1);
  });

  it("splits evenly when no weight is given", () => {
    const s = shares({ download: 0, verify: 0, extract: 0, setup: 0 });
    for (const step of steps) expect(s[step]).toBeCloseTo(0.25);
  });

  it("fills the bar once across all steps instead of once per step", () => {
    const layout = shares();
    expect(stepProgress(status("syncing", 0.5)).overall).toBeCloseTo(layout.download * 0.5);
    const verifying = stepProgress(status("verifying", 0.5));
    expect(verifying.index).toBe(2);
    expect(verifying.overall).toBeCloseTo(layout.download + layout.verify * 0.5);
    const setup = stepProgress(status("setup", 0));
    expect(setup.indeterminate).toBe(true);
    expect(setup.sections.map((s) => s.state)).toEqual(["done", "done", "done", "active"]);
  });

  it("counts a paused download and the queue as the download step", () => {
    expect(stepProgress(status("paused", 0.3)).current).toBe("download");
    // A restart queues a half-done download first; it must not drop to empty.
    const queued = stepProgress(status("queued", 0.6));
    expect(queued.current).toBe("download");
    expect(queued.sections[0].fill).toBeCloseTo(0.6);
  });

  it("marks the step a failure stopped at, where its code says", () => {
    const failed = stepProgress(status("failed", 0.99, "install.extract_error"));
    expect(failed.current).toBe("extract");
    expect(failed.sections.map((s) => s.fill)).toEqual([1, 1, 1, 0]);
    expect(stepProgress(status("failed", 1, "install.disk_full")).current).toBe("extract");
    const unknown = stepProgress(status("failed", 0.99, "install.receipt_error"));
    expect(unknown.current).toBeNull();
    expect(unknown.overall).toBe(0);
    expect(stepLabel(status("failed", 0.99, "install.receipt_error"))).toBe("99 %");
  });
});

describe("step label", () => {
  it("names the step and the step's own figure", () => {
    expect(stepLabel(status("verifying", 0.45))).toBe("Schritt 2 von 4 · 45 %");
    expect(stepLabel(status("syncing", 0.2), true)).toBe("1/4 · 20 %");
    expect(stepLabel(status("setup"))).toBe("Schritt 4 von 4");
  });
});
