<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";

  const dispatch = createEventDispatcher();

  let tab = "general";
  let theme = "system";
  let timezone = "";
  let caFingerprint = "";
  let caAlgorithm = "";
  let caNotAfter = "";
  let bundledVersion = null;
  let outdatedCount = 0;
  let revokedAgents = [];
  let scanOnLaunch = true;
  let autoUpdateAgents = false;
  let defaultUnblockMinutes = 60;
  let autoWww = true;
  let aliases = [];
  let newAliasDomain = "";
  let newAliasVariants = "";
  let error = "";
  let notice = "";

  let exportPath = "";
  let exportPassphrase = "";
  let exporting = false;

  let importPath = "";
  let importPassphrase = "";
  let importing = false;

  let updatingAll = false;

  async function load() {
    try {
      const [th, tz, ca, bv, agents, sol, aua, dum, aw, al] = await Promise.all([
        api.getTheme(), api.getTimezone(), api.caDetails(), api.bundledAgentVersion(), api.listAgents(),
        api.getScanOnLaunch(), api.getAutoUpdateAgents(), api.getDefaultUnblockMinutes(), api.getAutoWww(),
        api.listAliases(),
      ]);
      theme = th;
      timezone = tz;
      caFingerprint = ca.fingerprint;
      caAlgorithm = ca.algorithm;
      caNotAfter = ca.not_after;
      bundledVersion = bv;
      outdatedCount = bv ? agents.filter((a) => a.agent_version && a.agent_version !== "unknown" && a.agent_version !== bv).length : 0;
      revokedAgents = agents.filter((a) => a.revoked);
      scanOnLaunch = sol;
      autoUpdateAgents = aua;
      defaultUnblockMinutes = dum;
      autoWww = aw;
      aliases = al;
    } catch (e) {
      error = String(e);
    }
  }

  async function setTheme(value) {
    theme = value;
    if (value === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = value;
    try { await api.setTheme(value); } catch (e) { error = String(e); }
  }

  async function toggleScanOnLaunch() {
    try { await api.setScanOnLaunch(scanOnLaunch); } catch (e) { error = String(e); }
  }

  async function toggleAutoUpdateAgents() {
    try { await api.setAutoUpdateAgents(autoUpdateAgents); } catch (e) { error = String(e); }
  }

  async function toggleAutoWww() {
    try { await api.setAutoWww(autoWww); } catch (e) { error = String(e); }
  }

  async function saveDefaultUnblockMinutes() {
    error = ""; notice = "";
    try {
      await api.setDefaultUnblockMinutes(Number(defaultUnblockMinutes));
      notice = "Durée par défaut enregistrée.";
    } catch (e) { error = String(e); }
  }

  async function saveTimezone() {
    error = ""; notice = "";
    try { await api.setTimezone(timezone); notice = "Fuseau horaire enregistré."; } catch (e) { error = String(e); }
  }

  async function copyFingerprint() {
    try { await navigator.clipboard.writeText(caFingerprint); } catch { /* clipboard may be unavailable */ }
  }

  async function updateAllNow() {
    updatingAll = true; error = ""; notice = "";
    try {
      const results = await api.updateAllOutdatedAgents();
      notice = `${results.filter((r) => r.ok).length}/${results.length} poste(s) mis à jour.`;
      await load();
    } catch (e) { error = String(e); } finally { updatingAll = false; }
  }

  function parseVariants(raw) {
    return raw.split(/[\n,]+/).map((d) => d.trim().toLowerCase()).filter(Boolean);
  }

  async function addAlias() {
    const base = newAliasDomain.trim().toLowerCase();
    if (!base) return;
    try {
      await api.upsertAlias(base, parseVariants(newAliasVariants));
      newAliasDomain = "";
      newAliasVariants = "";
      aliases = await api.listAliases();
    } catch (e) { error = String(e); }
  }

  async function removeAlias(baseDomain) {
    try { await api.deleteAlias(baseDomain); aliases = await api.listAliases(); } catch (e) { error = String(e); }
  }

  async function doExport() {
    if (!exportPath.trim() || exportPassphrase.length < 8) return;
    exporting = true; error = ""; notice = "";
    try {
      await api.exportConfig(exportPath.trim(), exportPassphrase);
      notice = `Configuration exportée vers ${exportPath.trim()}.`;
      exportPassphrase = "";
    } catch (e) { error = String(e); } finally { exporting = false; }
  }

  async function doImport() {
    if (!importPath.trim() || !importPassphrase) return;
    importing = true; error = ""; notice = "";
    try {
      const restartNeeded = await api.importConfig(importPath.trim(), importPassphrase);
      notice = restartNeeded
        ? "Configuration importée. Le certificat racine a changé : redémarrez l'application."
        : "Configuration importée.";
      importPassphrase = "";
    } catch (e) { error = String(e); } finally { importing = false; }
  }

  onMount(load);
</script>

<div class="modal-backdrop" role="presentation" on:click|self={() => dispatch("close")}>
  <div class="modal wide">
    <div class="row" style="margin-bottom:var(--space-4);">
      <div class="segmented" style="margin:0 auto;">
        <button class:active={tab === "general"} on:click={() => (tab = "general")}>
          <span class="row" style="gap:5px;"><Icon name="settings" />Général</span>
        </button>
        <button class:active={tab === "aliases"} on:click={() => (tab = "aliases")}>
          <span class="row" style="gap:5px;"><Icon name="list" />Alias</span>
        </button>
        <button class:active={tab === "security"} on:click={() => (tab = "security")}>
          <span class="row" style="gap:5px;"><Icon name="shield" />Sécurité</span>
        </button>
      </div>
      <button class="toolbar-icon-btn" style="position:absolute; right:var(--space-5);" on:click={() => dispatch("close")} aria-label="Fermer">
        <Icon name="x" />
      </button>
    </div>

    {#if error}<p class="error">{error}</p>{/if}
    {#if notice}<p class="row" style="gap:6px; color:var(--ok); font-size:var(--text-sm);"><Icon name="check" />{notice}</p>{/if}

    {#if tab === "general"}
      <div class="group">
        <div class="group-title">Apparence</div>
        <div class="row-line">
          <span style="flex:1; font-size:var(--text-sm);">Thème</span>
          <div class="segmented">
            <button class:active={theme === "system"} on:click={() => setTheme("system")}>Système</button>
            <button class:active={theme === "light"} on:click={() => setTheme("light")}>Clair</button>
            <button class:active={theme === "dark"} on:click={() => setTheme("dark")}>Sombre</button>
          </div>
        </div>
      </div>

      <div class="group">
        <div class="group-title">Synchronisation</div>
        <div class="row-line">
          <span style="flex:1; font-size:var(--text-sm);">Chercher les postes au lancement</span>
          <label class="toggle">
            <input type="checkbox" bind:checked={scanOnLaunch} on:change={toggleScanOnLaunch} />
            <span class="track"><span class="thumb"></span></span>
          </label>
        </div>
        <div class="row-line">
          <div style="flex:1;">
            <div style="font-size:var(--text-sm);">Inclure automatiquement les variantes</div>
            <div class="subtitle">Ajoute www.&lt;domaine&gt; en plus du catalogue d'alias</div>
          </div>
          <label class="toggle">
            <input type="checkbox" bind:checked={autoWww} on:change={toggleAutoWww} />
            <span class="track"><span class="thumb"></span></span>
          </label>
        </div>
      </div>

      <div class="group">
        <div class="group-title">Déblocages</div>
        <div class="row-line">
          <span style="flex:1; font-size:var(--text-sm);">Durée par défaut d'un déblocage temporaire</span>
          <input
            type="number"
            min="1"
            bind:value={defaultUnblockMinutes}
            style="width:70px;"
          />
          <span class="subtitle">min</span>
          <button on:click={saveDefaultUnblockMinutes}>Enregistrer</button>
        </div>
      </div>

      <div class="group">
        <div class="group-title">Horaires</div>
        <div class="row-line">
          <div style="flex:1;">
            <div style="font-size:var(--text-sm);">Fuseau horaire des règles</div>
            <div class="subtitle">Appliqué par chaque poste, indépendamment de son heure système</div>
          </div>
          <input bind:value={timezone} placeholder="Europe/Paris" style="width:160px;" />
          <button on:click={saveTimezone}>Enregistrer</button>
        </div>
      </div>

      <div class="group">
        <div class="group-title">Agents</div>
        <div class="row-line">
          <span style="flex:1; font-size:var(--text-sm);">Version embarquée dans l'app</span>
          <span class="subtitle">{bundledVersion ?? "aucune"}</span>
        </div>
        <div class="row-line">
          <div style="flex:1;">
            <div style="font-size:var(--text-sm);">Mettre à jour automatiquement</div>
            <div class="subtitle">Dès qu'un poste obsolète est trouvé pendant une recherche</div>
          </div>
          <label class="toggle">
            <input type="checkbox" bind:checked={autoUpdateAgents} on:change={toggleAutoUpdateAgents} />
            <span class="track"><span class="thumb"></span></span>
          </label>
        </div>
        <div class="row-line">
          <span style="flex:1; font-size:var(--text-sm);">Postes à mettre à jour</span>
          <span class="subtitle">{outdatedCount}</span>
          <button on:click={updateAllNow} disabled={updatingAll || outdatedCount === 0}>
            {updatingAll ? "Mise à jour…" : "Tout mettre à jour"}
          </button>
        </div>
      </div>
    {:else if tab === "aliases"}
      <p class="subtitle" style="margin:0 0 var(--space-3);">
        Quand une liste bloque un domaine ci-dessous, ses variantes sont bloquées avec lui.
      </p>
      <div class="group" style="max-height:320px; overflow-y:auto;">
        {#if aliases.length === 0}
          <div class="row-line"><span class="subtitle">Aucun alias.</span></div>
        {:else}
          {#each aliases as a (a.base_domain)}
            <div class="row-line">
              <span class="mono" style="width:160px; flex-shrink:0;">{a.base_domain}</span>
              <span class="subtitle mono" style="flex:1;">{a.variants.join(", ") || "—"}</span>
              <button class="toolbar-icon-btn" on:click={() => removeAlias(a.base_domain)} aria-label="Supprimer"><Icon name="x" /></button>
            </div>
          {/each}
        {/if}
      </div>
      <div class="group" style="margin-top:var(--space-3);">
        <div class="group-title">Ajouter un alias</div>
        <div class="row-line">
          <input placeholder="exemple.com" bind:value={newAliasDomain} style="width:160px;" />
          <input placeholder="www.exemple.com, m.exemple.com…" bind:value={newAliasVariants} style="flex:1;" />
          <button class="tbtn primary" on:click={addAlias} disabled={!newAliasDomain.trim()}>Ajouter</button>
        </div>
      </div>
    {:else}
      <div class="group">
        <div class="group-title">Autorité de certification</div>
        <div class="row-line">
          <span class="modal-icon" style="width:28px; height:28px;"><Icon name="key" /></span>
          <div style="flex:1;">
            <div style="font-size:var(--text-sm); font-weight:600;">Rangée dans le Trousseau macOS</div>
            <div class="subtitle">Jamais écrite sur le disque</div>
          </div>
        </div>
        <div class="row-line">
          <span style="flex:1; font-size:var(--text-sm);">Empreinte SHA-256</span>
          <span class="mono subtitle">{caFingerprint.slice(0, 20)}…</span>
          <button on:click={copyFingerprint}>Copier</button>
        </div>
        <div class="row-line">
          <span style="flex:1; font-size:var(--text-sm);">Algorithme</span>
          <span class="subtitle">{caAlgorithm}</span>
        </div>
        <div class="row-line">
          <span style="flex:1; font-size:var(--text-sm);">Valide jusqu'au</span>
          <span class="subtitle">{caNotAfter ? new Date(caNotAfter).toLocaleDateString("fr-FR") : "…"}</span>
        </div>
      </div>

      {#if revokedAgents.length > 0}
        <div class="group">
          <div class="group-title">Postes retirés</div>
          {#each revokedAgents as agent (agent.id)}
            <div class="row-line">
              <span class="status-dot danger"></span>
              <span style="flex:1; font-size:var(--text-sm);">{agent.display_name || agent.hostname}</span>
              <span class="subtitle">Certificat révoqué</span>
            </div>
          {/each}
        </div>
      {/if}

      <div class="group">
        <div class="group-title">Sauvegarde et migration</div>
        <div class="row-line" style="align-items:flex-start; gap:var(--space-5);">
          <div style="flex:1;">
            <div style="font-weight:600; font-size:var(--text-sm); margin-bottom:4px;">Exporter</div>
            <p class="subtitle" style="margin:0 0 8px;">Sauvegarde chiffrée, avec le certificat racine.</p>
            <input placeholder="Chemin du fichier" bind:value={exportPath} style="width:100%; margin-bottom:6px;" />
            <input type="password" placeholder="Phrase de passe (8+ caractères)" bind:value={exportPassphrase} style="width:100%; margin-bottom:8px;" />
            <button class="primary" on:click={doExport} disabled={exporting || !exportPath.trim() || exportPassphrase.length < 8}>
              {exporting ? "Export…" : "Exporter"}
            </button>
          </div>
          <div style="width:1px; align-self:stretch; background:var(--border);"></div>
          <div style="flex:1;">
            <div style="font-weight:600; font-size:var(--text-sm); margin-bottom:4px;">Importer</div>
            <p class="subtitle" style="margin:0 0 8px;">Fusionne un fichier exporté dans la configuration actuelle.</p>
            <input placeholder="Chemin du fichier" bind:value={importPath} style="width:100%; margin-bottom:6px;" />
            <input type="password" placeholder="Phrase de passe" bind:value={importPassphrase} style="width:100%; margin-bottom:8px;" />
            <button class="danger" on:click={doImport} disabled={importing || !importPath.trim() || !importPassphrase}>
              {importing ? "Import…" : "Importer"}
            </button>
          </div>
        </div>
      </div>
    {/if}
  </div>
</div>
