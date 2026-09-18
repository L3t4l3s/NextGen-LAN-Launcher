export type Direction = "up" | "down" | "left" | "right";

export interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

const FOCUSABLE = [
  "button:not(:disabled)",
  "a[href]",
  "input:not(:disabled)",
  "select:not(:disabled)",
  "textarea:not(:disabled)",
  "summary",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

const center = (box: Box) => ({ x: (box.left + box.right) / 2, y: (box.top + box.bottom) / 2 });

/** Pick the closest control in the requested half-plane, favouring the same row or column. */
export function nextInDirection<T extends { box: Box }>(current: Box, candidates: T[], direction: Direction): T | null {
  const from = center(current);
  let best: { candidate: T; score: number } | null = null;
  for (const candidate of candidates) {
    const to = center(candidate.box);
    const dx = to.x - from.x;
    const dy = to.y - from.y;
    const primary = direction === "left" ? -dx : direction === "right" ? dx : direction === "up" ? -dy : dy;
    if (primary <= 1) continue;
    const cross = direction === "left" || direction === "right" ? Math.abs(dy) : Math.abs(dx);
    // A control only slightly in the requested direction but far off-axis is
    // less natural than the next card in the same row.
    const score = primary + cross * 2.5;
    if (!best || score < best.score) best = { candidate, score };
  }
  return best?.candidate ?? null;
}

export function gamepadDirection(buttons: readonly boolean[], axes: readonly number[]): Direction | null {
  const x = Math.abs(axes[0] ?? 0) >= 0.55 ? axes[0] : 0;
  const y = Math.abs(axes[1] ?? 0) >= 0.55 ? axes[1] : 0;
  if (buttons[12] || (y < 0 && Math.abs(y) >= Math.abs(x))) return "up";
  if (buttons[13] || (y > 0 && Math.abs(y) >= Math.abs(x))) return "down";
  if (buttons[14] || x < 0) return "left";
  if (buttons[15] || x > 0) return "right";
  return null;
}

function visible(element: Element): element is HTMLElement {
  if (!(element instanceof HTMLElement) || element.closest("[inert], [aria-hidden='true']")) return false;
  const style = getComputedStyle(element);
  const box = element.getBoundingClientRect();
  return style.display !== "none" && style.visibility !== "hidden" && box.width > 0 && box.height > 0;
}

function activeScope(): ParentNode {
  const overlays = [...document.querySelectorAll(".modal-backdrop")].filter(visible);
  return overlays[overlays.length - 1] ?? document;
}

function controls(scope = activeScope()): HTMLElement[] {
  return [...scope.querySelectorAll(FOCUSABLE)].filter(visible);
}

function focusWithGamepad(element: HTMLElement) {
  document.querySelector(".gamepad-focus")?.classList.remove("gamepad-focus");
  element.classList.add("gamepad-focus");
  element.focus({ preventScroll: true });
}

function focusInitial(items: HTMLElement[]) {
  const preferred =
    items.find((item) => item.matches("[data-gamepad-default]")) ??
    items.find((item) => item.matches(".tile.selected")) ??
    items.find((item) => item.matches(".tile")) ??
    items[0];
  if (preferred) focusWithGamepad(preferred);
  preferred?.scrollIntoView({ block: "nearest", inline: "nearest" });
}

function move(direction: Direction) {
  const scope = activeScope();
  const items = controls(scope);
  const current = document.activeElement instanceof HTMLElement && items.includes(document.activeElement) ? document.activeElement : null;
  if (!current) return focusInitial(items);
  const candidates = items.filter((item) => item !== current).map((element) => ({ element, box: element.getBoundingClientRect() }));
  const next = nextInDirection(current.getBoundingClientRect(), candidates, direction)?.element;
  if (next) {
    focusWithGamepad(next);
    next.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
  }
}

function activate(): HTMLSelectElement | null {
  const scope = activeScope();
  const current = document.activeElement;
  if (current instanceof HTMLSelectElement && controls(scope).includes(current)) return current;
  if (current instanceof HTMLElement && controls(scope).includes(current)) current.click();
  else focusInitial(controls(scope));
  return null;
}

function changeSelect(select: HTMLSelectElement, direction: Direction) {
  const delta = direction === "up" || direction === "left" ? -1 : 1;
  const enabled = [...select.options].filter((option) => !option.disabled);
  const current = enabled.findIndex((option) => option.index === select.selectedIndex);
  const next = enabled[Math.max(0, Math.min(enabled.length - 1, current + delta))];
  if (!next || next.index === select.selectedIndex) return;
  select.selectedIndex = next.index;
  select.dispatchEvent(new Event("input", { bubbles: true }));
  select.dispatchEvent(new Event("change", { bubbles: true }));
}

function back() {
  const scope = activeScope();
  const current = document.activeElement;
  if (current instanceof HTMLInputElement || current instanceof HTMLTextAreaElement || current instanceof HTMLSelectElement) {
    current.blur();
  }
  const targets = [...scope.querySelectorAll("[data-gamepad-back]")].filter(visible);
  targets[targets.length - 1]?.click();
}

/** Enable standard-layout controllers without changing mouse or keyboard behaviour. */
export function startGamepadNavigation(): () => void {
  let frame = 0;
  let previousA = false;
  let previousB = false;
  let heldDirection: Direction | null = null;
  let nextRepeat = 0;
  let editingSelect: HTMLSelectElement | null = null;
  const stopEditingSelect = () => {
    editingSelect?.classList.remove("gamepad-editing");
    editingSelect = null;
  };
  const clearControllerFocus = () => {
    document.querySelector(".gamepad-focus")?.classList.remove("gamepad-focus");
    stopEditingSelect();
  };
  const trackFocusChange = (event: FocusEvent) => {
    if (editingSelect && event.target !== editingSelect) stopEditingSelect();
  };
  document.addEventListener("pointerdown", clearControllerFocus, true);
  document.addEventListener("focusin", trackFocusChange, true);

  const poll = (now: number) => {
    if (document.hasFocus()) {
      const samples = (navigator.getGamepads?.() ?? [])
        .filter((item): item is Gamepad => !!item && item.connected)
        .map((pad) => {
          const pressed = pad.buttons.map((button) => button.pressed);
          return { pressed, direction: gamepadDirection(pressed, pad.axes) };
        });
      const sample = samples.find(({ pressed, direction }) => !!pressed[0] || !!pressed[1] || !!direction) ?? samples[0];
      if (sample) {
        const { pressed, direction } = sample;
        const a = !!pressed[0];
        const b = !!pressed[1];
        if (a && !previousA) {
          if (editingSelect) {
            editingSelect.classList.remove("gamepad-editing");
            editingSelect = null;
          } else {
            editingSelect = activate();
            editingSelect?.classList.add("gamepad-editing");
          }
        }
        if (b && !previousB) {
          if (editingSelect) {
            editingSelect.classList.remove("gamepad-editing");
            editingSelect = null;
          } else back();
        }
        previousA = a;
        previousB = b;

        // Once any direction is held, every further movement observes the
        // repeat timer. Analog noise near a diagonal must not turn rapid
        // left/up changes into one focus move per animation frame.
        if (direction && (heldDirection === null || now >= nextRepeat)) {
          if (editingSelect?.isConnected) changeSelect(editingSelect, direction);
          else {
            editingSelect?.classList.remove("gamepad-editing");
            editingSelect = null;
            move(direction);
          }
          nextRepeat = now + (heldDirection === null ? 320 : 110);
        }
        heldDirection = direction;
        if (!direction) nextRepeat = 0;
      } else {
        editingSelect?.classList.remove("gamepad-editing");
        editingSelect = null;
        previousA = previousB = false;
        heldDirection = null;
      }
    }
    frame = requestAnimationFrame(poll);
  };
  frame = requestAnimationFrame(poll);
  return () => {
    stopEditingSelect();
    clearControllerFocus();
    document.removeEventListener("pointerdown", clearControllerFocus, true);
    document.removeEventListener("focusin", trackFocusChange, true);
    cancelAnimationFrame(frame);
  };
}
