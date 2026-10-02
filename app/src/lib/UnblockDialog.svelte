<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";

  export let targetKind; // "agent" | "group"
  export let targetId;
  export let targetLabel = "";

  const dispatch = createEventDispatcher();

  const DURATIONS = [
    { label: "15 min", minutes: 15 }, { label: "30 min", minutes: 30 },
    { label: "1 h", minutes: 60 }, { label: "2 h", minutes: 120 }, { label: "4 h", minutes: 240 },
  ];

  let domainsText = "";
  let minutes = 60;
  let error = "";
  let saving = false;

  onMount(async () => {
    try {
      minutes = await api.getDefaultUnblockMinutes();
    } catch {
      // Keeps the 60-minute fallback already set above.
    }
  });

  async function submit() {
    const domains = domainsText.split(/[\n,]+/).map((d) => d.trim().toLowerCase()).filter(Boolean);
    if (domains.length === 0) return;
    saving = true;
    error = "";
    try {
      const until = new Date(Date.now() + minutes * 60_000).toISOString();
      await api.createException(targetKind, targetId, domains, until);
      dispatch("done");
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="modal-backdrop" role="presentation" on:click|self={() => dispatch("close")}>
  <div class="modal">
    <div class="modal-header">
      <span class="modal-icon"><Icon name="lock-open" /></span>
      <div>
        <div class="modal-title">Débloquer temporairement</div>
        <div class="modal-desc">
          {targetKind === "group" ? `Pour tout le groupe « ${targetLabel} »` : `Pour « ${targetLabel} »`}
        </div>
      </div>
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    <label style="display:block; margin-bottom:10px;">
      <span class="subtitle">Domaines à débloquer</span>
      <textarea
        bind:value={domainsText}
        placeholder="twitch.tv, youtube.com…"
        rows="3"
        style="width:100%; margin-top:4px; font-family:var(--font-mono);"
      />
    </label>
    <label style="display:block; margin-bottom:4px;">
      <span class="subtitle">Durée</span>
      <select bind:value={minutes} style="width:100%; margin-top:4px;">
        {#each DURATIONS as d}<option value={d.minutes}>{d.label}</option>{/each}
      </select>
    </label>

    <div class="modal-actions">
      <button class="primary" on:click={submit} disabled={saving || domainsText.trim() === ""}>
        {saving ? "Déblocage…" : "Débloquer"}
      </button>
      <button on:click={() => dispatch("close")}>Annuler</button>
    </div>
  </div>
</div>
