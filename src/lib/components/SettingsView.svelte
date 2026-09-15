<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api, pickFolder } from "$lib/api";
  import { languages, t, userText } from "$lib/i18n";
  import { formatBytes } from "$lib/format";
  import type { LibrarySpace, Settings } from "$lib/types";
  import { onMount } from "svelte";

  let draft = $state<Settings | null>(null);
  let space = $state<LibrarySpace[]>([]);
  let saving = $state(false);
  let showAdvanced = $state(false);

  onMount(async () => {
    draft = structuredClone($state.snapshot(app.settings)) as Settings;
    space = await api.librarySpace().catch(() => []);
  });

  async function addRoot() {
    const path = await pickFolder();
    if (!path || !draft) return;
    if (draft.library.roots.some((r) => r.path === path)) return;
    draft.library.roots.push({ path, label: path, isDefault: draft.library.roots.length === 0 });
    await save();
    space = await api.librarySpace().catch(() => []);
  }

  function makeDefault(path: string) {
    if (!draft) return;
    for (const r of draft.library.roots) r.isDefault = r.path === path;
  }

  function removeRoot(path: string) {
    if (!draft) return;
    const wasDefault = draft.library.roots.find((r) => r.path === path)?.isDefault;
    draft.library.roots = draft.library.roots.filter((r) => r.path !== path);
    if (wasDefault && draft.library.roots[0]) draft.library.roots[0].isDefault = true;
  }

  async function save() {
    if (!draft) return;
    saving = true;
    try {
      const saved = await app.saveSettings($state.snapshot(draft) as Settings);
      draft = structuredClone(saved);
      app.toast("success", t("settings.saved"));
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      saving = false;
    }
  }

  const spaceFor = (path: string) => space.find((s) => s.path === path);
</script>

<div class="page">
  <h1>{t("settings.title")}</h1>

  {#if draft}
    <div class="stack">
      <section class="card">
        <label for="player">{t("settings.player")}</label>
        <input id="player" bind:value={draft.playerName} maxlength="32" />
        <p class="hint">{t("settings.player.hint")}</p>
        <div class="two">
          <div>
            <label for="lang">{t("settings.language")}</label>
            <select id="lang" bind:value={draft.language}>
              {#each languages as l (l.id)}<option value={l.id}>{l.label}</option>{/each}
            </select>
          </div>
          <div>
            <label for="glang">{t("settings.game_language")}</label>
            <select id="glang" bind:value={draft.gameLanguage}>
              <option value="de">Deutsch</option>
              <option value="en">English</option>
              <option value="fr">Français</option>
            </select>
          </div>
        </div>
      </section>

      <section class="card">
        <div class="row">
          <h2 class="grow">{t("settings.library")}</h2>
          <button onclick={addRoot} disabled={app.bootstrap?.demo}>+ {t("settings.library.add")}</button>
        </div>
        <p class="hint">{t("settings.library.hint")}</p>
        <ul class="roots">
          {#each draft.library.roots as root (root.path)}
            {@const s = spaceFor(root.path)}
            <li>
              <div class="grow">
                <strong>{root.path}</strong>
                {#if root.isDefault}<span class="badge ready">{t("settings.library.default")}</span>{/if}
                <div class="hint">
                  {#if s?.freeBytes != null}
                    {t("settings.library.free", { free: formatBytes(s.freeBytes), total: formatBytes(s.totalBytes ?? 0) })} · {t("settings.library.games", { count: s.games })}
                  {:else if s}
                    {t("settings.library.unknown")}
                  {/if}
                </div>
                {#if s?.freeBytes != null && s.totalBytes}
                  <div class="progress"><span style:width={`${Math.round((1 - s.freeBytes / s.totalBytes) * 100)}%`}></span></div>
                {/if}
              </div>
              {#if !root.isDefault}<button class="ghost" onclick={() => makeDefault(root.path)}>{t("settings.library.make_default")}</button>{/if}
              <button class="ghost danger" onclick={() => removeRoot(root.path)} disabled={app.bootstrap?.demo}>{t("settings.library.remove")}</button>
            </li>
          {/each}
        </ul>
      </section>

      <section class="card">
        <h2>{t("settings.transport")}</h2>
        <div class="stack radios">
          <label class="radio"><input type="radio" bind:group={draft.transport} value="managed" disabled={app.bootstrap?.demo} /> {t("settings.transport.managed")}</label>
          <label class="radio"><input type="radio" bind:group={draft.transport} value="folder" disabled={app.bootstrap?.demo} /> {t("settings.transport.folder")}</label>
          {#if app.bootstrap?.demo}<label class="radio"><input type="radio" bind:group={draft.transport} value="demo" checked disabled /> {t("settings.transport.demo")}</label>{/if}
        </div>
        <p class="hint">{t("settings.transport.restart_hint")}</p>
        <label class="radio"><input type="checkbox" bind:checked={draft.lanMode} /> {t("settings.lan_mode")}</label>
      </section>

      <section class="card">
        <h2>{t("nav.lan")}</h2>
        <label for="host">{t("settings.lanpage_host")}</label>
        <input id="host" bind:value={draft.lanpageHost} />
        <p class="hint">{t("settings.lanpage_host.hint")}</p>
        <label class="radio"><input type="checkbox" bind:checked={draft.sendStats} /> {t("settings.send_stats")}</label>
      </section>

      <section class="card">
        <h2>{t("settings.theme")}</h2>
        <select bind:value={draft.theme}>
          <option value={null}>{t("settings.theme.auto")}</option>
          <option value="default">{t("settings.theme.default")}</option>
          <option value="beispiel-lan">{t("settings.theme.beispiel")}</option>
        </select>
      </section>

      <section class="card">
        <button class="ghost" onclick={() => (showAdvanced = !showAdvanced)}>{showAdvanced ? "▾" : "▸"} {t("settings.advanced")}</button>
        {#if showAdvanced}
          <div class="stack" style="margin-top:0.8rem">
            <div>
              <label for="port">{t("settings.sync_port")}</label>
              <input id="port" type="number" min="0" max="65535" bind:value={draft.syncPort} />
            </div>
            <div>
              <label for="ckey">{t("settings.catalog_key")}</label>
              <input id="ckey" bind:value={draft.catalogKey} maxlength="40" spellcheck="false" autocomplete="off" placeholder="B…" />
              <p class="hint">{t("settings.catalog_key.hint")}</p>
            </div>
            {#if app.bootstrap?.platform !== "windows"}
              <h3>{t("settings.runners")}</h3>
              <div><label for="wine">{t("settings.runners.wine")}</label><input id="wine" bind:value={draft.runnerPaths.wine} /></div>
              <div><label for="cx">{t("settings.runners.crossover")}</label><input id="cx" bind:value={draft.runnerPaths.crossoverApp} /></div>
              <div><label for="proton">{t("settings.runners.proton")}</label><input id="proton" bind:value={draft.runnerPaths.proton} /></div>
            {/if}
          </div>
        {/if}
      </section>

      <div class="row end">
        <button class="primary big" onclick={save} disabled={saving}>{saving ? t("action.working") : t("action.save")}</button>
      </div>

      <p class="hint about">
        {t("settings.about.text", { version: app.bootstrap?.version ?? "", platform: app.bootstrap?.platform ?? "" })}<br />
        {t("settings.dirs", { data: app.bootstrap?.dirs.data ?? "" })}
      </p>
    </div>
  {/if}
</div>

<style>
  input:not([type="radio"]):not([type="checkbox"]),
  select {
    width: 100%;
    max-width: 480px;
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1rem;
    margin-top: 0.8rem;
  }
  .roots {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .roots li {
    display: flex;
    gap: 0.6rem;
    align-items: center;
    background: var(--color-surface-alt);
    border-radius: calc(var(--radius) * 0.7);
    padding: 0.7rem 0.9rem;
  }
  .roots .progress {
    margin-top: 0.4rem;
    max-width: 320px;
  }
  .radio {
    font-weight: 400;
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }
  .radios {
    gap: 0.4rem;
  }
  .end {
    justify-content: flex-end;
  }
  .about {
    text-align: center;
    user-select: text;
  }
</style>
