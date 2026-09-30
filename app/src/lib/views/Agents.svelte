<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";

  export let refreshToken = 0;

  const DAYS = [
    { key: "mon", label: "L" },
    { key: "tue", label: "M" },
    { key: "wed", label: "M" },
    { key: "thu", label: "J" },
    { key: "fri", label: "V" },
    { key: "sat", label: "S" },
    { key: "sun", label: "D" },
  ];
  const DURATIONS = [
    { label: "15 min", minutes: 15 },
    { label: "30 min", minutes: 30 },
    { label: "1 h", minutes: 60 },
    { label: "2 h", minutes: 120 },
  ];

  let agents = [];
  let lists = [];
  // agentId -> true (online)/false (offline) once `refreshAgentsStatus`
  // answers for it; absent while the check hasn't run/returned yet.
  let liveStatus = {};
  // The agent version bundled in this build of the app (from
  // resources/agents/version.txt), or null if none is staged — see
  // `isOutdated`.
  let bundledVersion = null;
  let selected = null;
  let assignments = [];
  let exceptions = [];
  let error = "";
  let notice = "";
  let busy = false;
  let updating = false;

  let newAssignmentListId = "";
  let scheduleMode = "always"; // "always" | "scheduled"
  let scheduleDays = Object.fromEntries(DAYS.map((d) => [d.key, false]));
  let scheduleFrom = "08:00";
  let scheduleTo = "18:00";

  let exceptionDomains = "";
  let exceptionMinutes = 60;

  async function load() {
    error = "";
    try {
      [agents, lists] = await Promise.all([api.listAgents(), api.listLists()]);
      if (selected) {
        const stillThere = agents.find((a) => a.id === selected.id);
        if (stillThere) await select(stillThere);
        else selected = null;
      }
    } catch (e) {
      error = String(e);
    }
    // Not awaited: the poste list renders immediately, online/offline
    // badges settle in a moment later as each probe answers (or times out).
    refreshStatus();
  }

  async function refreshStatus() {
    try {
      const results = await api.refreshAgentsStatus();
      const next = {};
      for (const r of results) next[r.agent_id] = r.online;
      liveStatus = next;
    } catch (e) {
      // Best-effort: a failed probe pass shouldn't hide the poste list
      // itself, so this stays silent — badges just stay at "Vérification…".
    }
  }

  async function select(agent) {
    selected = agent;
    try {
      [assignments, exceptions] = await Promise.all([api.assignmentsFor(agent.id), api.exceptionsFor(agent.id)]);
    } catch (e) {
      error = String(e);
    }
  }

  async function sync(agent) {
    if (!agent.last_ip) {
      error = "Adresse IP inconnue pour ce poste — lancez d'abord une recherche depuis le tableau de bord.";
      return;
    }
    busy = true;
    error = "";
    try {
      await api.syncAgent(agent.id, `${agent.last_ip}:7878`);
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function updatePoste(agent) {
    if (!agent.last_ip) {
      error = "Adresse IP inconnue pour ce poste — lancez d'abord une recherche depuis le tableau de bord.";
      return;
    }
    if (!confirm(`Envoyer la mise à jour (v${bundledVersion}) à ${agent.hostname} ? L'agent va redémarrer.`)) return;
    updating = true;
    error = "";
    notice = "";
    try {
      // Blocks for several seconds: the backend pushes the binary, then
      // waits for the poste to restart and reconnects to confirm it came
      // back on the new version before returning.
      const result = await api.updateAgent(agent.id, `${agent.last_ip}:7878`);
      // `load()` resets `error`/leaves `notice` alone at its start, so set
      // the outcome message *after* it, not before — otherwise a message
      // set here would be wiped out the instant `load()` runs.
      await load();
      if (result.came_back_online && result.matches_expected) {
        notice = `${agent.hostname} est à jour (v${result.reported_version}).`;
      } else if (result.came_back_online) {
        notice = `${agent.hostname} est revenu en ligne, mais annonce la version ${result.reported_version} — pas celle attendue.`;
      } else {
        error = `${agent.hostname} n'a pas répondu après l'envoi de la mise à jour — à vérifier manuellement.`;
      }
    } catch (e) {
      error = String(e);
    } finally {
      updating = false;
    }
  }

  async function rename(agent, event) {
    const name = event.target.value.trim();
    if (!name) return;
    try {
      await api.setAgentDisplayName(agent.id, name);
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  function buildSchedule() {
    if (scheduleMode === "always") return null;
    const days = DAYS.map((d) => d.key).filter((k) => scheduleDays[k]);
    if (days.length === 0) return null;
    return [{ days, from: `${scheduleFrom}:00`, to: `${scheduleTo}:00` }];
  }

  async function addAssignment() {
    if (!selected || !newAssignmentListId) return;
    try {
      await api.createAssignment(newAssignmentListId, "agent", selected.id, buildSchedule());
      newAssignmentListId = "";
      scheduleMode = "always";
      scheduleDays = Object.fromEntries(DAYS.map((d) => [d.key, false]));
      await select(selected);
    } catch (e) {
      error = String(e);
    }
  }

  async function removeAssignment(assignmentId) {
    try {
      await api.deleteAssignment(assignmentId);
      await select(selected);
    } catch (e) {
      error = String(e);
    }
  }

  function scheduleSummary(schedule) {
    if (!schedule || schedule.length === 0) return "toujours";
    const w = schedule[0];
    const days = w.days.map((d) => DAYS.find((x) => x.key === d)?.label ?? d).join("");
    return `${days} ${w.from.slice(0, 5)}–${w.to.slice(0, 5)}`;
  }

  async function addException() {
    if (!selected || !exceptionDomains.trim()) return;
    const domains = exceptionDomains
      .split(/[\n,]+/)
      .map((d) => d.trim().toLowerCase())
      .filter(Boolean);
    if (domains.length === 0) return;
    const until = new Date(Date.now() + exceptionMinutes * 60_000).toISOString();
    try {
      await api.createException("agent", selected.id, domains, until);
      exceptionDomains = "";
      await select(selected);
    } catch (e) {
      error = String(e);
    }
  }

  async function removeException(exceptionId) {
    try {
      await api.deleteException(exceptionId);
      await select(selected);
    } catch (e) {
      error = String(e);
    }
  }

  function remaining(until) {
    const ms = new Date(until).getTime() - Date.now();
    if (ms <= 0) return "expiré";
    const min = Math.round(ms / 60_000);
    if (min < 60) return `${min} min restantes`;
    return `${Math.floor(min / 60)} h ${min % 60} min restantes`;
  }

  async function removePoste(agent) {
    if (!confirm(`Retirer ${agent.hostname} ? Le hosts sera restauré et le certificat révoqué.`)) return;
    try {
      await api.removeAgent(agent.id, agent.last_ip ? `${agent.last_ip}:7878` : null);
      selected = null;
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  function listName(id) {
    return lists.find((l) => l.id === id)?.name ?? id;
  }

  function statusPill(agent) {
    if (agent.revoked) return { cls: "pill-danger", label: "Révoqué" };
    if (!agent.last_ip) return { cls: "pill-neutral", label: "IP inconnue" };
    const online = liveStatus[agent.id];
    if (online === true) return { cls: "pill-ok", label: "En ligne" };
    if (online === false) return { cls: "pill-danger", label: "Hors ligne" };
    return { cls: "pill-neutral", label: "Vérification…" };
  }

  function isOutdated(agent) {
    return (
      !!bundledVersion &&
      !!agent.agent_version &&
      agent.agent_version !== "unknown" &&
      agent.agent_version !== bundledVersion
    );
  }

  onMount(async () => {
    load();
    bundledVersion = await api.bundledAgentVersion();
  });
  $: refreshToken, load();
</script>

<h1>Postes</h1>
<p class="subtitle">{agents.length} poste(s) connu(s)</p>
{#if error}<p class="error">{error}</p>{/if}
{#if notice}
  <p class="row" style="gap:6px; color: var(--ok); margin: 0 0 var(--space-5);"><Icon name="check" />{notice}</p>
{/if}

<div style="display:grid; grid-template-columns: 280px 1fr; gap: 16px; align-items:start;">
  <div class="card" style="padding:0;">
    {#if agents.length === 0}
      <div class="empty" style="padding: var(--space-5) var(--space-3);">
        <Icon name="monitor" large />
        <p>Aucun poste. Utilisez « + Postes » pour en ajouter un.</p>
      </div>
    {:else}
      {#each agents as agent}
        <div
          class="row"
          role="button"
          tabindex="0"
          style="padding:10px 12px; border-bottom:1px solid var(--border); cursor:pointer; background:{selected?.id === agent.id ? 'var(--accent-weak)' : 'transparent'}"
          on:click={() => select(agent)}
          on:keydown={(e) => e.key === "Enter" && select(agent)}
        >
          <span class="pill {statusPill(agent).cls}"><span class="pill-dot" /></span>
          <div>
            <div style={selected?.id === agent.id ? "color:var(--accent); font-weight:600;" : ""}>{agent.display_name || agent.hostname}</div>
            <div style="color:var(--text-dim); font-size:var(--text-xs);">{agent.os} · {agent.last_ip ?? "IP inconnue"}</div>
          </div>
        </div>
      {/each}
    {/if}
  </div>

  <div class="card">
    {#if !selected}
      <div class="empty">
        <Icon name="monitor" large />
        <p>Sélectionnez un poste à gauche pour voir ses listes, ses horaires et ses déblocages.</p>
      </div>
    {:else}
      <div class="row" style="margin-bottom:8px;">
        <h2 style="margin:0;">{selected.hostname}</h2>
        <span class="pill {statusPill(selected).cls}"><span class="pill-dot" />{statusPill(selected).label}</span>
        <div class="spacer" />
        <button on:click={() => sync(selected)} disabled={busy}>{busy ? "Synchronisation…" : "Synchroniser"}</button>
        <button class="danger" on:click={() => removePoste(selected)}>Retirer</button>
      </div>
      <p class="subtitle mono">
        {selected.os} · {selected.last_ip ?? "IP inconnue"} · politique v{selected.policy_version}
        {#if selected.agent_version}· agent v{selected.agent_version}{/if}
      </p>
      {#if isOutdated(selected)}
        <div class="row" style="gap:8px; margin: 0 0 var(--space-3);">
          <span class="row" style="gap:5px; color:var(--warn);"><Icon name="warning" />Mise à jour disponible (v{bundledVersion})</span>
          <button on:click={() => updatePoste(selected)} disabled={updating}>
            {updating ? "Mise à jour et vérification…" : "Mettre à jour"}
          </button>
        </div>
      {/if}

      <label style="display:block; margin-bottom:16px;">
        Nom affiché
        <input value={selected.display_name ?? ""} placeholder={selected.hostname} on:change={(e) => rename(selected, e)} />
      </label>

      <h2>Listes bloquées</h2>
      {#if assignments.length === 0}
        <p class="subtitle" style="margin:0 0 var(--space-3);">Aucune liste affectée directement à ce poste.</p>
      {:else}
        <table>
          <tbody>
            {#each assignments as a}
              <tr>
                <td>{listName(a.list_id)}</td>
                <td>
                  <span class="pill {a.schedule ? 'pill-accent' : 'pill-neutral'}">{scheduleSummary(a.schedule)}</span>
                </td>
                <td style="width:32px; text-align:right;">
                  <button on:click={() => removeAssignment(a.id)} aria-label="Retirer"><Icon name="x" /></button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      <div class="row" style="margin-top:10px; align-items:flex-start;">
        <select bind:value={newAssignmentListId}>
          <option value="">Ajouter une liste…</option>
          {#each lists as l}
            <option value={l.id}>{l.name}</option>
          {/each}
        </select>
        <label class="row" style="gap:4px;">
          <input type="radio" bind:group={scheduleMode} value="always" /> Toujours
        </label>
        <label class="row" style="gap:4px;">
          <input type="radio" bind:group={scheduleMode} value="scheduled" /> Selon un horaire
        </label>
      </div>

      {#if scheduleMode === "scheduled"}
        <div class="row" style="margin-top:8px; flex-wrap:wrap;">
          {#each DAYS as d}
            <label class="row" style="gap:2px;">
              <input type="checkbox" bind:checked={scheduleDays[d.key]} />
              {d.label}
            </label>
          {/each}
          <input type="time" bind:value={scheduleFrom} />
          <span>→</span>
          <input type="time" bind:value={scheduleTo} />
        </div>
      {/if}

      <div class="row" style="margin-top:10px;">
        <div class="spacer" />
        <button class="primary" on:click={addAssignment} disabled={!newAssignmentListId}>Ajouter</button>
      </div>

      <h2 style="margin-top:20px;">Déblocages temporaires</h2>
      {#if exceptions.length === 0}
        <p class="subtitle" style="margin:0 0 var(--space-3);">Aucun déblocage en cours.</p>
      {:else}
        <table>
          <tbody>
            {#each exceptions as ex}
              <tr>
                <td class="mono">{ex.domains.join(", ")}</td>
                <td><span class="pill pill-warn"><span class="pill-dot" />{remaining(ex.until)}</span></td>
                <td style="width:32px; text-align:right;">
                  <button on:click={() => removeException(ex.id)} aria-label="Rebloquer"><Icon name="x" /></button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      <div class="row" style="margin-top:8px;">
        <input placeholder="twitch.tv, youtube.com…" bind:value={exceptionDomains} style="flex:1;" />
        <select bind:value={exceptionMinutes}>
          {#each DURATIONS as d}
            <option value={d.minutes}>{d.label}</option>
          {/each}
        </select>
        <button on:click={addException} disabled={!exceptionDomains.trim()}>
          <span class="row" style="gap:5px;"><Icon name="clock-forward" />Débloquer</span>
        </button>
      </div>
    {/if}
  </div>
</div>
