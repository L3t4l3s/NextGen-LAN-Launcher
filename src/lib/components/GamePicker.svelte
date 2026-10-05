<script lang="ts">
  import { api, coverSrc } from "$lib/api";
  import { isTopic } from "$lib/chat";
  import { t, userText } from "$lib/i18n";
  import { app } from "$lib/stores/app.svelte";
  import { chat } from "$lib/stores/chat.svelte";

  /** Link a game of the library in the open conversation. */
  let { onclose }: { onclose: () => void } = $props();
  let filter = $state("");
  let busy = $state(false);

  const shown = $derived(
    app.games
      .filter((g) => !filter.trim() || g.title.toLowerCase().includes(filter.trim().toLowerCase()))
      .slice()
      .sort((a, b) => a.title.localeCompare(b.title)),
  );

  async function share(id: string) {
    if (busy) return;
    busy = true;
    try {
      await api.chat.shareGame(chat.active, id);
      chat.atBottom = true;
      onclose();
    } catch (e) {
      if (!chat.notePause(e)) app.toast("error", userText(e));
      onclose();
    } finally {
      busy = false;
    }
  }

  function focusOnMount(el: HTMLElement) {
    el.focus();
  }
</script>

<div class="modal-backdrop" role="presentation" onclick={onclose} onkeydown={(e) => e.key === "Escape" && onclose()}>
  <div class="modal card" role="dialog" tabindex="-1" aria-labelledby="game-picker-title" onclick={(e) => e.stopPropagation()} onkeydown={(e) => { e.stopPropagation(); if (e.key === "Escape") onclose(); }}>
    <h2 id="game-picker-title">🎮 {t("chat.game.share")}</h2>
    <p class="hint">
      {chat.active === null
        ? t("chat.game.to_public")
        : t("chat.game.to", { name: (isTopic(chat.active) ? "# " : "") + chat.conversationName(chat.active) })}
    </p>
    <input use:focusOnMount bind:value={filter} placeholder={t("library.search")} aria-label={t("library.search")} />
    <ul class="games">
      {#each shown as g (g.id)}
        {@const src = coverSrc(g.cover)}
        <li>
          <button disabled={busy} onclick={() => share(g.id)}>
            {#if src}<img src={src} alt="" />{:else}<span class="ph">🎮</span>{/if}
            <span class="grow">{g.title}</span>
          </button>
        </li>
      {:else}
        <li class="muted">{t("library.nothing_found")}</li>
      {/each}
    </ul>
    <div class="row end">
      <button class="ghost" onclick={onclose}>{t("action.cancel")}</button>
    </div>
  </div>
</div>

<style>
  input {
    width: 100%;
  }
  .games {
    list-style: none;
    margin: 0.5rem 0 0.8rem;
    padding: 0;
    max-height: 360px;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    border-radius: calc(var(--radius) * 0.6);
  }
  .games button {
    width: 100%;
    display: flex;
    gap: 0.6rem;
    align-items: center;
    text-align: left;
    border: none;
    border-radius: 0;
    background: transparent;
    padding: 0.35em 0.6em;
  }
  .games button:hover {
    background: var(--color-surface-alt);
  }
  .games img,
  .ph {
    width: 48px;
    height: 34px;
    object-fit: cover;
    border-radius: 4px;
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .games li.muted {
    padding: 0.5em 0.7em;
  }
  .end {
    justify-content: flex-end;
  }
</style>
