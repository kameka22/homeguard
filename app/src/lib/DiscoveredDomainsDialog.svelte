<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";
  import { autofocus } from "./actions.js";

  export let agentId;
  export let hostname = "";
  export let domains = [];

  const dispatch = createEventDispatcher();
  const NEW_LIST = "__new__";

  let lists = [];
  let selected = new Set(domains);
  let targetListId = "";
  let newListName = "";
  let error = "";
  let saving = false;

  onMount(async () => {
    try {
      lists = await api.listLists();
    } catch (e) {
      error = String(e);
    }
  });

  function toggle(domain) {
    if (selected.has(domain)) selected.delete(domain);
    else selected.add(domain);
    selected = selected;
  }

  async function ignore() {
    try {
      await api.clearAgentPendingDiscoveredDomains(agentId);
      dispatch("done");
    } catch (e) {
      error = String(e);
    }
  }

  async function submit() {
    if (selected.size === 0 || !targetListId) return;
    if (targetListId === NEW_LIST && !newListName.trim()) return;
    saving = true;
    error = "";
    try {
      if (targetListId === NEW_LIST) {
        await api.createList(newListName.trim(), [...selected]);
      } else {
        const target = lists.find((l) => l.id === targetListId);
        const merged = [...new Set([...(target?.domains ?? []), ...selected])];
        await api.updateListDomains(targetListId, merged);
      }
      await api.clearAgentPendingDiscoveredDomains(agentId);
      dispatch("done");
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
      <span class="modal-icon warn"><Icon name="warning" /></span>
      <div>
        <div class="modal-title">Domaines déjà bloqués détectés</div>
        <div class="modal-desc">
          {domains.length} domaine{domains.length > 1 ? "s" : ""} trouvé{domains.length > 1 ? "s" : ""} dans le fichier hosts de « {hostname} » avant l'enrôlement.
        </div>
      </div>
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    <div class="chip-row" style="margin-bottom:var(--space-3); max-height:160px; overflow-y:auto;">
      {#each domains as domain}
        <label class="chip" style="cursor:pointer;">
          <input type="checkbox" checked={selected.has(domain)} on:change={() => toggle(domain)} style="margin:0;" />
          {domain}
        </label>
      {/each}
    </div>

    <label style="display:block; margin-bottom:10px;">
      <span class="subtitle">Ajouter la sélection à</span>
      <select bind:value={targetListId} style="width:100%; margin-top:4px;">
        <option value="">Choisir une liste…</option>
        {#each lists as l}<option value={l.id}>{l.name}</option>{/each}
        <option value={NEW_LIST}>+ Nouvelle liste…</option>
      </select>
    </label>
    {#if targetListId === NEW_LIST}
      <input use:autofocus placeholder="Nom de la liste…" bind:value={newListName} style="width:100%; margin-bottom:10px;" />
    {/if}

    <div class="modal-actions">
      <button class="primary" on:click={submit} disabled={saving || selected.size === 0 || !targetListId}>
        {saving ? "Ajout…" : `Ajouter à la liste (${selected.size})`}
      </button>
      <button on:click={ignore}>Ignorer</button>
    </div>
  </div>
</div>
