<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";

  export let refreshToken = 0;
  // Set when arriving via "Voir dans l'historique" from a poste's page —
  // seeds the Poste dropdown below so it's pre-filtered; the dropdown
  // itself is then the only thing in charge of that filter.
  export let filterAgentId = null;

  const dispatch = createEventDispatcher();

  const WARNING_KINDS = new Set(["integrity_violation_detected", "manual_stop_detected"]);
  const KIND_LABELS = {
    enrolled: "Enrôlement",
    removed: "Poste retiré",
    policy_pushed: "Politique envoyée",
    update_pushed: "Mise à jour envoyée",
    update_confirmed: "Mise à jour confirmée",
    update_unconfirmed: "Mise à jour non confirmée",
    config_exported: "Export de configuration",
    config_imported: "Import de configuration",
    agent_started: "Démarrage de l'agent",
    policy_applied: "Politique appliquée",
    schedule_transition: "Transition d'horaire",
    integrity_violation_detected: "Altération détectée",
    integrity_restored: "Hosts restauré",
    manual_stop_detected: "Arrêt manuel détecté",
    uninstalled: "Désinstallation",
  };

  let history = [];
  let agents = [];
  let error = "";
  let notice = "";

  let viewMode = "all"; // "all" | "alerts"
  let sourceFilter = "all"; // "all" | "app" | "agent"
  let agentFilter = filterAgentId || "";
  let kindFilter = "";
  let searchText = "";

  let exportPath = "";
  let exporting = false;

  // Only re-seeds when a *new* navigation hands us a different poste —
  // never fights the dropdown once the admin picks something themselves.
  $: if (filterAgentId) agentFilter = filterAgentId;

  function kindLabel(kind) {
    return KIND_LABELS[kind] ?? kind;
  }

  function agentLabel(agentId) {
    const a = agents.find((x) => x.id === agentId);
    return a ? a.display_name || a.hostname : agentId;
  }

  function viewPoste(agentId) {
    dispatch("navigate", { kind: "agent", id: agentId });
  }

  async function load() {
    error = "";
    try {
      [history, agents] = await Promise.all([api.recentHistory(300), api.listAgents()]);
    } catch (e) {
      error = String(e);
    }
  }

  function dayLabel(at) {
    const d = new Date(at);
    const today = new Date();
    const yesterday = new Date();
    yesterday.setDate(yesterday.getDate() - 1);
    const sameDay = (a, b) => a.toDateString() === b.toDateString();
    if (sameDay(d, today)) return "Aujourd'hui";
    if (sameDay(d, yesterday)) return "Hier";
    return d.toLocaleDateString("fr-FR", { weekday: "long", day: "numeric", month: "long" });
  }

  function csvEscape(field) {
    const s = String(field ?? "");
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  }

  async function exportCsv() {
    if (!exportPath.trim()) return;
    exporting = true;
    error = "";
    notice = "";
    try {
      const header = ["Date", "Source", "Type", "Poste", "Message"].map(csvEscape).join(",");
      const rows = visible.map((h) =>
        [
          new Date(h.at).toLocaleString("fr-FR"),
          h.source === "app" ? "App" : "Agent",
          kindLabel(h.kind),
          h.agent_id ? agentLabel(h.agent_id) : "",
          h.message,
        ]
          .map(csvEscape)
          .join(",")
      );
      await api.writeTextFile(exportPath.trim(), [header, ...rows].join("\n"));
      notice = `${visible.length} ligne(s) exportée(s) vers ${exportPath.trim()}.`;
    } catch (e) {
      error = String(e);
    } finally {
      exporting = false;
    }
  }

  $: availableKinds = [...new Set(history.map((h) => h.kind))].sort((a, b) => kindLabel(a).localeCompare(kindLabel(b)));

  $: visible = history
    .filter((h) => (viewMode === "alerts" ? WARNING_KINDS.has(h.kind) : true))
    .filter((h) => (sourceFilter === "all" ? true : h.source === sourceFilter))
    .filter((h) => (agentFilter ? h.agent_id === agentFilter : true))
    .filter((h) => (kindFilter ? h.kind === kindFilter : true))
    .filter((h) => (searchText.trim() ? h.message.toLowerCase().includes(searchText.trim().toLowerCase()) : true));

  // history already arrives sorted newest-first — fold it into day bands
  // without re-sorting, so "Aujourd'hui"/"Hier" stay contiguous.
  $: dayGroups = (() => {
    const out = [];
    for (const h of visible) {
      const label = dayLabel(h.at);
      let g = out[out.length - 1];
      if (!g || g.label !== label) { g = { label, items: [] }; out.push(g); }
      g.items.push(h);
    }
    return out;
  })();

  onMount(load);
  $: refreshToken, load();
</script>

<div class="toolbar">
  <span class="section-title">Historique</span>
  <div class="spacer"></div>
  <div class="segmented">
    <button class:active={viewMode === "all"} on:click={() => (viewMode = "all")}>Tout</button>
    <button class:active={viewMode === "alerts"} on:click={() => (viewMode = "alerts")}>Alertes</button>
  </div>
</div>

<div class="pad">
  {#if error}<p class="error">{error}</p>{/if}
  {#if notice}<p class="row" style="gap:6px; color:var(--ok); font-size:var(--text-sm);"><Icon name="check" />{notice}</p>{/if}

  <div class="row" style="flex-wrap:wrap; gap:var(--space-2); margin-bottom:var(--space-2);">
    <input placeholder="Rechercher…" bind:value={searchText} style="flex:1; min-width:160px;" />
    <select bind:value={sourceFilter}>
      <option value="all">Toutes les sources</option>
      <option value="app">Actions de l'app</option>
      <option value="agent">Événements des agents</option>
    </select>
    <select bind:value={agentFilter}>
      <option value="">Tous les postes</option>
      {#each agents as a}<option value={a.id}>{a.display_name || a.hostname}</option>{/each}
    </select>
    <select bind:value={kindFilter}>
      <option value="">Tous les types</option>
      {#each availableKinds as k}<option value={k}>{kindLabel(k)}</option>{/each}
    </select>
  </div>

  <div class="row" style="gap:var(--space-2); margin-bottom:var(--space-3);">
    <input placeholder="Chemin du fichier .csv" bind:value={exportPath} style="flex:1;" />
    <button on:click={exportCsv} disabled={exporting || !exportPath.trim() || visible.length === 0}>
      {exporting ? "Export…" : `Exporter (${visible.length})`}
    </button>
  </div>

  {#if visible.length === 0}
    <div class="group"><div class="empty"><Icon name="history" large /><p>Rien à afficher pour le moment.</p></div></div>
  {:else}
    <div class="group">
      {#each dayGroups as g, gi}
        <div class="row-line" style="{gi > 0 ? '' : 'border-top:none;'} padding-top:8px; padding-bottom:4px;">
          <span class="subtitle" style="font-weight:600;">{g.label}</span>
        </div>
        {#each g.items as h}
          <div class="row-line" style={WARNING_KINDS.has(h.kind) ? "background:var(--warn-weak);" : ""}>
            <span class="mono subtitle" style="width:70px;">{new Date(h.at).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" })}</span>
            <span class="pill {h.source === 'app' ? 'pill-accent' : 'pill-neutral'}">{h.source === "app" ? "App" : "Agent"}</span>
            <span style="flex:1;">
              {#if WARNING_KINDS.has(h.kind)}
                <span class="row" style="gap:5px; color:var(--warn); display:inline-flex;"><Icon name="warning" />{h.message}</span>
              {:else}
                {h.message}
              {/if}
            </span>
            {#if WARNING_KINDS.has(h.kind) && h.agent_id}
              <button class="link" on:click={() => viewPoste(h.agent_id)}>Voir le détail</button>
            {/if}
          </div>
        {/each}
      {/each}
    </div>
  {/if}
</div>
