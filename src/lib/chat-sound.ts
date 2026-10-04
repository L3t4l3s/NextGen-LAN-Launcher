// Notification sounds, synthesised: no audio files to ship or license.

let ctx: AudioContext | null = null;
let lastPlayed = 0;

function context(): AudioContext | null {
  if (typeof window === "undefined" || !("AudioContext" in window)) return null;
  ctx ??= new AudioContext();
  return ctx;
}

/** Webviews only allow audio after the user did something; the first click
 *  or key press unlocks it. */
export function unlockAudio() {
  const resume = () => {
    void context()?.resume().catch(() => {});
  };
  window.addEventListener("pointerdown", resume, { once: true, capture: true });
  window.addEventListener("keydown", resume, { once: true, capture: true });
}

export type Sound = "message" | "direct";

/** A soft two-note chime; a private message or mention rises higher. A burst
 *  of messages plays once, not once each. */
export function playSound(kind: Sound) {
  const now = Date.now();
  if (now - lastPlayed < 1200) return;
  lastPlayed = now;
  const ac = context();
  if (!ac || ac.state !== "running") return;
  const notes = kind === "direct" ? [784, 1175] : [659, 880];
  const start = ac.currentTime;
  notes.forEach((freq, i) => {
    const osc = ac.createOscillator();
    const gain = ac.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    const t0 = start + i * 0.11;
    gain.gain.setValueAtTime(0, t0);
    gain.gain.linearRampToValueAtTime(0.18, t0 + 0.015);
    gain.gain.exponentialRampToValueAtTime(0.001, t0 + 0.32);
    osc.connect(gain).connect(ac.destination);
    osc.start(t0);
    osc.stop(t0 + 0.35);
  });
}
