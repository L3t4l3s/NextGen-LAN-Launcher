import { mount } from "svelte";
import App from "./App.svelte";
import { app } from "$lib/stores/app.svelte";
import { chat } from "$lib/stores/chat.svelte";
import { api } from "$lib/api";
import { startGamepadNavigation } from "$lib/gamepad";

void app.init();
void chat.init().catch((e) => console.warn("chat unavailable", e));
mount(App, { target: document.getElementById("app")! });
startGamepadNavigation();
// A window that opens and stays empty is the one failure the launcher cannot
// report through this interface; the backend waits for this call to know the
// difference between "came up" and "never ran".
void api.ready();
