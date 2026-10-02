<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";
  import ScheduleGrid from "./ScheduleGrid.svelte";

  export let assignmentId;
  export let title;
  export let initialSchedule = null;

  const dispatch = createEventDispatcher();

  let schedule = initialSchedule;
  let timezone = "";
  let error = "";
  let saving = false;

  onMount(async () => {
    try {
      timezone = await api.getTimezone();
    } catch {
      // The note just won't show a timezone name — not worth blocking on.
    }
  });

  async function save() {
    saving = true;
    error = "";
    try {
      await api.updateAssignmentSchedule(assignmentId, schedule);
      dispatch("saved");
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
      <span class="modal-icon"><Icon name="clock-forward" /></span>
      <div>
        <div class="modal-title">{title}</div>
        <div class="modal-desc">Choisissez quand la liste est bloquée. En dehors de ces plages, les sites sont accessibles.</div>
      </div>
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    <ScheduleGrid {schedule} on:change={(e) => (schedule = e.detail)} />

    {#if timezone}
      <p class="subtitle" style="margin-top:var(--space-3);">
        Fuseau {timezone}, fixe — changer l'heure du poste ne contourne pas l'horaire.
      </p>
    {/if}

    <div class="modal-actions">
      <button class="primary" on:click={save} disabled={saving}>{saving ? "Enregistrement…" : "Enregistrer"}</button>
      <button on:click={() => dispatch("close")}>Annuler</button>
    </div>
  </div>
</div>
