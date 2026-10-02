<script>
  import { createEventDispatcher } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";
  import ConfirmDialog from "../ConfirmDialog.svelte";
  import UnblockDialog from "../UnblockDialog.svelte";
  import { autofocus } from "../actions.js";

  export let agentId;
  export let refreshToken = 0;

  const dispatch = createEventDispatcher();

  const DAYS = [
    { key: "mon", label: "L" }, { key: "tue", label: "M" }, { key: "wed", label: "M" },
    { key: "thu", label: "J" }, { key: "fri", label: "V" }, { key: "sat", label: "S" }, { key: "sun", label: "D" },
  ];

  let agent = null;
  let groups = [];
  let lists = [];
  let assignments = [];
  let exceptions = [];
  let memberGroupIds = [];
  let addingGroupId = "";
  let bundledVersion = null;
  let online = null; // null = checking, true/false once known
  let error = "";
  let notice = "";
  let busy = false;
  let updating = false;
  let menuOpen = false;
  let confirmUninstall = false;
  let renaming = false;
  let renameValue = "";
  let showUnblock = false;

  let newAssignmentListId = "";
  let scheduleMode = "always";
  let scheduleDays = Object.fromEntries(DAYS.map((d) => [d.key, false]));
  let scheduleFrom = "08:00";
  let scheduleTo = "18:00";

  async function load() {
    error = "";
    try {
      const [agents, g, l, a, e, mg] = await Promise.all([
        api.listAgents(),
        api.listGroups(),
        api.listLists(),
        api.assignmentsFor(agentId),
        api.exceptionsFor(agentId),
        api.agentGroupIds(agentId),
      ]);
      agent = agents.find((x) => x.id === agentId) ?? null;
      groups = g;
      lists = l;
      assignments = a;
      exceptions = e;
      memberGroupIds = mg;
    } catch (e) {
      error = String(e);
    }
    checkOnline();
  }

  async function checkOnline() {
    online = null;
    try {
      const results = await api.refreshAgentsStatus();
      const row = results.find((r) => r.agent_id === agentId);
      online = row ? row.online : false;
    } catch {
      online = null;
    }
  }

  function groupName(id) {
    return groups.find((g) => g.id === id)?.name ?? id;
  }

  $: memberGroups = groups.filter((g) => memberGroupIds.includes(g.id));
  $: joinableGroups = groups.filter((g) => !memberGroupIds.includes(g.id));

  async function joinGroup() {
    if (!addingGroupId) return;
    try {
      await api.addGroupMember(addingGroupId, agentId);
      addingGroupId = "";
      await load();
      dispatch("changed");
    } catch (e) { error = String(e); }
  }

  async function leaveGroup(groupId) {
    try {
      await api.removeGroupMember(groupId, agentId);
      await load();
      dispatch("changed");
    } catch (e) { error = String(e); }
  }

  function viewInHistory() {
    menuOpen = false;
    dispatch("navigate", { kind: "history", agentId });
  }

  function listName(id) {
    return lists.find((l) => l.id === id)?.name ?? id;
  }

  // Where a blocked-list assignment actually comes from: this poste
  // directly, or one of its groups — both are already in `assignments`
  // (assignments_for resolves the union), distinguished by target_kind.
  function provenance(a) {
    return a.target_kind === "group" ? `via ${groupName(a.target_id)}` : "ce poste";
  }

  function scheduleSummary(schedule) {
    if (!schedule || schedule.length === 0) return "Toujours";
    const w = schedule[0];
    const days = w.days.map((d) => DAYS.find((x) => x.key === d)?.label ?? d).join("");
    return `${days} ${w.from.slice(0, 5)}–${w.to.slice(0, 5)}`;
  }

  function remaining(until) {
    const ms = new Date(until).getTime() - Date.now();
    if (ms <= 0) return "expiré";
    const min = Math.round(ms / 60_000);
    if (min < 60) return `${min} min`;
    return `${Math.floor(min / 60)} h ${min % 60} min`;
  }

  async function sync() {
    if (!agent?.last_ip) { error = "Adresse IP inconnue — lancez une recherche depuis l'Aperçu."; return; }
    busy = true; error = "";
    try {
      await api.syncAgent(agentId, `${agent.last_ip}:7878`);
      await load();
      dispatch("changed");
    } catch (e) { error = String(e); } finally { busy = false; }
  }

  async function updatePoste() {
    if (!agent?.last_ip) { error = "Adresse IP inconnue — lancez une recherche depuis l'Aperçu."; return; }
    updating = true; error = ""; notice = "";
    try {
      const result = await api.updateAgent(agentId, `${agent.last_ip}:7878`);
      await load();
      if (result.came_back_online && result.matches_expected) notice = `À jour (v${result.reported_version}).`;
      else if (result.came_back_online) notice = `Revenu en ligne, version ${result.reported_version} inattendue.`;
      else error = "Aucune réponse après la mise à jour — à vérifier manuellement.";
    } catch (e) { error = String(e); } finally { updating = false; }
  }

  function startRename() {
    renameValue = agent.display_name || agent.hostname;
    renaming = true;
    menuOpen = false;
  }

  async function commitRename() {
    if (!renameValue.trim()) { renaming = false; return; }
    try {
      await api.setAgentDisplayName(agentId, renameValue.trim());
      renaming = false;
      await load();
      dispatch("changed");
    } catch (e) { error = String(e); }
  }

  async function confirmUninstallNow() {
    confirmUninstall = false;
    try {
      await api.removeAgent(agentId, agent?.last_ip ? `${agent.last_ip}:7878` : null);
      dispatch("changed");
      dispatch("removed");
    } catch (e) { error = String(e); }
  }

  function buildSchedule() {
    if (scheduleMode === "always") return null;
    const days = DAYS.map((d) => d.key).filter((k) => scheduleDays[k]);
    if (days.length === 0) return null;
    return [{ days, from: `${scheduleFrom}:00`, to: `${scheduleTo}:00` }];
  }

  async function addAssignment() {
    if (!newAssignmentListId) return;
    try {
      await api.createAssignment(newAssignmentListId, "agent", agentId, buildSchedule());
      newAssignmentListId = "";
      scheduleMode = "always";
      scheduleDays = Object.fromEntries(DAYS.map((d) => [d.key, false]));
      await load();
    } catch (e) { error = String(e); }
  }

  async function removeAssignment(id) {
    try { await api.deleteAssignment(id); await load(); } catch (e) { error = String(e); }
  }

  async function unblockDone() {
    showUnblock = false;
    await load();
  }

  async function removeException(id) {
    try { await api.deleteException(id); await load(); } catch (e) { error = String(e); }
  }

  $: isOutdated = bundledVersion && agent?.agent_version && agent.agent_version !== "unknown" && agent.agent_version !== bundledVersion;

  $: agentId, load();
  $: refreshToken, load();
  api.bundledAgentVersion().then((v) => (bundledVersion = v));

  function statusPill() {
    if (!agent) return { cls: "pill-neutral", label: "…" };
    if (agent.revoked) return { cls: "pill-danger", label: "Révoqué" };
    if (!agent.last_ip) return { cls: "pill-neutral", label: "IP inconnue" };
    if (online === true) return { cls: "pill-ok", label: "En ligne" };
    if (online === false) return { cls: "pill-danger", label: "Hors ligne" };
    return { cls: "pill-neutral", label: "Vérification…" };
  }
</script>

<svelte:window on:click={() => (menuOpen = false)} />

<div class="toolbar">
  {#if renaming}
    <input
      use:autofocus
      bind:value={renameValue}
      on:keydown={(e) => e.key === "Enter" && commitRename()}
      on:blur={commitRename}
      style="font-weight:650; font-size:var(--text-md); flex:0 1 240px;"
    />
  {:else}
    <span class="section-title">{agent?.display_name || agent?.hostname || "…"}</span>
  {/if}
  <span class="pill {statusPill().cls}"><span class="pill-dot"></span>{statusPill().label}</span>
  <div class="spacer"></div>
  <button class="tbtn" on:click={sync} disabled={busy}>{busy ? "Synchronisation…" : "Synchroniser"}</button>
  <div style="position:relative;">
    <button class="toolbar-icon-btn" on:click|stopPropagation={() => (menuOpen = !menuOpen)} aria-label="Plus d'actions">
      <Icon name="more" />
    </button>
    {#if menuOpen}
      <div
        class="group"
        role="menu"
        tabindex="-1"
        style="position:absolute; right:0; top:32px; width:220px; box-shadow:var(--shadow-pop); z-index:5; padding:4px 0;"
        on:click|stopPropagation
        on:keydown={(e) => e.key === "Escape" && (menuOpen = false)}
      >
        <button class="nav-item" style="color:var(--text); width:100%;" on:click={startRename}>Renommer…</button>
        <button class="nav-item" style="color:var(--text); width:100%;" on:click={() => { menuOpen = false; sync(); }}>Synchroniser maintenant</button>
        <button class="nav-item" style="color:var(--text); width:100%;" on:click={() => { menuOpen = false; updatePoste(); }}>Mettre à jour l'agent</button>
        <button class="nav-item" style="color:var(--text); width:100%;" on:click={viewInHistory}>Voir dans l'historique</button>
        <button class="nav-item" style="color:var(--danger); width:100%;" on:click={() => { menuOpen = false; confirmUninstall = true; }}>Désinstaller à distance…</button>
      </div>
    {/if}
  </div>
</div>

<div class="pad">
  {#if error}<p class="error">{error}</p>{/if}
  {#if notice}<p class="row" style="gap:6px; color:var(--ok); font-size:var(--text-sm);"><Icon name="check" />{notice}</p>{/if}

  {#if agent}
    <div class="info-row">
      <div class="field"><div class="k">Système</div><div class="v">{agent.os}</div></div>
      <div class="field"><div class="k">Adresse IP</div><div class="v mono">{agent.last_ip ?? "inconnue"}</div></div>
      <div class="field"><div class="k">Politique</div><div class="v">v{agent.policy_version}</div></div>
      <div class="field"><div class="k">Version agent</div><div class="v">{agent.agent_version ?? "inconnue"}</div></div>
    </div>

    {#if isOutdated}
      <div class="row-line" style="background:var(--warn-weak); border-radius:var(--radius-md); border-top:none; margin-bottom:var(--space-4);">
        <Icon name="warning" style="color:var(--warn);" />
        <span style="flex:1; font-size:var(--text-sm);">Mise à jour disponible (v{bundledVersion})</span>
        <button class="tbtn primary" on:click={updatePoste} disabled={updating}>{updating ? "Mise à jour…" : "Mettre à jour"}</button>
      </div>
    {/if}

    <div class="group">
      <div class="group-title">Listes bloquées</div>
      {#if assignments.length === 0}
        <div class="row-line"><span class="subtitle">Aucune liste affectée.</span></div>
      {:else}
        {#each assignments as a}
          <div class="row-line">
            <span style="flex:1;">{listName(a.list_id)}</span>
            <span class="pill pill-neutral">{provenance(a)}</span>
            <span class="pill {a.schedule ? 'pill-accent' : 'pill-neutral'}">{scheduleSummary(a.schedule)}</span>
            <button class="toolbar-icon-btn" on:click={() => removeAssignment(a.id)} aria-label="Retirer"><Icon name="x" /></button>
          </div>
        {/each}
      {/if}
      <div class="row-line">
        <select bind:value={newAssignmentListId} style="flex:1;">
          <option value="">+ Ajouter une liste…</option>
          {#each lists as l}<option value={l.id}>{l.name}</option>{/each}
        </select>
        <label class="row" style="gap:4px; font-size:var(--text-sm);"><input type="radio" bind:group={scheduleMode} value="always" />Toujours</label>
        <label class="row" style="gap:4px; font-size:var(--text-sm);"><input type="radio" bind:group={scheduleMode} value="scheduled" />Horaire</label>
        <button class="tbtn primary" on:click={addAssignment} disabled={!newAssignmentListId}>Ajouter</button>
      </div>
      {#if scheduleMode === "scheduled"}
        <div class="row-line" style="flex-wrap:wrap;">
          {#each DAYS as d}
            <label class="row" style="gap:2px; font-size:var(--text-sm);"><input type="checkbox" bind:checked={scheduleDays[d.key]} />{d.label}</label>
          {/each}
          <input type="time" bind:value={scheduleFrom} /><span>→</span><input type="time" bind:value={scheduleTo} />
        </div>
      {/if}
    </div>

    <div class="group">
      <div class="row-line" style="border-top:none;">
        <span style="flex:1; font-weight:650; font-size:var(--text-sm);">Déblocages temporaires</span>
        <button class="tbtn primary" on:click={() => (showUnblock = true)}>Débloquer…</button>
      </div>
      {#if exceptions.length === 0}
        <div class="row-line"><span class="subtitle">Aucun déblocage en cours.</span></div>
      {:else}
        {#each exceptions as ex}
          <div class="row-line">
            <span class="mono" style="flex:1;">{ex.domains.join(", ")}</span>
            <span class="pill pill-warn"><span class="pill-dot"></span>{remaining(ex.until)}</span>
            <button class="toolbar-icon-btn" on:click={() => removeException(ex.id)} aria-label="Rebloquer"><Icon name="x" /></button>
          </div>
        {/each}
      {/if}
    </div>

    <div class="group">
      <div class="group-title">Groupes</div>
      <div class="row-line" style="flex-wrap:wrap;">
        {#if memberGroups.length === 0}
          <span class="subtitle">Ce poste n'appartient à aucun groupe.</span>
        {:else}
          {#each memberGroups as g}
            <span class="pill pill-accent">
              {g.name}
              <button class="toolbar-icon-btn" style="width:14px; height:14px; padding:0;" on:click={() => leaveGroup(g.id)} aria-label="Retirer de {g.name}"><Icon name="x" /></button>
            </span>
          {/each}
        {/if}
      </div>
      {#if joinableGroups.length > 0}
        <div class="row-line">
          <select bind:value={addingGroupId} style="flex:1;">
            <option value="">+ Ajouter à un groupe…</option>
            {#each joinableGroups as g}<option value={g.id}>{g.name}</option>{/each}
          </select>
          <button on:click={joinGroup} disabled={!addingGroupId}>Ajouter</button>
        </div>
      {/if}
    </div>
  {:else if !error}
    <p class="subtitle">Chargement…</p>
  {/if}
</div>

{#if confirmUninstall}
  <ConfirmDialog
    icon="warning"
    title="Désinstaller HomeGuard de « {agent?.display_name || agent?.hostname} » ?"
    description="Le fichier hosts et les réglages des navigateurs seront restaurés, puis le certificat du poste sera révoqué. Pour le protéger à nouveau, il faudra relancer la commande d'installation."
    confirmLabel="Désinstaller"
    on:cancel={() => (confirmUninstall = false)}
    on:confirm={confirmUninstallNow}
  />
{/if}

{#if showUnblock}
  <UnblockDialog
    targetKind="agent"
    targetId={agentId}
    targetLabel={agent?.display_name || agent?.hostname}
    on:close={() => (showUnblock = false)}
    on:done={unblockDone}
  />
{/if}
