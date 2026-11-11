<script>
  import { fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { toasts, dismiss } from "./toasts.js";

  const ICONS = { ok: "check", info: "refresh", warn: "warning", danger: "warning" };
</script>

<div class="toast-stack" aria-live="polite">
  {#each $toasts as t (t.id)}
    <div class="toast toast-{t.kind}" role={t.kind === "danger" ? "alert" : "status"} transition:fly={{ y: 12, duration: 180 }}>
      <span class="toast-icon"><Icon name={ICONS[t.kind]} /></span>
      <div class="toast-body">
        <div class="toast-title">{t.title}</div>
        {#if t.detail}<div class="toast-detail">{t.detail}</div>{/if}
      </div>
      {#if t.action}
        <button class="tbtn" on:click={() => { dismiss(t.id); t.action.run(); }}>{t.action.label}</button>
      {/if}
      <button class="toolbar-icon-btn" on:click={() => dismiss(t.id)} aria-label="Fermer"><Icon name="x" /></button>
    </div>
  {/each}
</div>

<style>
  .toast-stack {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 360px;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 10px 10px 12px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-left: 3px solid var(--neutral);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-pop);
    font-size: var(--text-sm);
  }
  .toast-ok { border-left-color: var(--ok); }
  .toast-warn { border-left-color: var(--warn); }
  .toast-danger { border-left-color: var(--danger); }
  .toast-icon { margin-top: 1px; color: var(--neutral); }
  .toast-ok .toast-icon { color: var(--ok); }
  .toast-warn .toast-icon { color: var(--warn); }
  .toast-danger .toast-icon { color: var(--danger); }
  .toast-body { flex: 1; min-width: 0; }
  .toast-title { font-weight: 600; color: var(--text); }
  .toast-detail { color: var(--text-dim); margin-top: 2px; line-height: 1.35; }
</style>
