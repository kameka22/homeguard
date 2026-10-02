<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";
  import ScheduleGrid from "./ScheduleGrid.svelte";

  export let listId;
  export let listName = "";

  const dispatch = createEventDispatcher();

  let agents = [];
  let groups = [];
  let targetKind = "agent"; // "agent" | "group"
  let targetId = "";
  let schedule = null;
  let error = "";
  let saving = false;

  onMount(async () => {
    try {
      [agents, groups] = await Promise.all([api.listAgents(), api.listGroups()]);
    } catch (e) {
      error = String(e);
    }
  });

  $: targets = targetKind === "agent" ? agents : groups;
  $: targetKind, (targetId = ""); // reset the pick whenever the kind toggle changes

  async function submit() {
    if (!targetId) return;
    saving = true;
    error = "";
    try {
      await api.createAssignment(listId, targetKind, targetId, schedule);
      dispatch("done");
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="modal-backdrop" role="presentation" on:click|self={() => dispatch("close")}>
  <div class="modal wide">
    <div class="modal-header">
      <span class="modal-icon"><Icon name="list" /></span>
      <div>
        <div class="modal-title">Affecter « {listName} »</div>
        <div class="modal-desc">À un poste ou un groupe, avec ou sans horaire</div>
      </div>
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    <div class="segmented" style="margin-bottom:10px;">
      <button class:active={targetKind === "agent"} on:click={() => (targetKind = "agent")}>Un poste</button>
      <button class:active={targetKind === "group"} on:click={() => (targetKind = "group")}>Un groupe</button>
    </div>

    <label style="display:block; margin-bottom:var(--space-3);">
      <select bind:value={targetId} style="width:100%;">
        <option value="">{targetKind === "agent" ? "Choisir un poste…" : "Choisir un groupe…"}</option>
        {#each targets as t}
          <option value={t.id}>{targetKind === "agent" ? t.display_name || t.hostname : t.name}</option>
        {/each}
      </select>
    </label>

    <ScheduleGrid {schedule} on:change={(e) => (schedule = e.detail)} />

    <div class="modal-actions">
      <button class="primary" on:click={submit} disabled={saving || !targetId}>{saving ? "Affectation…" : "Affecter"}</button>
      <button on:click={() => dispatch("close")}>Annuler</button>
    </div>
  </div>
</div>
