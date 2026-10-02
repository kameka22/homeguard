<script>
  import { createEventDispatcher } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";
  import ConfirmDialog from "../ConfirmDialog.svelte";
  import UnblockDialog from "../UnblockDialog.svelte";
  import ScheduleDialog from "../ScheduleDialog.svelte";

  export let groupId;
  export let refreshToken = 0;

  const dispatch = createEventDispatcher();

  const DAYS = [
    { key: "mon", label: "L" }, { key: "tue", label: "M" }, { key: "wed", label: "M" },
    { key: "thu", label: "J" }, { key: "fri", label: "V" }, { key: "sat", label: "S" }, { key: "sun", label: "D" },
  ];

  let group = null;
  let agents = [];
  let lists = [];
  let allAssignments = [];
  let allExceptions = [];
  let memberIds = new Set();
  let newAssignmentListId = "";
  let addingAgentId = "";
  let error = "";
  let confirmDelete = false;
  let showUnblock = false;
  let editingAssignment = null;

  async function load() {
    error = "";
    try {
      const [groups, a, l, asn, exc] = await Promise.all([
        api.listGroups(),
        api.listAgents(),
        api.listLists(),
        api.listAssignments(),
        api.listExceptions(),
      ]);
      group = groups.find((g) => g.id === groupId) ?? null;
      agents = a;
      lists = l;
      allAssignments = asn.filter((x) => x.target_kind === "group" && x.target_id === groupId);
      allExceptions = exc.filter((x) => x.target_kind === "group" && x.target_id === groupId);
      const ids = await Promise.all(agents.map((ag) => api.agentGroupIds(ag.id).then((gs) => (gs.includes(groupId) ? ag.id : null))));
      memberIds = new Set(ids.filter(Boolean));
    } catch (e) {
      error = String(e);
    }
  }

  $: members = agents.filter((a) => memberIds.has(a.id));
  $: nonMembers = agents.filter((a) => !memberIds.has(a.id));

  function listName(id) {
    return lists.find((l) => l.id === id)?.name ?? id;
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

  function remaining(until) {
    const ms = new Date(until).getTime() - Date.now();
    if (ms <= 0) return "expiré";
    const min = Math.round(ms / 60_000);
    return min < 60 ? `${min} min` : `${Math.floor(min / 60)} h ${min % 60} min`;
  }

  async function removeMember(agentId) {
    try { await api.removeGroupMember(groupId, agentId); await load(); dispatch("changed"); } catch (e) { error = String(e); }
  }

  async function addMember() {
    if (!addingAgentId) return;
    try { await api.addGroupMember(groupId, addingAgentId); addingAgentId = ""; await load(); dispatch("changed"); } catch (e) { error = String(e); }
  }

  async function addAssignment() {
    if (!newAssignmentListId) return;
    try { await api.createAssignment(newAssignmentListId, "group", groupId, null); newAssignmentListId = ""; await load(); } catch (e) { error = String(e); }
  }

  async function removeAssignment(id) {
    try { await api.deleteAssignment(id); await load(); } catch (e) { error = String(e); }
  }

  async function removeException(id) {
    try { await api.deleteException(id); await load(); } catch (e) { error = String(e); }
  }

  async function unblockDone() {
    showUnblock = false;
    await load();
  }

  async function confirmDeleteNow() {
    confirmDelete = false;
    try { await api.deleteGroup(groupId); dispatch("changed"); dispatch("removed"); } catch (e) { error = String(e); }
  }

  $: groupId, load();
  $: refreshToken, load();
</script>

<div class="toolbar">
  <span class="section-title">{group?.name ?? "…"}</span>
  <div class="spacer"></div>
</div>

<div class="pad">
  {#if error}<p class="error">{error}</p>{/if}

  {#if group}
    <p class="subtitle" style="margin:0 0 var(--space-4);">
      {members.length} poste(s) · {allAssignments.length} liste(s) affectée(s) · s'ajoute aux règles propres à chaque poste
    </p>

    <div class="group">
      <div class="group-title">Postes du groupe</div>
      {#if members.length === 0}
        <div class="row-line"><span class="subtitle">Aucun poste dans ce groupe.</span></div>
      {:else}
        {#each members as m}
          <div class="row-line">
            <span class="status-dot {m.revoked ? '' : m.last_ip ? 'ok' : ''}"></span>
            <span style="flex:1;">{m.display_name || m.hostname}</span>
            <span class="subtitle">{m.os}</span>
            <button class="toolbar-icon-btn" on:click={() => removeMember(m.id)} aria-label="Retirer du groupe"><Icon name="minus" /></button>
          </div>
        {/each}
      {/if}
      {#if nonMembers.length > 0}
        <div class="row-line">
          <select bind:value={addingAgentId} style="flex:1;">
            <option value="">+ Ajouter des postes au groupe…</option>
            {#each nonMembers as a}<option value={a.id}>{a.display_name || a.hostname}</option>{/each}
          </select>
          <button class="tbtn primary" on:click={addMember} disabled={!addingAgentId}>Ajouter</button>
        </div>
      {/if}
    </div>

    <div class="group">
      <div class="group-title">Listes bloquées pour ce groupe</div>
      {#if allAssignments.length === 0}
        <div class="row-line"><span class="subtitle">Aucune liste affectée.</span></div>
      {:else}
        {#each allAssignments as a}
          <div class="row-line">
            <span style="flex:1;">{listName(a.list_id)}</span>
            <span class="pill {a.schedule ? 'pill-accent' : 'pill-neutral'}">{scheduleSummary(a.schedule)}</span>
            <button on:click={() => (editingAssignment = a)}>Horaire…</button>
            <button class="toolbar-icon-btn" on:click={() => removeAssignment(a.id)} aria-label="Retirer"><Icon name="x" /></button>
          </div>
        {/each}
      {/if}
      <div class="row-line">
        <select bind:value={newAssignmentListId} style="flex:1;">
          <option value="">+ Bloquer une autre liste…</option>
          {#each lists as l}<option value={l.id}>{l.name}</option>{/each}
        </select>
        <button class="tbtn primary" on:click={addAssignment} disabled={!newAssignmentListId}>Ajouter</button>
      </div>
    </div>

    <div class="group">
      <div class="row-line" style="border-top:none;">
        <span style="flex:1; font-weight:650; font-size:var(--text-sm);">Déblocages temporaires</span>
        <button class="tbtn primary" on:click={() => (showUnblock = true)}>Débloquer…</button>
      </div>
      {#if allExceptions.length === 0}
        <div class="row-line"><span class="subtitle">Aucun déblocage en cours pour le groupe.</span></div>
      {:else}
        {#each allExceptions as ex}
          <div class="row-line">
            <span class="mono" style="flex:1;">{ex.domains.join(", ")}</span>
            <span class="pill pill-warn"><span class="pill-dot"></span>{remaining(ex.until)}</span>
            <button class="toolbar-icon-btn" on:click={() => removeException(ex.id)} aria-label="Rebloquer"><Icon name="x" /></button>
          </div>
        {/each}
      {/if}
    </div>

    <div class="row" style="justify-content:flex-end; margin-top:var(--space-4);">
      <button class="danger" on:click={() => (confirmDelete = true)}>Supprimer le groupe…</button>
    </div>
  {:else if !error}
    <p class="subtitle">Chargement…</p>
  {/if}
</div>

{#if confirmDelete}
  <ConfirmDialog
    icon="warning"
    title="Supprimer le groupe « {group?.name} » ?"
    description="Les affectations qui le ciblent seront aussi supprimées."
    confirmLabel="Supprimer"
    on:cancel={() => (confirmDelete = false)}
    on:confirm={confirmDeleteNow}
  />
{/if}

{#if showUnblock}
  <UnblockDialog
    targetKind="group"
    targetId={groupId}
    targetLabel={group?.name}
    on:close={() => (showUnblock = false)}
    on:done={unblockDone}
  />
{/if}

{#if editingAssignment}
  <ScheduleDialog
    assignmentId={editingAssignment.id}
    title="Bloquer « {listName(editingAssignment.list_id)} » pour le groupe {group?.name}"
    initialSchedule={editingAssignment.schedule}
    on:close={() => (editingAssignment = null)}
    on:saved={scheduleSaved}
  />
{/if}
