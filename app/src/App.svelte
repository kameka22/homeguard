<script>
  import "./app.css";
  import Icon from "./lib/Icon.svelte";
  import Dashboard from "./lib/views/Dashboard.svelte";
  import Agents from "./lib/views/Agents.svelte";
  import Groups from "./lib/views/Groups.svelte";
  import Lists from "./lib/views/Lists.svelte";
  import History from "./lib/views/History.svelte";
  import Settings from "./lib/views/Settings.svelte";
  import EnrollDialog from "./lib/EnrollDialog.svelte";

  const views = [
    { id: "dashboard", label: "Tableau de bord", icon: "dashboard", component: Dashboard },
    { id: "agents", label: "Postes", icon: "monitor", component: Agents },
    { id: "groups", label: "Groupes", icon: "users", component: Groups },
    { id: "lists", label: "Listes", icon: "list", component: Lists },
    { id: "history", label: "Historique", icon: "history", component: History },
    { id: "settings", label: "Réglages", icon: "settings", component: Settings },
  ];

  let activeView = "dashboard";
  let showEnroll = false;
  // Bumped whenever something changes agents/groups/lists so every view
  // that reads them refetches — simplest possible cross-view refresh.
  let refreshToken = 0;
  const refresh = () => (refreshToken += 1);

  $: current = views.find((v) => v.id === activeView);
</script>

<div class="layout">
  <nav class="sidebar">
    <div class="brand">
      <span class="brand-mark"><Icon name="shield" /></span>
      HomeGuard
    </div>
    {#each views as view}
      <button class="nav-item" class:active={activeView === view.id} on:click={() => (activeView = view.id)}>
        <Icon name={view.icon} />
        {view.label}
      </button>
    {/each}
    <div class="spacer" />
    <button class="primary" on:click={() => (showEnroll = true)}>
      <span class="row" style="gap:5px; justify-content:center;"><Icon name="plus" />Postes</span>
    </button>
  </nav>

  <main class="content">
    <svelte:component this={current.component} {refreshToken} on:changed={refresh} />
  </main>
</div>

{#if showEnroll}
  <EnrollDialog on:close={() => { showEnroll = false; refresh(); }} />
{/if}
