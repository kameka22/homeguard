<script>
  import { onMount } from "svelte";
  import "./app.css";
  import Icon from "./lib/Icon.svelte";
  import { api } from "./lib/api.js";
  import { listColor } from "./lib/colors.js";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Dashboard from "./lib/views/Dashboard.svelte";
  import AgentDetail from "./lib/views/AgentDetail.svelte";
  import GroupDetail from "./lib/views/GroupDetail.svelte";
  import ListDetail from "./lib/views/ListDetail.svelte";
  import History from "./lib/views/History.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import EnrollDialog from "./lib/EnrollDialog.svelte";
  import CreateGroupDialog from "./lib/CreateGroupDialog.svelte";
  import CreateListDialog from "./lib/CreateListDialog.svelte";
  import Welcome from "./lib/Welcome.svelte";

  // What fills the content area: a standalone screen ("dashboard" /
  // "history"), or one specific entity's detail — the sidebar plays the
  // role of the list pane permanently, so there's no separate "browse all
  // postes" screen to fall back to.
  let route = { kind: "dashboard" };
  // null = not checked yet (render nothing rather than flash the wrong
  // screen); true/false once known.
  let onboardingDone = null;
  let showEnroll = false;
  let showSettings = false;
  let showCreateGroup = false;
  let showCreateList = false;
  // Bumped whenever something changes agents/groups/lists so every screen
  // (and the sidebar) refetches — simplest possible cross-view refresh.
  let refreshToken = 0;
  const refresh = () => (refreshToken += 1);

  let agents = [];
  let groups = [];
  let lists = [];
  let liveStatus = {};

  async function loadSidebar() {
    try {
      [agents, groups, lists] = await Promise.all([api.listAgents(), api.listGroups(), api.listLists()]);
    } catch {
      // The sidebar is a convenience view of data every screen also loads
      // itself — a failure here just means stale sections, not a broken app.
    }
    try {
      const results = await api.refreshAgentsStatus();
      const next = {};
      for (const r of results) next[r.agent_id] = r.online;
      liveStatus = next;
    } catch {
      // Same: the dots just stay at "checking" if this fails.
    }
  }

  function statusDotClass(agent) {
    if (agent.revoked) return "danger";
    if (!agent.last_ip) return "";
    if (liveStatus[agent.id] === true) return "ok";
    if (liveStatus[agent.id] === false) return "danger";
    return "";
  }

  function lastSeenLabel(agent) {
    if (!agent.last_seen_at) return "";
    const ms = Date.now() - new Date(agent.last_seen_at).getTime();
    const days = Math.floor(ms / 86400000);
    if (days >= 1) return `${days} j`;
    return "";
  }

  async function groupCreated(e) {
    showCreateGroup = false;
    await loadSidebar();
    route = { kind: "group", id: e.detail.id };
  }

  async function listCreated(e) {
    showCreateList = false;
    await loadSidebar();
    route = { kind: "list", id: e.detail.id };
  }

  // `data-tauri-drag-region` alone is the documented way to do this, but
  // it's flaky in dev mode behind Vite's HMR (the region Tauri's webview
  // scans for at load can go stale once Svelte replaces the DOM) — calling
  // startDragging() explicitly on mousedown works regardless.
  function startDrag(e) {
    if (e.buttons !== 1) return;
    if (e.detail === 2) {
      getCurrentWindow().toggleMaximize();
    } else {
      getCurrentWindow().startDragging();
    }
  }

  onMount(async () => {
    try {
      const theme = await api.getTheme();
      if (theme !== "system") document.documentElement.dataset.theme = theme;
    } catch {
      // Falls back to following the system appearance, same as if nothing were saved.
    }
  });
  onMount(loadSidebar);
  $: refreshToken, loadSidebar();

  onMount(async () => {
    try {
      onboardingDone = await api.getOnboardingDone();
    } catch {
      onboardingDone = true; // don't block the whole app on a settings read failing
    }
  });
</script>

{#if onboardingDone === false}
  <Welcome on:done={(e) => { onboardingDone = true; if (e.detail?.openSettings) showSettings = true; }} />
{:else if onboardingDone}
<div class="window">
  <div class="chrome-strip">
    <div class="chrome-strip-sidebar" role="presentation" data-tauri-drag-region on:mousedown={startDrag}></div>
    <div class="chrome-strip-content" role="presentation" data-tauri-drag-region on:mousedown={startDrag}></div>
  </div>
  <div class="body">
    <nav class="sidebar">
      <div class="sidebar-section" style="margin-top:var(--space-3);">
        <button class="nav-item" class:active={route.kind === "dashboard"} on:click={() => (route = { kind: "dashboard" })}>
          <Icon name="dashboard" />Aperçu
        </button>
        <button class="nav-item" class:active={route.kind === "history"} on:click={() => (route = { kind: "history" })}>
          <Icon name="history" />Historique
        </button>
      </div>

      <div class="sidebar-section">
        <div class="sidebar-label">Postes</div>
        {#if agents.length === 0}
          <div class="sidebar-empty">Aucun poste</div>
        {:else}
          {#each agents as agent (agent.id)}
            <button
              class="nav-item"
              class:active={route.kind === "agent" && route.id === agent.id}
              on:click={() => (route = { kind: "agent", id: agent.id })}
            >
              <span class="status-dot {statusDotClass(agent)}"></span>
              {agent.display_name || agent.hostname}
              {#if lastSeenLabel(agent)}<span class="nav-sub">{lastSeenLabel(agent)}</span>{/if}
            </button>
          {/each}
        {/if}
      </div>

      <div class="sidebar-section">
        <div class="sidebar-label">Groupes</div>
        {#if groups.length === 0}
          <div class="sidebar-empty">Aucun groupe</div>
        {:else}
          {#each groups as group (group.id)}
            <button
              class="nav-item"
              class:active={route.kind === "group" && route.id === group.id}
              on:click={() => (route = { kind: "group", id: group.id })}
            >
              <Icon name="users" />{group.name}
            </button>
          {/each}
        {/if}
        <button class="nav-item" on:click={() => (showCreateGroup = true)}>
          <Icon name="plus" />Nouveau groupe
        </button>
      </div>

      <div class="sidebar-section">
        <div class="sidebar-label">Listes de blocage</div>
        {#each lists as list (list.id)}
          <button
            class="nav-item"
            class:active={route.kind === "list" && route.id === list.id}
            on:click={() => (route = { kind: "list", id: list.id })}
          >
            <span class="swatch" style="background:{listColor(list.id)};"></span>
            {list.name}
            <span class="nav-badge">{list.domains.length}</span>
          </button>
        {/each}
        <button class="nav-item" on:click={() => (showCreateList = true)}>
          <Icon name="plus" />Nouvelle liste
        </button>
      </div>

      <div class="grow"></div>
      <div class="row" style="border-top:1px solid var(--border); padding-top:var(--space-2); margin-top:var(--space-2);">
        <button class="sidebar-add" style="border-top:none; padding-top:7px; flex:1;" on:click={() => (showEnroll = true)}>
          <Icon name="plus" />Ajouter un poste
        </button>
        <button class="toolbar-icon-btn" aria-label="Réglages" on:click={() => (showSettings = true)}>
          <Icon name="settings" />
        </button>
      </div>
    </nav>

    <main class="content">
      {#if route.kind === "dashboard"}
        <Dashboard
          {refreshToken}
          on:changed={refresh}
          on:navigate={(e) => (route = e.detail)}
          on:openEnroll={() => (showEnroll = true)}
        />
      {:else if route.kind === "history"}
        <History {refreshToken} filterAgentId={route.agentId} on:navigate={(e) => (route = e.detail)} />
      {:else if route.kind === "agent"}
        {#key route.id}
          <AgentDetail
            agentId={route.id}
            {refreshToken}
            on:changed={refresh}
            on:removed={() => (route = { kind: "dashboard" })}
            on:navigate={(e) => (route = e.detail)}
          />
        {/key}
      {:else if route.kind === "group"}
        {#key route.id}
          <GroupDetail groupId={route.id} {refreshToken} on:changed={refresh} on:removed={() => (route = { kind: "dashboard" })} />
        {/key}
      {:else if route.kind === "list"}
        {#key route.id}
          <ListDetail listId={route.id} {refreshToken} on:changed={refresh} on:removed={() => (route = { kind: "dashboard" })} />
        {/key}
      {/if}
    </main>
  </div>
</div>
{/if}

{#if showEnroll}
  <EnrollDialog on:close={() => { showEnroll = false; refresh(); }} />
{/if}
{#if showSettings}
  <SettingsDialog on:close={() => (showSettings = false)} />
{/if}
{#if showCreateGroup}
  <CreateGroupDialog on:close={() => (showCreateGroup = false)} on:created={groupCreated} />
{/if}
{#if showCreateList}
  <CreateListDialog on:close={() => (showCreateList = false)} on:created={listCreated} />
{/if}
