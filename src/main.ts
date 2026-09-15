import { mount } from "svelte";
import App from "./App.svelte";
import { app } from "$lib/stores/app.svelte";

void app.init();
mount(App, { target: document.getElementById("app")! });
