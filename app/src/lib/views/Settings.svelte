<script>
  import { onMount } from "svelte";
  import { api } from "../api.js";
  import Icon from "../Icon.svelte";

  let timezone = "";
  let caFingerprint = "";
  let error = "";
  let notice = "";

  let exportPath = "";
  let exportPassphrase = "";
  let exporting = false;

  let importPath = "";
  let importPassphrase = "";
  let importing = false;

  async function load() {
    try {
      [timezone, caFingerprint] = await Promise.all([api.getTimezone(), api.caFingerprint()]);
    } catch (e) {
      error = String(e);
    }
  }

  async function saveTimezone() {
    error = "";
    try {
      await api.setTimezone(timezone);
      notice = "Fuseau horaire enregistré.";
    } catch (e) {
      error = String(e);
    }
  }

  async function doExport() {
    if (!exportPath.trim() || exportPassphrase.length < 8) return;
    exporting = true;
    error = "";
    notice = "";
    try {
      await api.exportConfig(exportPath.trim(), exportPassphrase);
      notice = `Configuration exportée vers ${exportPath.trim()}.`;
      exportPassphrase = "";
    } catch (e) {
      error = String(e);
    } finally {
      exporting = false;
    }
  }

  async function doImport() {
    if (!importPath.trim() || !importPassphrase) return;
    if (!confirm("Importer va fusionner cette configuration dans celle en cours (les postes, groupes, listes et affectations existants avec les mêmes identifiants seront écrasés). Continuer ?")) {
      return;
    }
    importing = true;
    error = "";
    notice = "";
    try {
      const restartNeeded = await api.importConfig(importPath.trim(), importPassphrase);
      notice = restartNeeded
        ? "Configuration importée. Le certificat racine a changé : redémarrez l'application pour qu'il soit actif."
        : "Configuration importée.";
      importPassphrase = "";
    } catch (e) {
      error = String(e);
    } finally {
      importing = false;
    }
  }

  onMount(load);
</script>

<h1>Réglages</h1>
{#if error}<p class="error">{error}</p>{/if}
{#if notice}
  <p class="row" style="gap:6px; color: var(--ok); margin: 0 0 var(--space-5);"><Icon name="check" />{notice}</p>
{/if}

<div class="card" style="margin-bottom:16px;">
  <h2>Général</h2>
  <label style="display:block; margin-bottom:8px;">
    Fuseau horaire des horaires
    <div class="row">
      <input bind:value={timezone} placeholder="Europe/Paris" style="flex:1;" />
      <button on:click={saveTimezone}>Enregistrer</button>
    </div>
  </label>
  <p class="subtitle" style="margin:8px 0 0;">Empreinte de la CA : <code>{caFingerprint}</code></p>
</div>

<div class="card" style="margin-bottom:16px;">
  <h2>Exporter la configuration</h2>
  <p class="subtitle">
    Sauvegarde chiffrée de tous les postes, groupes, listes, affectations et déblocages — ainsi que le certificat
    racine, ce qui permet de migrer vers un autre Mac sans réenrôler les postes. Gardez la phrase de passe en lieu sûr :
    elle seule protège ce fichier.
  </p>
  <label style="display:block; margin-bottom:8px;">
    Chemin du fichier
    <input bind:value={exportPath} placeholder="/Users/vous/Desktop/homeguard-backup.age" style="width:100%;" />
  </label>
  <label style="display:block; margin-bottom:8px;">
    Phrase de passe (8 caractères minimum)
    <input type="password" bind:value={exportPassphrase} style="width:100%;" />
  </label>
  <button class="primary" on:click={doExport} disabled={exporting || !exportPath.trim() || exportPassphrase.length < 8}>
    {exporting ? "Export…" : "Exporter"}
  </button>
</div>

<div class="card">
  <h2>Importer une configuration</h2>
  <p class="subtitle">Fusionne un fichier exporté dans la configuration actuelle.</p>
  <label style="display:block; margin-bottom:8px;">
    Chemin du fichier
    <input bind:value={importPath} placeholder="/Users/vous/Desktop/homeguard-backup.age" style="width:100%;" />
  </label>
  <label style="display:block; margin-bottom:8px;">
    Phrase de passe
    <input type="password" bind:value={importPassphrase} style="width:100%;" />
  </label>
  <button class="danger" on:click={doImport} disabled={importing || !importPath.trim() || !importPassphrase}>
    {importing ? "Import…" : "Importer"}
  </button>
</div>
