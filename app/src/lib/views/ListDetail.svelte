<script>
  import { createEventDispatcher } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";
  import ConfirmDialog from "../ConfirmDialog.svelte";
  import AssignListDialog from "../AssignListDialog.svelte";
  import ScheduleDialog from "../ScheduleDialog.svelte";
  import { listColor } from "../colors.js";

  export let listId;
  export let refreshToken = 0;

  const dispatch = createEventDispatcher();

  const DAYS = [
    { key: "mon", label: "L" }, { key: "tue", label: "M" }, { key: "wed", label: "M" },
    { key: "thu", label: "J" }, { key: "fri", label: "V" }, { key: "sat", label: "S" }, { key: "sun", label: "D" },
  ];

  let list = null;
  let agents = [];
  let groups = [];
  let usedBy = [];
  let newDomainText = "";
  let domainFilter = "";
  let error = "";
  let confirmDelete = false;
  let showAssign = false;
  let editingAssignment = null;

  async function load() {
    error = "";
    try {
      const [lists, a, g, asn] = await Promise.all([
        api.listLists(), api.listAgents(), api.listGroups(), api.listAssignments(),
      ]);
      list = lists.find((l) => l.id === listId) ?? null;
      agents = a;
      groups = g;
      usedBy = asn.filter((x) => x.list_id === listId);
    } catch (e) {
      error = String(e);
    }
  }

  function targetName(a) {
    if (a.target_kind === "agent") {
      const ag = agents.find((x) => x.id === a.target_id);
      return ag ? ag.display_name || ag.hostname : a.target_id;
    }
    const g = groups.find((x) => x.id === a.target_id);
    return g ? g.name : a.target_id;
  }

  function scheduleSummary(schedule) {
    if (!schedule || schedule.length === 0) return "Toujours";
    const w = schedule[0];
    const days = w.days.map((d) => DAYS.find((x) => x.key === d)?.label ?? d).join("");
    return `${days} ${w.from.slice(0, 5)}–${w.to.slice(0, 5)}`;
  }

  async function scheduleSaved() {
    editingAssignment = null;
    await load();
  }

  function normalizeDomain(raw) {
    return raw.trim().toLowerCase().replace(/^https?:\/\//, "").replace(/\/.*$/, "");
  }

  async function addDomains() {
    if (!list || !newDomainText.trim()) return;
    const added = newDomainText.split(/[\n,]+/).map(normalizeDomain).filter(Boolean);
    const domains = Array.from(new Set([...list.domains, ...added]));
    try {
      await api.updateListDomains(listId, domains);
      newDomainText = "";
      await load();
    } catch (e) { error = String(e); }
  }

  async function removeDomain(domain) {
    if (!list) return;
    const domains = list.domains.filter((d) => d !== domain);
    try { await api.updateListDomains(listId, domains); await load(); } catch (e) { error = String(e); }
  }

  async function confirmDeleteNow() {
    confirmDelete = false;
    try { await api.deleteList(listId); dispatch("changed"); dispatch("removed"); } catch (e) { error = String(e); }
  }

  async function removeAssignment(id) {
    try { await api.deleteAssignment(id); await load(); } catch (e) { error = String(e); }
  }

  async function assignDone() {
    showAssign = false;
    await load();
  }

  $: visibleDomains = list ? list.domains.filter((d) => d.includes(domainFilter.trim().toLowerCase())) : [];

  $: listId, load();
  $: refreshToken, load();
</script>

<div class="toolbar">
  {#if list}<span class="swatch" style="background:{listColor(list.id)}; width:11px; height:11px;"></span>{/if}
  <span class="section-title">{list?.name ?? "…"}</span>
  <div class="spacer"></div>
</div>

<div class="pad">
  {#if error}<p class="error">{error}</p>{/if}

  {#if list}
    <p class="subtitle" style="margin:0 0 var(--space-4);">
      {list.domains.length} domaine(s) · utilisée par {usedBy.length} cible(s)
    </p>

    <div class="group">
      <div class="row-line">
        <span style="flex:1; font-weight:650; font-size:var(--text-sm);">Domaines</span>
        {#if list.domains.length > 6}
          <input placeholder="Filtrer…" bind:value={domainFilter} style="width:140px;" />
        {/if}
      </div>
      {#if list.domains.length === 0}
        <div class="row-line"><span class="subtitle">Aucun domaine — ajoutez-en un ou plusieurs ci-dessous.</span></div>
      {:else if visibleDomains.length === 0}
        <div class="row-line"><span class="subtitle">Aucun domaine ne correspond à « {domainFilter} ».</span></div>
      {:else}
        <div class="row-line" style="flex-wrap:wrap;">
          <div class="chip-row">
            {#each visibleDomains as domain}
              <span class="chip">
                {domain}
                <button on:click={() => removeDomain(domain)} aria-label="Retirer {domain}"><Icon name="x" /></button>
              </span>
            {/each}
          </div>
        </div>
      {/if}
      <div class="row-line" style="flex-direction:column; align-items:stretch; gap:var(--space-2);">
        <textarea
          bind:value={newDomainText}
          placeholder="youtube.com, tiktok.com…"
          rows="3"
          style="width:100%; font-family:var(--font-mono);"
        />
        <div class="row">
          <p class="subtitle" style="margin:0; flex:1;">
            http(s)://, chemins et doublons sont retirés automatiquement. Les variantes connues sont ajoutées.
          </p>
          <button class="tbtn primary" on:click={addDomains} disabled={!newDomainText.trim()}>Ajouter</button>
        </div>
      </div>
    </div>

    <div class="group">
      <div class="row-line" style="border-top:none;">
        <span style="flex:1; font-weight:650; font-size:var(--text-sm);">Utilisée par</span>
        <button class="tbtn primary" on:click={() => (showAssign = true)}>Affecter…</button>
      </div>
      {#if usedBy.length === 0}
        <div class="row-line"><span class="subtitle">Pas encore affectée à un poste ou un groupe.</span></div>
      {:else}
        {#each usedBy as a}
          <div class="row-line">
            <Icon name={a.target_kind === "group" ? "users" : "monitor"} style="color:var(--text-faint);" />
            <span style="flex:1;">{targetName(a)}</span>
            <span class="pill {a.schedule ? 'pill-accent' : 'pill-neutral'}">{scheduleSummary(a.schedule)}</span>
            <button on:click={() => (editingAssignment = a)}>Horaire…</button>
            <button class="toolbar-icon-btn" on:click={() => removeAssignment(a.id)} aria-label="Retirer"><Icon name="x" /></button>
          </div>
        {/each}
      {/if}
    </div>

    <div class="row" style="justify-content:flex-end; margin-top:var(--space-4);">
      <button class="danger" on:click={() => (confirmDelete = true)}>Supprimer la liste…</button>
    </div>
  {:else if !error}
    <p class="subtitle">Chargement…</p>
  {/if}
</div>

{#if confirmDelete}
  <ConfirmDialog
    icon="warning"
    title="Supprimer la liste « {list?.name} » ?"
    description="Elle sera retirée des postes et groupes qui l'utilisent."
    confirmLabel="Supprimer"
    on:cancel={() => (confirmDelete = false)}
    on:confirm={confirmDeleteNow}
  />
{/if}

{#if showAssign}
  <AssignListDialog
    listId={listId}
    listName={list?.name}
    on:close={() => (showAssign = false)}
    on:done={assignDone}
  />
{/if}

{#if editingAssignment}
  <ScheduleDialog
    assignmentId={editingAssignment.id}
    title="Bloquer « {list?.name} » pour {targetName(editingAssignment)}"
    initialSchedule={editingAssignment.schedule}
    on:close={() => (editingAssignment = null)}
    on:saved={scheduleSaved}
  />
{/if}
