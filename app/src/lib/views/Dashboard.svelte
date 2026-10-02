<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";

  export let refreshToken = 0;

  const dispatch = createEventDispatcher();

  const WARNING_KINDS = new Set(["integrity_violation_detected", "manual_stop_detected"]);
  const TIMELINE_HOURS = 24;

  let agents = [];
  let groups = [];
  let lists = [];
  let fullHistory = [];
  let transitions = [];
  let liveStatus = {};
  let activeExceptions = [];
  let timeline = [];
  let scanning = false;
  let scanResults = [];
  let loadError = "";
  let bundledVersion = null;
  let updatingAll = false;

  async function load() {
    loadError = "";
    try {
      const [a, g, l, h, t, live, exc, tl] = await Promise.all([
        api.listAgents(),
        api.listGroups(),
        api.listLists(),
        api.recentHistory(200),
        api.upcomingTransitions(8),
        api.refreshAgentsStatus(),
        api.listExceptions(),
        api.fleetTimeline(TIMELINE_HOURS),
      ]);
      agents = a;
      groups = g;
      lists = l;
      fullHistory = h;
      transitions = t;
      liveStatus = Object.fromEntries(live.map((r) => [r.agent_id, r.online]));
      activeExceptions = exc.filter((e) => new Date(e.until).getTime() > Date.now());
      timeline = tl;
    } catch (e) {
      loadError = String(e);
    }
  }

  async function scan() {
    scanning = true;
    try {
      scanResults = await api.discoverAndSyncAll(3000);
      if (await api.getAutoUpdateAgents()) {
        await api.updateAllOutdatedAgents();
      }
    } catch (e) {
      loadError = String(e);
    } finally {
      scanning = false;
      await load();
    }
  }

  async function updateAll() {
    updatingAll = true;
    try {
      await api.updateAllOutdatedAgents();
      await load();
    } catch (e) {
      loadError = String(e);
    } finally {
      updatingAll = false;
    }
  }

  async function updateOne(agent) {
    if (!agent.last_ip) return;
    try { await api.updateAgent(agent.id, `${agent.last_ip}:7878`); await load(); } catch (e) { loadError = String(e); }
  }

  async function syncOne(agentId) {
    const a = agents.find((x) => x.id === agentId);
    if (!a?.last_ip) return;
    try { await api.syncAgent(agentId, `${a.last_ip}:7878`); await load(); } catch (e) { loadError = String(e); }
  }

  function goTo(agentId) {
    dispatch("navigate", { kind: "agent", id: agentId });
  }

  function exceptionTargetLabel(ex) {
    if (ex.target_kind === "group") {
      return groups.find((g) => g.id === ex.target_id)?.name ?? ex.target_id;
    }
    const a = agents.find((x) => x.id === ex.target_id);
    return a ? a.display_name || a.hostname : ex.target_id;
  }

  function remaining(until) {
    const ms = new Date(until).getTime() - Date.now();
    if (ms <= 0) return "expiré";
    const min = Math.round(ms / 60_000);
    return min < 60 ? `${min} min` : `${Math.floor(min / 60)} h ${min % 60} min`;
  }

  $: history = fullHistory.slice(0, 8);
  $: onlineCount = agents.filter((a) => !a.revoked && liveStatus[a.id] === true).length;
  $: toHandle = scanResults.filter((r) => !r.ok || r.integrity_ok === false);
  $: driftCount = toHandle.filter((r) => r.integrity_ok === false).length;
  $: since24h = Date.now() - 24 * 3600_000;
  $: alterations24h = fullHistory.filter((h) => WARNING_KINDS.has(h.kind) && new Date(h.at).getTime() >= since24h).length;
  $: outdatedAgents = bundledVersion
    ? agents.filter((a) => a.agent_version && a.agent_version !== "unknown" && a.agent_version !== bundledVersion)
    : [];

  // One merged, severity-ordered list — a poste that's both outdated and
  // drifted only needs one row, not two competing cards for the same
  // underlying "this needs you" signal.
  $: attentionItems = (() => {
    const items = [];
    for (const r of toHandle) {
      items.push({
        agentId: r.agent_id, title: r.hostname, kind: r.ok ? "warn" : "danger",
        detail: r.detail, actionLabel: "Synchroniser", action: () => syncOne(r.agent_id),
      });
    }
    for (const a of outdatedAgents) {
      if (items.some((i) => i.agentId === a.id)) continue;
      items.push({
        agentId: a.id, title: a.display_name || a.hostname, kind: "warn",
        detail: `Agent ${a.agent_version} · v${bundledVersion} disponible`,
        actionLabel: "Mettre à jour", action: () => updateOne(a),
      });
    }
    return items;
  })();

  // Positions a row's blocked segments as left/width percentages of the
  // [now, now + TIMELINE_HOURS] window the backend already bounded them
  // to — open stretches need no element, they're just the track's bg.
  function blockedSegmentStyles(row) {
    const windowStart = Date.now();
    const windowMs = TIMELINE_HOURS * 3600_000;
    return row.segments
      .filter((s) => s.blocked)
      .map((s) => {
        const left = Math.max(0, (new Date(s.from).getTime() - windowStart) / windowMs) * 100;
        const right = Math.min(1, (new Date(s.to).getTime() - windowStart) / windowMs) * 100;
        return `left:${left}%; width:${Math.max(0, right - left)}%;`;
      });
  }

  onMount(async () => {
    bundledVersion = await api.bundledAgentVersion();
    if (await api.getScanOnLaunch().catch(() => true)) {
      await scan();
    } else {
      await load();
    }
  });
  $: refreshToken, load();
</script>

<div class="toolbar">
  <span class="section-title">Aperçu</span>
  <div class="spacer"></div>
  {#if agents.length > 0}
    <button class="toolbar-icon-btn" on:click={scan} disabled={scanning} aria-label="Rechercher et synchroniser">
      <Icon name="refresh" />
    </button>
  {/if}
</div>

<div class="pad">
  {#if loadError}<p class="error">{loadError}</p>{/if}

  {#if agents.length === 0 && !loadError}
    <div class="empty empty-page">
      <Icon name="monitor" large />
      <h2>Ajoutez votre premier poste</h2>
      <p class="subtitle">HomeGuard pilote les ordinateurs Windows et Linux de la maison à distance, depuis ce Mac.</p>
      <ol class="steps">
        <li>Cliquez sur « Ajouter un poste »</li>
        <li>Collez la commande affichée sur l'ordinateur à protéger</li>
        <li>Il apparaît ici automatiquement, prêt à recevoir des règles</li>
      </ol>
      <button class="primary" on:click={() => dispatch("openEnroll")}>
        <span class="row" style="gap:5px;"><Icon name="plus" />Ajouter un poste</span>
      </button>
    </div>
  {:else}

  <div class="banner">
    <span class="banner-icon {attentionItems.length > 0 ? 'warn' : ''}">
      <Icon name={attentionItems.length > 0 ? "warning" : "shield"} />
    </span>
    <div style="flex:1;">
      <div class="banner-title">
        {#if attentionItems.length > 0}
          {attentionItems.length} poste{attentionItems.length > 1 ? "s" : ""} à traiter sur {agents.length}
        {:else}
          {onlineCount} poste{onlineCount > 1 ? "s" : ""} en ligne sur {agents.length}
        {/if}
      </div>
      <div class="banner-detail">
        {#if scanning}Recherche et synchronisation en cours…
        {:else if attentionItems.length > 0}{attentionItems.map((i) => i.title).join(", ")}
        {:else if activeExceptions.length > 0}
          {activeExceptions.length} déblocage{activeExceptions.length > 1 ? "s" : ""} temporaire{activeExceptions.length > 1 ? "s" : ""} en cours
        {:else}Tout est synchronisé.{/if}
      </div>
    </div>
    <button class="primary" on:click={scan} disabled={scanning}>{scanning ? "Recherche…" : "Tout synchroniser"}</button>
  </div>

  <div class="stat-strip">
    <div class="stat-seg"><div class="stat-value" class:ok={onlineCount > 0}>{onlineCount}</div><div class="stat-label">En ligne</div></div>
    <div class="stat-seg"><div class="stat-value" class:warn={driftCount > 0}>{driftCount}</div><div class="stat-label">En écart</div></div>
    <div class="stat-seg"><div class="stat-value" class:warn={alterations24h > 0}>{alterations24h}</div><div class="stat-label">Altérations 24 h</div></div>
    <div class="stat-seg">
      <div class="stat-value" class:warn={outdatedAgents.length > 0}>{outdatedAgents.length}</div>
      <div class="stat-label">MàJ</div>
    </div>
  </div>

  {#if activeExceptions.length > 0}
    <div class="group">
      <div class="group-title">Déblocages en cours ({activeExceptions.length})</div>
      {#each activeExceptions as ex}
        <div class="row-line">
          <Icon name={ex.target_kind === "group" ? "users" : "monitor"} style="color:var(--text-faint);" />
          <span style="flex:1;">{exceptionTargetLabel(ex)}</span>
          <span class="mono subtitle">{ex.domains.join(", ")}</span>
          <span class="pill pill-warn"><span class="pill-dot"></span>{remaining(ex.until)}</span>
        </div>
      {/each}
    </div>
  {/if}

  {#if attentionItems.length > 0}
    <div class="group">
      <div class="group-title">À traiter</div>
      {#each attentionItems as item}
        <div class="row-line">
          <span class="attn-icon {item.kind}"><Icon name="warning" /></span>
          <div style="flex:1; min-width:0; cursor:pointer;" on:click={() => goTo(item.agentId)} on:keydown={(e) => e.key === "Enter" && goTo(item.agentId)} role="button" tabindex="0">
            <div class="attn-title">{item.title}</div>
            <div class="attn-detail">{item.detail}</div>
          </div>
          <button on:click={item.action}>{item.actionLabel}</button>
        </div>
      {/each}
    </div>
  {/if}

  {#if outdatedAgents.length > 0}
    <div class="row" style="justify-content:flex-end; margin:var(--space-3) 0;">
      <button class="primary" on:click={updateAll} disabled={updatingAll}>
        {updatingAll ? "Mise à jour en cours…" : `Mettre à jour tout (${outdatedAgents.length})`}
      </button>
    </div>
  {/if}

  {#if timeline.length > 0}
    <div class="group">
      <div class="group-title">Les prochaines 24 h</div>
      {#each timeline as row}
        <div class="timeline-row">
          <span class="timeline-label">« {row.list_name} » · {row.target_label}</span>
          <div class="timeline-track">
            {#each blockedSegmentStyles(row) as style}
              <span class="timeline-seg" style={style}></span>
            {/each}
          </div>
        </div>
      {/each}
      <div class="timeline-axis">
        <span>Maintenant</span><span>+6 h</span><span>+12 h</span><span>+18 h</span><span>+24 h</span>
      </div>
    </div>
  {/if}

  <div class="two-col">
    <div class="group" style="flex:1;">
      <div class="group-title">Prochaines transitions</div>
      {#if transitions.length === 0}
        <div class="empty"><Icon name="clock-forward" large /><p>Aucune liste avec horaire pour le moment.</p></div>
      {:else}
        {#each transitions as t}
          <div class="row-line">
            <span class="mono subtitle" style="width:100px;">
              {new Date(t.at).toLocaleString("fr-FR", { weekday: "short", hour: "2-digit", minute: "2-digit" })}
            </span>
            <span style="flex:1;">« {t.list_name} »</span>
            <span class="pill {t.becomes_blocked ? 'pill-danger' : 'pill-ok'}"><span class="pill-dot"></span>{t.becomes_blocked ? "bloquée" : "débloquée"}</span>
          </div>
        {/each}
      {/if}
    </div>

    <div class="group" style="flex:1;">
      <div class="group-title">Activité récente</div>
      {#if history.length === 0}
        <div class="empty"><Icon name="history" large /><p>Rien à afficher pour le moment.</p></div>
      {:else}
        {#each history as h}
          <div class="row-line">
            <span class="mono subtitle" style="width:90px;">{new Date(h.at).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" })}</span>
            <span>{h.message}</span>
          </div>
        {/each}
      {/if}
    </div>
  </div>
  {/if}
</div>
