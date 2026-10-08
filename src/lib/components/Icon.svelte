<script lang="ts">
  // Small line icons that follow the text colour. "bug" marks every way to
  // report a problem, "share" sharing a working configuration.
  let { name, size = 16 }: { name: "bug" | "share"; size?: number } = $props();
</script>

<svg class="icon" viewBox="0 0 24 24" width={size} height={size} aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  {#if name === "bug"}
    <rect x="7" y="7" width="10" height="13" rx="5" />
    <path d="M9 7.5V6a3 3 0 0 1 6 0v1.5M12 11v9M3 13h4M17 13h4M4 7l3 2M20 7l-3 2M4 20l3-2.5M20 20l-3-2.5" />
  {:else}
    <circle cx="18" cy="5" r="3" />
    <circle cx="6" cy="12" r="3" />
    <circle cx="18" cy="19" r="3" />
    <path d="M8.6 13.5l6.8 4M15.4 6.5l-6.8 4" />
  {/if}
</svg>

<style>
  .icon {
    flex: none;
    vertical-align: -0.15em;
  }
</style>
