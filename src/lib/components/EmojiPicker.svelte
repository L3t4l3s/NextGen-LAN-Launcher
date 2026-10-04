<script lang="ts">
  import { emojiGroups } from "$lib/emoji";
  import { t } from "$lib/i18n";

  let { onpick, onclose }: { onpick: (emoji: string) => void; onclose: () => void } = $props();
  let group = $state(emojiGroups[0].id);
  const current = $derived(emojiGroups.find((g) => g.id === group) ?? emojiGroups[0]);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="picker" role="dialog" aria-label={t("chat.emoji")} tabindex="-1">
  <div class="tabs">
    {#each emojiGroups as g (g.id)}
      <button class="tab" class:active={g.id === group} title={t(`chat.emoji.${g.id}`)} onclick={() => (group = g.id)}>{g.icon}</button>
    {/each}
  </div>
  <div class="grid">
    {#each current.emoji as e (e)}
      <button class="emoji" onclick={() => onpick(e)}>{e}</button>
    {/each}
  </div>
</div>

<style>
  .picker {
    width: 300px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 0.4rem;
    z-index: 30;
  }
  .tabs {
    display: flex;
    gap: 0.15rem;
    border-bottom: 1px solid var(--color-border);
    padding-bottom: 0.3rem;
    margin-bottom: 0.3rem;
  }
  .tab {
    flex: 1;
    padding: 0.25em 0;
    background: transparent;
    border-color: transparent;
    font-size: 1.1rem;
    opacity: 0.6;
  }
  .tab.active {
    opacity: 1;
    background: var(--color-surface-alt);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    max-height: 210px;
    overflow-y: auto;
  }
  .emoji {
    background: transparent;
    border: none;
    padding: 0.2em 0;
    font-size: 1.3rem;
    line-height: 1.3;
    border-radius: 8px;
  }
  .emoji:hover {
    background: var(--color-surface-alt);
  }
</style>
