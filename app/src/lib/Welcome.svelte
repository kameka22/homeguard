<script>
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import { api } from "./api.js";

  const dispatch = createEventDispatcher();
  let error = "";
  let finishing = false;

  async function createAndContinue() {
    finishing = true;
    error = "";
    try {
      await api.setOnboardingDone();
      dispatch("done");
    } catch (e) {
      error = String(e);
    } finally {
      finishing = false;
    }
  }

  // The real import form (path + passphrase) already lives in Réglages —
  // no need for a second one here. This just gets out of the way and
  // points there.
  async function importInstead() {
    try { await api.setOnboardingDone(); } catch { /* best-effort */ }
    dispatch("done", { openSettings: true });
  }
</script>

<div class="welcome">
  <div class="welcome-icon"><Icon name="shield" large /></div>
  <h1 style="text-align:center;">Bienvenue dans HomeGuard</h1>
  <p class="subtitle" style="text-align:center;">Bloquez des sites sur les ordinateurs de la maison, depuis ce Mac.</p>

  <div class="welcome-features">
    <div class="welcome-feature">
      <span class="modal-icon"><Icon name="key" /></span>
      <div>
        <div style="font-weight:600; font-size:var(--text-sm);">Une autorité de certification privée</div>
        <div class="subtitle">Créée maintenant et rangée dans votre Trousseau macOS. Seul ce Mac pourra piloter vos postes.</div>
      </div>
    </div>
    <div class="welcome-feature">
      <span class="modal-icon"><Icon name="wifi" /></span>
      <div>
        <div style="font-weight:600; font-size:var(--text-sm);">Détection automatique</div>
        <div class="subtitle">Une commande à coller sur chaque PC Windows ou Linux, et il apparaît ici tout seul.</div>
      </div>
    </div>
    <div class="welcome-feature">
      <span class="modal-icon"><Icon name="clock-forward" /></span>
      <div>
        <div style="font-weight:600; font-size:var(--text-sm);">Horaires et déblocages</div>
        <div class="subtitle">Les postes appliquent les règles eux-mêmes, même quand ce Mac est éteint.</div>
      </div>
    </div>
  </div>

  {#if error}<p class="error">{error}</p>{/if}

  <div class="row" style="margin-top:var(--space-5);">
    <button on:click={importInstead}>Importer une sauvegarde…</button>
    <div class="spacer"></div>
    <button class="primary" on:click={createAndContinue} disabled={finishing}>Créer et continuer</button>
  </div>
</div>
