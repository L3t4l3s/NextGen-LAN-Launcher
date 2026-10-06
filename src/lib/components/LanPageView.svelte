<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import { t, userText } from "$lib/i18n";

  // Bumped by "reload": a cross-origin frame cannot be reloaded from here,
  // only replaced.
  let generation = $state(0);

  async function openInBrowser() {
    if (!app.lanPage) return;
    try {
      await api.openUrl(app.lanPage);
    } catch (e) {
      app.toast("error", userText(String(e)));
    }
  }
</script>

{#if app.lanPage}
  <div class="lanpage">
    <div class="bar">
      <span class="url muted" title={app.lanPage}>{app.lanPage}</span>
      <button class="ghost" onclick={() => generation++}>⟳ {t("lanpage.reload")}</button>
      <button class="ghost" onclick={openInBrowser}>↗ {t("lanpage.open_browser")}</button>
    </div>
    {#key `${app.lanPage}#${generation}`}
      <!-- The page keeps its own origin (logins, forms) but may neither navigate
           the launcher away nor open windows of its own; the launcher's IPC is
           not granted to remote origins. Links meant for a new tab do nothing
           here, "open in browser" is for those. -->
      <iframe src={app.lanPage} title={t("lanpage.title")} sandbox="allow-scripts allow-forms allow-same-origin" referrerpolicy="no-referrer"></iframe>
    {/key}
  </div>
{/if}

<style>
  .lanpage {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.35rem 0.75rem;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }
  .url {
    flex: 1;
    /* A width of its own, or a long address widens the whole window grid. */
    width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
  }
  iframe {
    flex: 1;
    width: 100%;
    border: 0;
    /* Pages without a background of their own expect white, not the theme. */
    background: #fff;
  }
</style>
