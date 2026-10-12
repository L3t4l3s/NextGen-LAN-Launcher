import { mount } from "svelte";
import App from "./App.svelte";
import { app } from "$lib/stores/app.svelte";
import { chat } from "$lib/stores/chat.svelte";
import { api } from "$lib/api";
import { startGamepadNavigation } from "$lib/gamepad";

void app.init();
void chat.init().catch((e) => console.warn("chat unavailable", e));
// The webview's own menu (reload, save, print, share) has no business in a
// launcher. Text fields keep theirs for cut, copy and paste; chat messages
// open their own. The dev build keeps it for the inspector.
if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (e) => {
    if (e.target instanceof Element && e.target.closest("input, textarea, [contenteditable='true']")) return;
    e.preventDefault();
  });
}
// Nothing in the launcher is dragged. A cover or link that starts to be
// dragged hands the pointer to GTK's drag-and-drop, and a drag that never
// ends (seen on Bazzite through Sunshine: its 1x1 helper windows stayed at
// -99,-99 carrying the image's types) leaves the whole window deaf to clicks
// and keys until the launcher restarts. So no drag starts, except selected
// text in a text field.
window.addEventListener(
  "dragstart",
  (e) => {
    if (e.target instanceof Element && e.target.closest("input, textarea, [contenteditable='true']")) return;
    e.preventDefault();
  },
  true,
);
mount(App, { target: document.getElementById("app")! });
startGamepadNavigation();
// A window that opens and stays empty is the one failure the launcher cannot
// report through this interface; the backend waits for this call to know the
// difference between "came up" and "never ran".
void api.ready();
