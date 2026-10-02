<script>
  import { createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";
  import { autofocus } from "./actions.js";

  const dispatch = createEventDispatcher();

  let name = "";
  let error = "";
  let saving = false;

  async function submit() {
    if (!name.trim()) return;
    saving = true;
    error = "";
    try {
      const id = await api.createGroup(name.trim());
      dispatch("created", { id });
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="modal-backdrop" role="presentation" on:click|self={() => dispatch("close")}>
  <div class="modal">
    <div class="modal-header">
      <span class="modal-icon"><Icon name="users" /></span>
      <div>
        <div class="modal-title">Nouveau groupe</div>
        <div class="modal-desc">Regroupe plusieurs postes pour leur affecter des règles en une fois</div>
      </div>
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    <label style="display:block; margin-bottom:4px;">
      <span class="subtitle">Nom du groupe</span>
      <input
        use:autofocus
        bind:value={name}
        placeholder="Enfants, Invités…"
        style="width:100%; margin-top:4px;"
        on:keydown={(e) => e.key === "Enter" && submit()}
      />
    </label>

    <div class="modal-actions">
      <button class="primary" on:click={submit} disabled={saving || !name.trim()}>{saving ? "Création…" : "Créer"}</button>
      <button on:click={() => dispatch("close")}>Annuler</button>
    </div>
  </div>
</div>
