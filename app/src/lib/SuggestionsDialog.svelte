<script>
  import { createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";
  import { SITE_SUGGESTIONS } from "./siteSuggestions.js";

  export let listId;
  export let existingDomains = [];

  const dispatch = createEventDispatcher();

  let activeCategory = SITE_SUGGESTIONS[0];
  let selected = new Set();
  let error = "";
  let saving = false;

  function isAdded(domain) {
    return existingDomains.includes(domain);
  }

  function toggle(domain) {
    if (isAdded(domain)) return;
    if (selected.has(domain)) selected.delete(domain);
    else selected.add(domain);
    selected = selected;
  }

  function addWholeCategory(category) {
    for (const site of category.sites) {
      if (!isAdded(site.domain)) selected.add(site.domain);
    }
    selected = selected;
  }

  function categoryFullyAdded(category) {
    return category.sites.every((s) => isAdded(s.domain) || selected.has(s.domain));
  }

  async function submit() {
    if (selected.size === 0) return;
    saving = true;
    error = "";
    try {
      const domains = [...new Set([...existingDomains, ...selected])];
      await api.updateListDomains(listId, domains);
      dispatch("done");
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="modal-backdrop" role="presentation" on:click|self={() => dispatch("close")}>
  <div class="modal wide">
    <div class="modal-header">
      <span class="modal-icon"><Icon name="list" /></span>
      <div>
        <div class="modal-title">Ajouter depuis des suggestions</div>
        <div class="modal-desc">Un point de départ par catégorie — rien n'est ajouté tant que vous ne validez pas.</div>
      </div>
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    <div class="suggest-body">
      <div class="suggest-categories">
        {#each SITE_SUGGESTIONS as category}
          <button class="nav-item" class:active={category === activeCategory} on:click={() => (activeCategory = category)}>
            <span style="flex:1;">{category.category}</span>
            <span class="nav-badge">{category.sites.length}</span>
          </button>
        {/each}
      </div>

      <div class="suggest-sites">
        <div class="row" style="margin-bottom:var(--space-2);">
          <span class="subtitle" style="flex:1;">{activeCategory.category}</span>
          <button on:click={() => addWholeCategory(activeCategory)} disabled={categoryFullyAdded(activeCategory)}>
            Tout ajouter
          </button>
        </div>
        {#each activeCategory.sites as site}
          <label class="suggest-site-row" class:disabled={isAdded(site.domain)}>
            <input type="checkbox" checked={isAdded(site.domain) || selected.has(site.domain)} disabled={isAdded(site.domain)} on:change={() => toggle(site.domain)} />
            <span style="flex:1;">{site.label}</span>
            <span class="mono subtitle">{site.domain}</span>
            {#if isAdded(site.domain)}<span class="subtitle">déjà ajoutée</span>{/if}
          </label>
        {/each}
      </div>
    </div>

    <div class="modal-actions">
      <button class="primary" on:click={submit} disabled={saving || selected.size === 0}>
        {saving ? "Ajout…" : `Ajouter (${selected.size})`}
      </button>
      <button on:click={() => dispatch("close")}>Annuler</button>
    </div>
  </div>
</div>
