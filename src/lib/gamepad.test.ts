import { describe, expect, it } from "vitest";
import { gamepadDirection, nextInDirection, type Box } from "./gamepad";

const box = (left: number, top: number, width = 100, height = 60): Box => ({ left, top, right: left + width, bottom: top + height });

describe("gamepad navigation", () => {
  it("moves spatially and favours controls on the same row", () => {
    const current = box(100, 100);
    const sameRow = { id: "same-row", box: box(220, 100) };
    const diagonal = { id: "diagonal", box: box(180, 260) };
    expect(nextInDirection(current, [diagonal, sameRow], "right")?.id).toBe("same-row");
    expect(nextInDirection(current, [{ id: "above", box: box(100, 10) }], "up")?.id).toBe("above");
    expect(nextInDirection(current, [sameRow], "left")).toBeNull();
  });

  it("maps the standard d-pad and dominant stick axis", () => {
    const buttons = Array(16).fill(false);
    buttons[15] = true;
    expect(gamepadDirection(buttons, [0, 0])).toBe("right");
    buttons[15] = false;
    expect(gamepadDirection(buttons, [-0.8, 0.6])).toBe("left");
    expect(gamepadDirection(buttons, [0.2, -0.8])).toBe("up");
    expect(gamepadDirection(buttons, [0.2, 0.3])).toBeNull();
  });
});
