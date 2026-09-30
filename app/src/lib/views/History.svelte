<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";

  export let refreshToken = 0;

  const WARNING_KINDS = new Set(["integrity_violation_detected", "manual_stop_detected"]);

  let history = [];
  let error = "";
  let sourceFilter = "all"; // "all" | "app" | "agent"

  async function load() {
    error = "";
    try {
      history = await api.recentHistory(300);
    } catch (e) {
      error = String(e);
    }
  }

  function sourcePill(source) {
    return source === "app" ? { cls: "pill-accent", label: "App" } : { cls: "pill-neutral", label: "Agent" };
  }

  $: visible = sourceFilter === "all" ? history : history.filter((h) => h.source === sourceFilter);

  onMount(load);
  $: refreshToken, load();
</script>

<h1>Historique</h1>
<p class="subtitle">Actions de l'application et événements remontés par les agents.</p>
{#if error}<p class="error">{error}</p>{/if}

<div class="row" style="margin-bottom:12px; gap:6px;">
  {#each [["all", "Tout"], ["app", "App"], ["agent", "Agent"]] as [key, label]}
    <button class="pill {sourceFilter === key ? 'pill-accent' : 'pill-neutral'}" on:click={() => (sourceFilter = key)}>{label}</button>
  {/each}
</div>

<div class="card">
  {#if visible.length === 0}
    <div class="empty">
      <Icon name="history" large />
      <p>Rien à afficher pour le moment.</p>
    </div>
  {:else}
    <table>
      <thead>
        <tr><th style="width:150px;">Quand</th><th style="width:70px;">Source</th><th>Message</th></tr>
      </thead>
      <tbody>
        {#each visible as h}
          <tr>
            <td class="mono" style="color: var(--text-dim);">{new Date(h.at).toLocaleString("fr-FR")}</td>
            <td><span class="pill {sourcePill(h.source).cls}">{sourcePill(h.source).label}</span></td>
            <td>
              {#if WARNING_KINDS.has(h.kind)}
                <span class="row" style="gap:5px; color:var(--warn);"><Icon name="warning" />{h.message}</span>
              {:else}
                {h.message}
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
