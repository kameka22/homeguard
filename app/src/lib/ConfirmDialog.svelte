<script>
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";

  // macOS convention: Cancel stays the filled/primary button even when the
  // other action is destructive — picking something irreversible takes a
  // deliberate click on the plain button, not the reflexive "big blue one".
  export let title = "";
  export let description = "";
  export let confirmLabel = "Confirmer";
  export let icon = "warning";

  const dispatch = createEventDispatcher();
</script>

<div class="modal-backdrop" role="presentation" on:click|self={() => dispatch("cancel")}>
  <div class="modal">
    <div class="modal-header">
      <span class="modal-icon danger"><Icon name={icon} /></span>
      <div>
        <div class="modal-title">{title}</div>
        {#if description}<div class="modal-desc">{description}</div>{/if}
      </div>
    </div>
    <div class="modal-actions">
      <button class="primary" on:click={() => dispatch("cancel")}>Annuler</button>
      <button on:click={() => dispatch("confirm")}>{confirmLabel}</button>
    </div>
  </div>
</div>
