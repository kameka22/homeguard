<script>
  import { createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";
  import { autofocus } from "./actions.js";

  const dispatch = createEventDispatcher();

  let name = "";
  let domainsText = "";
  let error = "";
  let saving = false;

  function normalizeDomain(raw) {
    return raw.trim().toLowerCase().replace(/^https?:\/\//, "").replace(/\/.*$/, "");
  }

  async function submit() {
    if (!name.trim()) return;
    saving = true;
    error = "";
    try {
      const domains = [...new Set(domainsText.split(/[\n,]+/).map(normalizeDomain).filter(Boolean))];
      const id = await api.createList(name.trim(), domains);
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
      <span class="modal-icon"><Icon name="list" /></span>
      <div>
        <div class="modal-title">Nouvelle liste</div>
        <div class="modal-desc">Un ensemble de domaines à bloquer, affectable à des postes ou des groupes</div>
      </div>
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    <label style="display:block; margin-bottom:10px;">
      <span class="subtitle">Nom de la liste</span>
      <input
        use:autofocus
        bind:value={name}
        placeholder="Réseaux sociaux, Jeux…"
        style="width:100%; margin-top:4px;"
      />
    </label>

    <label style="display:block; margin-bottom:4px;">
      <span class="subtitle">Domaines (facultatif, modifiable ensuite)</span>
      <textarea
        bind:value={domainsText}
        placeholder="youtube.com, tiktok.com…"
        rows="3"
        style="width:100%; margin-top:4px; font-family:var(--font-mono);"
      />
    </label>

    <div class="modal-actions">
      <button class="primary" on:click={submit} disabled={saving || !name.trim()}>{saving ? "Création…" : "Créer"}</button>
      <button on:click={() => dispatch("close")}>Annuler</button>
    </div>
  </div>
</div>
