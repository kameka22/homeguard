<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";

  export let refreshToken = 0;

  let groups = [];
  let agents = [];
  let newName = "";
  let error = "";
  let memberOf = {}; // agentId -> Set(groupId), loaded lazily per expanded group

  async function load() {
    error = "";
    try {
      [groups, agents] = await Promise.all([api.listGroups(), api.listAgents()]);
    } catch (e) {
      error = String(e);
    }
  }

  async function create() {
    if (!newName.trim()) return;
    try {
      await api.createGroup(newName.trim());
      newName = "";
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function remove(group) {
    if (!confirm(`Supprimer le groupe « ${group.name} » ? Les affectations qui le ciblent seront aussi supprimées.`)) return;
    try {
      await api.deleteGroup(group.id);
      await load();
    } catch (e) {
      error = String(e);
    }
  }

  async function isMember(groupId, agentId) {
    if (!memberOf[agentId]) {
      memberOf[agentId] = new Set(await api.agentGroupIds(agentId));
      memberOf = memberOf;
    }
    return memberOf[agentId].has(groupId);
  }

  async function toggleMember(group, agent) {
    const member = memberOf[agent.id]?.has(group.id);
    try {
      if (member) await api.removeGroupMember(group.id, agent.id);
      else await api.addGroupMember(group.id, agent.id);
      memberOf[agent.id] = new Set(await api.agentGroupIds(agent.id));
      memberOf = memberOf;
    } catch (e) {
      error = String(e);
    }
  }

  onMount(async () => {
    await load();
    for (const a of agents) await isMember(null, a.id);
  });
  $: refreshToken, load();
</script>

<h1>Groupes</h1>
<p class="subtitle">Regroupez des postes pour leur affecter les mêmes listes en une fois.</p>
{#if error}<p class="error">{error}</p>{/if}

<div class="card" style="margin-bottom:16px;">
  <div class="row">
    <input placeholder="Nom du groupe (ex. Enfants)" bind:value={newName} on:keydown={(e) => e.key === "Enter" && create()} />
    <button class="primary" on:click={create}>Créer</button>
  </div>
</div>

{#if groups.length === 0}
  <div class="empty">
    <Icon name="users" large />
    <p>Aucun groupe pour le moment. Créez-en un ci-dessus pour affecter des listes à plusieurs postes à la fois.</p>
  </div>
{:else}
  {#each groups as group}
    <div class="card" style="margin-bottom:12px;">
      <div class="row" style="margin-bottom:10px;">
        <h2 style="margin:0;">{group.name}</h2>
        <span class="pill pill-neutral">
          {agents.filter((a) => memberOf[a.id]?.has(group.id)).length} membre{agents.filter((a) => memberOf[a.id]?.has(group.id)).length === 1 ? "" : "s"}
        </span>
        <div class="spacer" />
        <button class="danger" on:click={() => remove(group)}>Supprimer</button>
      </div>
      {#if agents.length === 0}
        <p class="subtitle" style="margin:0;">Aucun poste connu — ajoutez-en depuis « + Postes ».</p>
      {:else}
        <div class="row" style="flex-wrap:wrap; gap:6px;">
          {#each agents as agent}
            <button
              class="pill {memberOf[agent.id]?.has(group.id) ? 'pill-accent' : 'pill-neutral'}"
              on:click={() => toggleMember(group, agent)}
            >
              {#if memberOf[agent.id]?.has(group.id)}<Icon name="check" />{/if}
              {agent.display_name || agent.hostname}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/each}
{/if}
