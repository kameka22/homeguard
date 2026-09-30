<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";

  export let refreshToken = 0;

  let agents = [];
  let groups = [];
  let lists = [];
  let history = [];
  let transitions = [];
  let scanning = false;
  let scanResults = [];
  let loadError = "";
  let bundledVersion = null;
  let updatingAll = false;
  let updateResults = [];

  async function load() {
    loadError = "";
    try {
      [agents, groups, lists, history, transitions] = await Promise.all([
        api.listAgents(),
        api.listGroups(),
        api.listLists(),
        api.recentHistory(8),
        api.upcomingTransitions(8),
      ]);
    } catch (e) {
      loadError = String(e);
    }
  }

  async function scan() {
    scanning = true;
    scanResults = [];
    try {
      scanResults = await api.discoverAndSyncAll(3000);
    } catch (e) {
      loadError = String(e);
    } finally {
      scanning = false;
      await load();
    }
  }

  async function updateAll() {
    updatingAll = true;
    updateResults = [];
    try {
      // Sequential server-side, one poste at a time (see
      // `update_all_outdated_agents`) — can take a while if several
      // postes are outdated, each waited on for up to ~17s.
      updateResults = await api.updateAllOutdatedAgents();
    } catch (e) {
      loadError = String(e);
    } finally {
      updatingAll = false;
      await load();
    }
  }

  $: driftCount = scanResults.filter((r) => r.integrity_ok === false).length;
  $: toHandle = scanResults.filter((r) => !r.ok || r.integrity_ok === false);
  $: outdatedAgents = bundledVersion
    ? agents.filter((a) => a.agent_version && a.agent_version !== "unknown" && a.agent_version !== bundledVersion)
    : [];

  onMount(async () => {
    load();
    bundledVersion = await api.bundledAgentVersion();
  });
  $: refreshToken, load();
</script>

<h1>Tableau de bord</h1>
<p class="subtitle">Vue d'ensemble du parc</p>

{#if loadError}
  <p class="error">{loadError}</p>
{/if}

<div class="grid-4">
  <div class="card stat-tile">
    <span class="icon-badge"><Icon name="monitor" /></span>
    <div>
      <div class="stat-value">{agents.length}</div>
      <div class="stat-label">Postes connus</div>
    </div>
  </div>
  <div class="card stat-tile">
    <span class="icon-badge"><Icon name="users" /></span>
    <div>
      <div class="stat-value">{groups.length}</div>
      <div class="stat-label">Groupes</div>
    </div>
  </div>
  <div class="card stat-tile">
    <span class="icon-badge"><Icon name="list" /></span>
    <div>
      <div class="stat-value">{lists.length}</div>
      <div class="stat-label">Listes</div>
    </div>
  </div>
  <div class="card stat-tile" class:warn={driftCount > 0}>
    <span class="icon-badge"><Icon name="warning" /></span>
    <div>
      <div class="stat-value" style={driftCount > 0 ? "color: var(--warn)" : ""}>{driftCount}</div>
      <div class="stat-label">Écarts (dernier scan)</div>
    </div>
  </div>
</div>

{#if toHandle.length > 0}
  <div class="card" style="margin-bottom:16px;">
    <h2>
      <span class="row" style="gap:6px; display:inline-flex;"><Icon name="warning" />À traiter</span>
    </h2>
    <table>
      <tbody>
        {#each toHandle as r}
          <tr>
            <td style="width:160px;">{r.hostname}</td>
            <td>
              <span class="pill {r.ok ? 'pill-warn' : 'pill-danger'}"><span class="pill-dot" />{r.detail}</span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<div class="card">
  <div class="row" style="margin-bottom: 10px;">
    <h2 style="margin:0;">Rechercher les postes sur le réseau</h2>
    <div class="spacer" />
    <button class="primary" on:click={scan} disabled={scanning}>
      <span class="row" style="gap:5px;"><Icon name="search" />{scanning ? "Recherche…" : "Rechercher et synchroniser"}</span>
    </button>
  </div>

  {#if scanResults.length === 0 && !scanning}
    <p class="subtitle" style="margin:0;">
      Recherche les agents annoncés sur le réseau local (mDNS) et leur envoie la politique à jour.
    </p>
  {:else}
    <table>
      <thead>
        <tr><th>Poste</th><th>Statut</th></tr>
      </thead>
      <tbody>
        {#each scanResults as r}
          <tr>
            <td>{r.hostname}</td>
            <td>
              <span class="pill {r.ok ? (r.integrity_ok === false ? 'pill-warn' : 'pill-ok') : 'pill-danger'}">
                <span class="pill-dot" />{r.detail}
              </span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<div class="card" style="margin-top:16px;">
  <div class="row" style="margin-bottom: 10px;">
    <h2 style="margin:0;">Mises à jour</h2>
    <div class="spacer" />
    <button class="primary" on:click={updateAll} disabled={updatingAll || outdatedAgents.length === 0}>
      <span class="row" style="gap:5px;">
        <Icon name="clock-forward" />{updatingAll ? "Mise à jour en cours…" : `Mettre à jour tout (${outdatedAgents.length})`}
      </span>
    </button>
  </div>

  {#if updateResults.length === 0 && !updatingAll}
    {#if bundledVersion === null}
      <p class="subtitle" style="margin:0;">Aucun binaire agent embarqué dans cette build de l'application.</p>
    {:else if outdatedAgents.length === 0}
      <p class="subtitle" style="margin:0;">Tous les postes connus sont à jour (v{bundledVersion}).</p>
    {:else}
      <p class="subtitle" style="margin:0;">
        {outdatedAgents.length} poste(s) pas encore en v{bundledVersion} : {outdatedAgents.map((a) => a.hostname).join(", ")}.
      </p>
    {/if}
  {:else}
    <table>
      <thead>
        <tr><th>Poste</th><th>Résultat</th></tr>
      </thead>
      <tbody>
        {#each updateResults as r}
          <tr>
            <td>{r.hostname}</td>
            <td>
              <span class="pill {r.ok ? 'pill-ok' : 'pill-danger'}"><span class="pill-dot" />{r.detail}</span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<div style="display:grid; grid-template-columns: 1fr 1fr; gap:16px; margin-top:16px;">
  <div class="card">
    <h2>Prochaines transitions</h2>
    {#if transitions.length === 0}
      <div class="empty">
        <Icon name="clock-forward" large />
        <p>Aucune liste avec horaire pour le moment.</p>
      </div>
    {:else}
      <table>
        <tbody>
          {#each transitions as t}
            <tr>
              <td style="width:120px; color:var(--text-dim);" class="mono">
                {new Date(t.at).toLocaleString("fr-FR", { weekday: "short", hour: "2-digit", minute: "2-digit" })}
              </td>
              <td>« {t.list_name} »</td>
              <td>
                <span class="pill {t.becomes_blocked ? 'pill-danger' : 'pill-ok'}">
                  <span class="pill-dot" />{t.becomes_blocked ? "bloquée" : "débloquée"}
                </span>
              </td>
              <td style="color:var(--text-dim);">{t.target_label}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>

  <div class="card">
    <h2>Activité récente</h2>
    {#if history.length === 0}
      <div class="empty">
        <Icon name="history" large />
        <p>Rien à afficher pour le moment.</p>
      </div>
    {:else}
      <table>
        <tbody>
          {#each history as h}
            <tr>
              <td style="color: var(--text-dim); width: 140px;" class="mono">{new Date(h.at).toLocaleString("fr-FR")}</td>
              <td>{h.message}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>
