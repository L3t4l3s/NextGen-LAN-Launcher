<script lang="ts">
  import type { ConfigReport, GameConfig, GameConfigView, GameView, RunnerKind } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { api, confirmDialog, copyText } from "$lib/api";
  import { t, userText } from "$lib/i18n";

  let { game, onclose, onsaved }: { game: GameView; onclose: () => void; onsaved: () => void } = $props();

  let view = $state<GameConfigView | null>(null);
  let config = $state<GameConfig | null>(null);
  // What is stored, to tell whether the editor holds unsaved changes.
  let stored = $state("");
  const dirty = $derived(!!config && JSON.stringify(config) !== stored);
  const platformName = $derived(
    ({ linux: "Linux", macos: "macOS", windows: "Windows" } as Record<string, string>)[view?.platform ?? ""] ?? view?.platform ?? "",
  );
  let working = $state(false);
  let step = $state<"edit" | "share">("edit");
  let comment = $state("");
  let report = $state<ConfigReport | null>(null);
  let copied = $state(false);

  // Variables testers reach for most often; offered as suggestions only.
  const commonEnv = [
    "PROTON_USE_WINED3D",
    "PROTON_NO_ESYNC",
    "PROTON_NO_FSYNC",
    "PROTON_ENABLE_NVAPI",
    "PROTON_LOG",
    "DXVK_HUD",
    "DXVK_FRAME_RATE",
    "WINEDEBUG",
    "WINE_FULLSCREEN_FSR",
    "MESA_GL_VERSION_OVERRIDE",
    "SDL_VIDEODRIVER",
  ];

  const runners = $derived<RunnerKind[]>(
    view?.platform === "macos" ? ["auto", "crossover", "wine", "native"] : ["auto", "proton", "wine", "native"],
  );

  // Keyed on the id alone: a catalog reload hands in a new `game` object
  // for the same game, and must not wipe what the tester is typing.
  const gameId = $derived(game.id);
  $effect(() => {
    const id = gameId;
    view = null;
    api
      .gameConfig(id)
      .then((v) => {
        view = v;
        config = structuredClone(v.config);
        stored = JSON.stringify(v.config);
      })
      .catch((e) => {
        app.toast("error", userText(e));
        onclose();
      });
  });

  async function save(): Promise<boolean> {
    if (!config) return false;
    working = true;
    try {
      const snapshot = $state.snapshot(config);
      const own = await api.saveGameConfig(game.id, snapshot);
      stored = JSON.stringify(snapshot);
      app.toast("success", t(own ? "config.saved" : "config.saved_as_profile"));
      if (view) view.own = own;
      onsaved();
      return true;
    } catch (e) {
      app.toast("error", userText(e));
      return false;
    } finally {
      working = false;
    }
  }

  // The report holds what is stored, so unsaved changes are saved first —
  // otherwise a tester would send the state before their last edit.
  async function toShare() {
    if (dirty && !(await save())) return;
    report = null;
    step = "share";
  }

  async function reset() {
    working = true;
    try {
      await api.resetGameConfig(game.id);
      view = await api.gameConfig(game.id);
      config = structuredClone(view.config);
      stored = JSON.stringify(view.config);
      app.toast("success", t("config.reset_done"));
      onsaved();
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      working = false;
    }
  }

  async function buildReport() {
    working = true;
    try {
      report = await api.shareGameConfig(game.id, comment);
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      working = false;
    }
  }

  async function open(url: string) {
    try {
      await api.openUrl(url);
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  async function copyReport() {
    if (!report) return;
    await copyText(`${report.subject}\n\n${report.body}`);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  async function saveFile() {
    if (!report) return;
    try {
      const path = await api.exportGameConfig(report.fileName, report.toml);
      if (path) app.toast("success", t("config.share.file_saved", { path }));
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  // A stray touch outside the dialog must not throw away what was typed.
  async function close() {
    if (dirty && !(await confirmDialog(t("config.discard")))) return;
    onclose();
  }

  const addEnv = () => config?.env.push({ name: "", value: "" });
  const removeEnv = (i: number) => config?.env.splice(i, 1);
</script>

<div class="modal-backdrop" role="presentation" onclick={close} onkeydown={(e) => e.key === "Escape" && close()}>
  <div class="modal card config" role="dialog" tabindex="-1" aria-labelledby="config-title" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
    <h2 id="config-title">{t(step === "edit" ? "config.title" : "config.share.title", { title: game.title })}</h2>

    {#if !view || !config}
      <p class="muted">{t("config.loading")}</p>
    {:else if step === "edit"}
      <p class="hint">{t("config.intro", { platform: platformName })}</p>
      {#if view.own}<p class="hint own">{t("config.own_profile")}</p>{/if}
      {#if view.configError}<p class="hint warn">{t("config.unreadable", { detail: view.configError })}</p>{/if}

      <div class="fields">
        <label for="cfg-exe">{t("config.exe")}</label>
        <input id="cfg-exe" list="cfg-exes" bind:value={config.exe} placeholder="Game.exe" />
        <datalist id="cfg-exes">
          {#each view.executables as exe (exe)}<option value={exe}></option>{/each}
        </datalist>

        <label for="cfg-args">{t("config.args")}</label>
        <input id="cfg-args" bind:value={config.args} placeholder="-windowed +set name %player%" />
        <p class="hint field-hint">{t("config.args_hint")}</p>

        <label for="cfg-workdir">{t("config.workdir")}</label>
        <input id="cfg-workdir" bind:value={config.workdir} placeholder={t("config.workdir_placeholder")} />

        <label for="cfg-runner">{t("config.runner")}</label>
        <select id="cfg-runner" bind:value={config.runner}>
          {#each runners as runner (runner)}<option value={runner}>{t(`config.runner.${runner}`)}</option>{/each}
        </select>
        <p class="hint field-hint">{t("config.runner_hint")}</p>

        <label for="cfg-wrapper">{t("config.wrapper")}</label>
        <input id="cfg-wrapper" bind:value={config.wrapper} placeholder="gamemoderun mangohud" />
        <p class="hint field-hint">{t("config.wrapper_hint")}</p>

        <label for="cfg-dll">{t("config.dll")}</label>
        <input id="cfg-dll" bind:value={config.dllOverrides} placeholder="dinput8=n,b;ddraw=n" />

        <span class="label">{t("config.env")}</span>
        <datalist id="cfg-env-names">
          {#each commonEnv as name (name)}<option value={name}></option>{/each}
        </datalist>
        {#each config.env as variable, i (i)}
          <div class="row env">
            <input class="grow" list="cfg-env-names" bind:value={variable.name} placeholder="PROTON_USE_WINED3D" aria-label={t("config.env_name")} />
            <input class="value" bind:value={variable.value} placeholder="1" aria-label={t("config.env_value")} />
            <button class="ghost" onclick={() => removeEnv(i)} aria-label={t("config.env_remove")}>✕</button>
          </div>
        {/each}
        <div class="row"><button class="ghost" onclick={addEnv}>+ {t("config.env_add")}</button></div>
      </div>

      <div class="row end">
        {#if view.own || view.configError}
          <button class="ghost" onclick={reset} disabled={working}>{t("config.reset")}</button>
        {/if}
        <span class="grow"></span>
        <button class="ghost" onclick={toShare} disabled={working}>{t("config.share")}</button>
        <button data-gamepad-back onclick={close}>{t("action.close")}</button>
        <button class="primary" onclick={save} disabled={working || !dirty}>{t("config.save")}</button>
      </div>
    {:else}
      <p class="hint">{t("config.share.intro")}</p>
      <label for="cfg-comment">{t("config.share.comment")}</label>
      <textarea id="cfg-comment" rows="3" bind:value={comment} placeholder={t("config.share.comment_placeholder")}></textarea>
      <div class="row end">
        <button onclick={buildReport} disabled={working}>{t(report ? "config.share.rebuild" : "config.share.build")}</button>
      </div>

      {#if report}
        <pre class="preview">{report.subject}{"\n\n"}{report.body}</pre>
        <div class="row">
          <button class="primary" onclick={() => open(report!.mailto)}>{t("config.share.mail")}</button>
          <button onclick={() => open(report!.issueUrl)}>{t("config.share.issue")}</button>
          <button onclick={copyReport}>{copied ? t("action.copied") : t("config.share.copy")}</button>
          <button onclick={saveFile}>{t("config.share.file")}</button>
        </div>
        <p class="hint">{t("config.share.fallback", { email: view.reportEmail })}</p>
      {/if}

      <div class="row end">
        <button class="ghost" onclick={() => (step = "edit")}>{t("config.share.back")}</button>
        <button data-gamepad-back onclick={close}>{t("action.close")}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .config {
    width: min(680px, 94vw);
    max-height: 92vh;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .config h2 {
    margin: 0;
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .fields label,
  .fields .label {
    margin-top: 0.5rem;
    margin-bottom: 0;
    font-weight: 600;
  }
  .field-hint {
    margin: 0;
    font-size: 0.8rem;
  }
  .own {
    color: var(--color-primary);
  }
  .warn {
    color: var(--color-warning);
  }
  .env .value {
    width: 7rem;
  }
  .end {
    justify-content: flex-end;
  }
  .preview {
    font-size: 0.75rem;
    background: var(--color-bg);
    padding: 0.6rem;
    border-radius: 6px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 30vh;
    overflow: auto;
    user-select: text;
    margin: 0;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
  }
</style>
