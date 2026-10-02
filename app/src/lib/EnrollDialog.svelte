<script>
  import { onMount, onDestroy, createEventDispatcher } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";

  const dispatch = createEventDispatcher();

  // No separate "configure, then start" step — the mockup opens straight
  // into a running session with sane defaults; the group dropdown (kept,
  // next to the OS tabs) only matters for the *next* token, via "Nouveau
  // jeton".
  const TTL_SECS = 30 * 60;

  let groups = [];
  let defaultGroupId = "";
  let info = null;
  let error = "";
  let starting = false;
  let osTab = "windows"; // "windows" | "linux" | "offline"
  let remainingSecs = 0;
  let knownIds = new Set();
  let detected = [];
  let countdownTimer = null;
  let pollTimer = null;

  let addingByIp = false;
  let ipInput = "";
  let ipError = "";
  let ipProbing = false;

  onMount(async () => {
    try {
      const [g, existing] = await Promise.all([api.listGroups(), api.listAgents()]);
      groups = g;
      knownIds = new Set(existing.map((a) => a.id));
    } catch (e) {
      error = String(e);
    }
    start();
  });

  onDestroy(() => {
    clearInterval(countdownTimer);
    clearInterval(pollTimer);
  });

  async function start() {
    starting = true;
    error = "";
    try {
      info = await api.startEnrollment(TTL_SECS, defaultGroupId || null);
      osTab = info.binaries_available ? "windows" : "offline";
      remainingSecs = info.expires_in_secs;
      clearInterval(countdownTimer);
      countdownTimer = setInterval(() => { remainingSecs = Math.max(0, remainingSecs - 1); }, 1000);
      clearInterval(pollTimer);
      pollTimer = setInterval(poll, 3000);
      poll();
    } catch (e) {
      error = String(e);
    } finally {
      starting = false;
    }
  }

  // Only surfaces agents the app didn't already know about before this
  // dialog opened — re-discovering an already-enrolled poste mid-session
  // isn't "detected", it's just mDNS doing its usual thing.
  async function poll() {
    try {
      const found = await api.discoverAgents(2000);
      for (const a of found) {
        if (knownIds.has(a.agent_id)) continue;
        if (!detected.some((d) => d.agent_id === a.agent_id)) {
          detected = [...detected, { agent_id: a.agent_id, hostname: a.hostname, os: a.os, name: a.hostname, saved: false }];
        }
      }
    } catch {
      // Best-effort — a failed poll just means the list doesn't grow this tick.
    }
  }

  // Fallback for when a poste *did* enroll but mDNS doesn't surface it in
  // this session (blocked multicast, a different VLAN…) — probes every
  // already-known poste's identity against this one IP directly.
  async function addByIp() {
    if (!ipInput.trim()) return;
    ipProbing = true;
    ipError = "";
    try {
      const a = await api.probeAgentByIp(ipInput.trim());
      if (!detected.some((d) => d.agent_id === a.agent_id)) {
        detected = [...detected, { agent_id: a.agent_id, hostname: a.hostname, os: a.os, name: a.hostname, saved: false }];
      }
      ipInput = "";
      addingByIp = false;
    } catch (e) {
      ipError = String(e);
    } finally {
      ipProbing = false;
    }
  }

  async function saveName(d) {
    try {
      await api.setAgentDisplayName(d.agent_id, d.name);
      d.saved = true;
      detected = detected;
    } catch (e) {
      error = String(e);
    }
  }

  $: installShCommand = info ? `curl -fsSL http://${info.app_ip}:${info.deploy_port}/install.sh?t=${info.token} | sudo sh` : "";
  $: installPs1Command = info ? `irm http://${info.app_ip}:${info.deploy_port}/install.ps1?t=${info.token} | iex` : "";
  $: enrollCommand = info
    ? `homeguard-agent enroll --url https://${info.app_ip}:${info.enrollment_port}/v1/enroll --token ${info.token} --ca-sha256 ${info.ca_fingerprint}`
    : "";
  $: shortFingerprint = info ? `${info.ca_fingerprint.slice(0, 20)}…` : "";
  $: mmss = `${String(Math.floor(remainingSecs / 60)).padStart(2, "0")}:${String(remainingSecs % 60).padStart(2, "0")}`;

  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard API can be unavailable depending on webview permissions;
      // the text is still selectable/visible either way.
    }
  }

  async function finish() {
    clearInterval(countdownTimer);
    clearInterval(pollTimer);
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
  <div class="modal wide">
    <div class="modal-header">
      <span class="modal-icon"><Icon name="plus" /></span>
      <div style="flex:1;">
        <div class="modal-title">Ajouter des postes</div>
        <div class="modal-desc">Exécutez cette commande sur chaque poste à protéger. Il apparaîtra ici automatiquement.</div>
      </div>
      {#if info}
        <span class="pill {remainingSecs > 0 ? 'pill-ok' : 'pill-danger'}"><span class="pill-dot"></span>Expire dans {mmss}</span>
      {/if}
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    {#if !info}
      <p class="subtitle">{starting ? "Démarrage…" : "…"}</p>
    {:else}
      <div class="row" style="margin-bottom:var(--space-3);">
        <div class="segmented">
          <button class:active={osTab === "windows"} on:click={() => (osTab = "windows")}>Windows</button>
          <button class:active={osTab === "linux"} on:click={() => (osTab = "linux")}>Linux</button>
          <button class:active={osTab === "offline"} on:click={() => (osTab = "offline")}>Hors ligne</button>
        </div>
        <div class="spacer"></div>
        <span class="subtitle">Groupe par défaut</span>
        <select bind:value={defaultGroupId} style="width:140px;">
          <option value="">(aucun)</option>
          {#each groups as g}<option value={g.id}>{g.name}</option>{/each}
        </select>
      </div>

      {#if osTab === "windows"}
        {#if info.binaries_available}
          <code class="cmd">{installPs1Command}</code>
          <button on:click={() => copy(installPs1Command)}>Copier</button>
          <p class="subtitle" style="margin:6px 0 0;">Dans PowerShell, en administrateur · binaire vérifié SHA-256 · CA {shortFingerprint}</p>
        {:else}
          <p class="subtitle">Binaire non embarqué dans cette build — utilisez l'onglet « Hors ligne ».</p>
        {/if}
      {:else if osTab === "linux"}
        {#if info.binaries_available}
          <code class="cmd">{installShCommand}</code>
          <button on:click={() => copy(installShCommand)}>Copier</button>
          <p class="subtitle" style="margin:6px 0 0;">En root (sudo) · binaire vérifié SHA-256 · CA {shortFingerprint}</p>
        {:else}
          <p class="subtitle">Binaire non embarqué dans cette build — utilisez l'onglet « Hors ligne ».</p>
        {/if}
      {:else}
        <p class="subtitle" style="margin:0 0 8px;">
          Copiez <code>homeguard-agent</code> sur le poste manuellement, puis exécutez :
        </p>
        <code class="cmd">{enrollCommand}</code>
        <button on:click={() => copy(enrollCommand)}>Copier</button>
      {/if}

      <div class="group" style="margin-top:var(--space-4);">
        <div class="group-title">Postes détectés pendant cette session</div>
        {#if detected.length === 0}
          <div class="row-line"><Icon name="wifi" style="color:var(--text-faint);" /><span class="subtitle">En attente d'autres postes…</span></div>
        {:else}
          {#each detected as d}
            <div class="row-line">
              <Icon name="check" style="color:var(--ok);" />
              <span class="subtitle" style="width:140px;">{d.hostname} · {d.os}</span>
              <input bind:value={d.name} on:blur={() => saveName(d)} on:keydown={(e) => e.key === "Enter" && saveName(d)} style="flex:1;" />
              {#if d.saved}<Icon name="check" style="color:var(--ok);" />{/if}
            </div>
          {/each}
          <div class="row-line"><Icon name="wifi" style="color:var(--text-faint);" /><span class="subtitle">En attente d'autres postes…</span></div>
        {/if}
      </div>

      {#if ipError}<p class="error" style="margin:8px 0 0;">{ipError}</p>{/if}
      {#if addingByIp}
        <div class="row" style="margin-top:var(--space-3); gap:6px;">
          <input
            placeholder="192.168.1.50"
            bind:value={ipInput}
            style="flex:1;"
            on:keydown={(e) => e.key === "Enter" && addByIp()}
          />
          <button class="primary" on:click={addByIp} disabled={ipProbing || !ipInput.trim()}>{ipProbing ? "Recherche…" : "Ajouter"}</button>
          <button on:click={() => { addingByIp = false; ipError = ""; }}>Annuler</button>
        </div>
      {/if}

      <div class="modal-actions">
        {#if !addingByIp}
          <button class="link" style="margin-right:auto;" on:click={() => (addingByIp = true)}>Ajouter par adresse IP…</button>
        {/if}
        <button class="primary" on:click={finish}>Terminer</button>
        <button on:click={start}>Nouveau jeton</button>
      </div>
    {/if}
  </div>
</div>
