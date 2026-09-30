<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";

  export let refreshToken = 0;

  let lists = [];
  let selected = null;
  let newListName = "";
  let newDomainText = "";
  let domainFilter = "";
  let error = "";

  async function load() {
    error = "";
    try {
      lists = await api.listLists();
      if (selected) selected = lists.find((l) => l.id === selected.id) ?? null;
    } catch (e) {
      error = String(e);
    }
  }

  async function createList() {
    if (!newListName.trim()) return;
    try {
      const id = await api.createList(newListName.trim(), []);
      newListName = "";
      await load();
      selected = lists.find((l) => l.id === id) ?? null;
    } catch (e) {
      error = String(e);
    }
  }

  function selectList(list) {
    selected = list;
    domainFilter = "";
  }

  async function removeList(list) {
    if (!confirm(`Supprimer la liste « ${list.name} » ? Elle sera retirée des postes qui l'utilisent.`)) return;
    try {
      await api.deleteList(list.id);
      if (selected?.id === list.id) selected = null;
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  function normalizeDomain(raw) {
    return raw
      .trim()
      .toLowerCase()
      .replace(/^https?:\/\//, "")
      .replace(/\/.*$/, "");
  }

  async function addDomains() {
    if (!selected || !newDomainText.trim()) return;
    const added = newDomainText
      .split(/[\n,]+/)
      .map(normalizeDomain)
      .filter(Boolean);
    const domains = Array.from(new Set([...selected.domains, ...added]));
    try {
      await api.updateListDomains(selected.id, domains);
      newDomainText = "";
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function removeDomain(domain) {
    if (!selected) return;
    const domains = selected.domains.filter((d) => d !== domain);
    try {
      await api.updateListDomains(selected.id, domains);
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  $: visibleDomains = selected
    ? selected.domains.filter((d) => d.includes(domainFilter.trim().toLowerCase()))
    : [];

  onMount(load);
  $: refreshToken, load();
</script>

<h1>Listes</h1>
<p class="subtitle">Domaines regroupés, réutilisables sur plusieurs postes.</p>
{#if error}<p class="error">{error}</p>{/if}

<div style="display:grid; grid-template-columns: 240px 1fr; gap:16px; align-items:start;">
  <div class="card" style="padding:0;">
    {#if lists.length === 0}
      <div class="empty" style="padding: var(--space-5) var(--space-3);">
        <Icon name="list" large />
        <p>Aucune liste pour le moment. Créez-en une ci-dessous.</p>
      </div>
    {:else}
      {#each lists as list}
        <div
          class="row"
          role="button"
          tabindex="0"
          style="padding:10px 12px; border-bottom:1px solid var(--border); cursor:pointer; background:{selected?.id === list.id ? 'var(--accent-weak)' : 'transparent'}"
          on:click={() => selectList(list)}
          on:keydown={(e) => e.key === "Enter" && selectList(list)}
        >
          <span style={selected?.id === list.id ? "color:var(--accent); font-weight:600;" : ""}>{list.name}</span>
          <div class="spacer" />
          <span class="pill pill-neutral">{list.domains.length}</span>
        </div>
      {/each}
    {/if}
    <div style="padding:10px 12px;">
      <input placeholder="Nouvelle liste…" bind:value={newListName} on:keydown={(e) => e.key === "Enter" && createList()} style="width:100%; margin-bottom:6px;" />
      <button class="primary" style="width:100%;" on:click={createList}>
        <span class="row" style="gap:5px; justify-content:center;"><Icon name="plus" />Nouvelle liste</span>
      </button>
    </div>
  </div>

  <div class="card">
    {#if !selected}
      <div class="empty">
        <Icon name="list" large />
        <p>Sélectionnez une liste à gauche pour voir et modifier ses domaines.</p>
      </div>
    {:else}
      <div class="row" style="margin-bottom:12px;">
        <h2 style="margin:0;">{selected.name}</h2>
        <span class="pill pill-neutral">{selected.domains.length} domaine{selected.domains.length === 1 ? "" : "s"}</span>
        <div class="spacer" />
        {#if selected.domains.length > 6}
          <div class="row" style="gap:6px;">
            <Icon name="search" />
            <input placeholder="Filtrer…" bind:value={domainFilter} style="width:140px;" />
          </div>
        {/if}
        <button class="danger" on:click={() => removeList(selected)}>Supprimer</button>
      </div>

      {#if selected.domains.length === 0}
        <div class="empty">
          <Icon name="list" large />
          <p>Aucun domaine pour le moment — ajoutez-en un ou plusieurs ci-dessous.</p>
        </div>
      {:else if visibleDomains.length === 0}
        <p class="subtitle" style="margin:0;">Aucun domaine ne correspond à « {domainFilter} ».</p>
      {:else}
        <div class="chip-row">
          {#each visibleDomains as domain}
            <span class="chip">
              {domain}
              <button on:click={() => removeDomain(domain)} aria-label="Retirer {domain}">
                <Icon name="x" />
              </button>
            </span>
          {/each}
        </div>
      {/if}

      <div style="margin-top:16px;">
        <textarea
          bind:value={newDomainText}
          placeholder="youtube.com, tiktok.com…"
          rows="3"
          style="width:100%; font-family:var(--font-mono); font-size:var(--text-sm); border:1px solid var(--border); border-radius:var(--radius-sm); padding:8px;"
        />
        <p class="subtitle" style="margin: 4px 0 8px;">
          Un domaine par ligne ou séparés par des virgules. Les variantes courantes (www., m.…) sont ajoutées automatiquement à l'application.
        </p>
        <button class="primary" on:click={addDomains} disabled={!newDomainText.trim()}>Ajouter</button>
      </div>
    {/if}
  </div>
</div>
