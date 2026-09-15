<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
</script>

<div class="toasts">
  {#each app.toasts as toast (toast.id)}
    <div class="toast {toast.kind}">{toast.text}</div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 1.25rem;
    bottom: calc(var(--statusbar-h) + 1rem);
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    z-index: 60;
    max-width: 380px;
  }
  .toast {
    background: var(--color-surface-alt);
    border: 1px solid var(--color-border);
    border-left: 4px solid var(--color-primary);
    border-radius: calc(var(--radius) * 0.7);
    padding: 0.7rem 1rem;
    box-shadow: var(--shadow);
    animation: slide 0.2s ease-out;
  }
  .toast.success {
    border-left-color: var(--color-success);
  }
  .toast.error {
    border-left-color: var(--color-danger);
  }
  @keyframes slide {
    from {
      transform: translateX(20px);
      opacity: 0;
    }
  }
</style>
