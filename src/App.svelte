<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import TopBar from "$lib/components/TopBar.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import Toasts from "$lib/components/Toasts.svelte";
  import LibraryView from "$lib/components/LibraryView.svelte";
  import DownloadsView from "$lib/components/DownloadsView.svelte";
  import DiagnosticsView from "$lib/components/DiagnosticsView.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import SetupWizard from "$lib/components/SetupWizard.svelte";
</script>

<div id="bg_layer"></div>

{#if !app.ready}
  <div class="loading">
    <div class="spinner"></div>
  </div>
{:else if app.loadError}
  <div class="page">
    <div class="card">
      <h2>Backend nicht erreichbar</h2>
      <p class="muted">{app.loadError}</p>
    </div>
  </div>
{:else}
  <div class="shell">
    <TopBar />
    <main>
      {#if app.view === "library"}
        <LibraryView />
      {:else if app.view === "downloads"}
        <DownloadsView />
      {:else if app.view === "diagnostics"}
        <DiagnosticsView />
      {:else}
        <SettingsView />
      {/if}
    </main>
    <StatusBar />
  </div>
  {#if app.showWizard}
    <SetupWizard />
  {/if}
  <Toasts />
{/if}

<style>
  .shell {
    height: 100vh;
    display: grid;
    grid-template-rows: var(--topbar-h) 1fr var(--statusbar-h);
  }
  main {
    min-height: 0;
    overflow: hidden;
  }
  .loading {
    height: 100vh;
    display: grid;
    place-items: center;
  }
  .spinner {
    width: 42px;
    height: 42px;
    border-radius: 50%;
    border: 4px solid var(--color-surface-alt);
    border-top-color: var(--color-primary);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
