<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { api } from "./api.js";

  const dispatch = createEventDispatcher();

  let groups = [];
  let defaultGroupId = "";
  let ttlSecs = 30 * 60;
  let info = null;
  let error = "";
  let starting = false;

  onMount(async () => {
    try {
      groups = await api.listGroups();
    } catch (e) {
      error = String(e);
    }
  });

  async function start() {
    starting = true;
    error = "";
    try {
      info = await api.startEnrollment(ttlSecs, defaultGroupId || null);
    } catch (e) {
      error = String(e);
    } finally {
      starting = false;
    }
  }

  $: enrollUrl = info ? `https://${info.app_ip}:${info.enrollment_port}/v1/enroll` : "";
  $: enrollCommand = info
    ? `homeguard-agent enroll --url ${enrollUrl} --token ${info.token} --ca-sha256 ${info.ca_fingerprint}`
    : "";
  $: installShCommand = info ? `curl -fsSL http://${info.app_ip}:${info.deploy_port}/install.sh?t=${info.token} | sudo sh` : "";
  $: installPs1Command = info ? `irm http://${info.app_ip}:${info.deploy_port}/install.ps1?t=${info.token} | iex` : "";

  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard API can be unavailable depending on webview
      // permissions; the text is still selectable/visible either way.
    }
  }

  async function finish() {
    try {
      await api.stopEnrollment();
    } catch {
      // Best-effort — closing the dialog matters more than a clean
      // server shutdown, which the app will also do on quit.
    }
    dispatch("close");
  }
</script>

<div class="modal-backdrop" role="presentation" on:click|self={finish}>
  <div class="modal">
    <h2>Ajouter des postes</h2>

    {#if error}<p class="error">{error}</p>{/if}

    {#if !info}
      <p class="subtitle">
        Génère un jeton temporaire et démarre un petit serveur local pour enrôler des agents.
      </p>
      <label style="display:block; margin-bottom:10px;">
        Groupe par défaut
        <select bind:value={defaultGroupId} style="width:100%;">
          <option value="">(aucun)</option>
          {#each groups as g}
            <option value={g.id}>{g.name}</option>
          {/each}
        </select>
      </label>
      <label style="display:block; margin-bottom:16px;">
        Durée de validité
        <select bind:value={ttlSecs} style="width:100%;">
          <option value={900}>15 minutes</option>
          <option value={1800}>30 minutes</option>
          <option value={3600}>1 heure</option>
        </select>
      </label>
      <div class="row">
        <div class="spacer" />
        <button on:click={finish}>Annuler</button>
        <button class="primary" on:click={start} disabled={starting}>
          {starting ? "Démarrage…" : "Démarrer l'enrôlement"}
        </button>
      </div>
    {:else}
      {#if info.binaries_available}
        <p class="subtitle">Sur le poste à enrôler :</p>
        <p style="margin:0 0 4px; font-weight:600;">Linux</p>
        <code class="cmd">{installShCommand}</code>
        <button on:click={() => copy(installShCommand)}>Copier</button>

        <p style="margin:12px 0 4px; font-weight:600;">Windows (PowerShell administrateur)</p>
        <code class="cmd">{installPs1Command}</code>
        <button on:click={() => copy(installPs1Command)}>Copier</button>
      {:else}
        <p class="subtitle">
          L'installateur en une commande n'est pas encore disponible dans cette version (aucun binaire agent
          n'est embarqué). Copiez le binaire <code>homeguard-agent</code> sur le poste manuellement, puis
          exécutez :
        </p>
        <code class="cmd">{enrollCommand}</code>
        <button on:click={() => copy(enrollCommand)}>Copier</button>
      {/if}

      <p class="subtitle" style="margin:10px 0 16px;">Jeton valable {Math.round(ttlSecs / 60)} min.</p>

      <div class="row">
        <div class="spacer" />
        <button on:click={start}>Nouveau jeton</button>
        <button class="primary" on:click={finish}>Terminer</button>
      </div>
    {/if}
  </div>
</div>
