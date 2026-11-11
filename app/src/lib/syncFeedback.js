import { toast } from "./toasts.js";

// Turns sync outcomes (see `SyncOutcome` in src-tauri/src/sync.rs) into
// toasts and status labels. The app-level actions a toast can offer
// ("Réenrôler…", "Voir le poste") are registered once by App.svelte.
let handlers = { openEnroll: () => {}, goToAgent: () => {} };
export function setFeedbackHandlers(h) {
  handlers = { ...handlers, ...h };
}

// Error kinds where retrying won't help: the poste itself needs fixing.
const NEEDS_REENROLL = new Set(["tls_rejected", "wrong_identity"]);

/** The one action that most likely fixes this failure, if any. */
export function remedyFor(outcome, retry) {
  const kind = outcome.error?.kind;
  if (NEEDS_REENROLL.has(kind)) return { label: "Réenrôler…", run: () => handlers.openEnroll() };
  if (kind === "incompatible" || kind === "revoked" || kind === "no_address") {
    return { label: "Voir le poste", run: () => handlers.goToAgent(outcome.agent_id) };
  }
  return retry ? { label: "Réessayer", run: retry } : null;
}

export function reportOutcome(outcome, retry) {
  if (outcome.ok) {
    toast({ kind: "ok", title: `${outcome.hostname} synchronisé`, detail: outcome.detail, key: `sync-${outcome.agent_id}` });
  } else {
    toast({
      kind: "danger",
      title: `${outcome.hostname} : échec de la synchronisation`,
      detail: outcome.detail,
      action: remedyFor(outcome, retry),
      key: `sync-${outcome.agent_id}`,
    });
  }
}

const plural = (n, s = "s") => (n > 1 ? s : "");

/**
 * One summary toast for a multi-poste run.
 * trigger "manual": the user clicked "Tout synchroniser" — always report.
 * trigger "auto": pushed after an edit — say the rules went out.
 * trigger "launch": the scan on startup — only speak up if something's wrong.
 */
export function reportRun(outcomes, { trigger = "manual", retry } = {}) {
  const ok = outcomes.filter((o) => o.ok);
  const failed = outcomes.filter((o) => !o.ok);

  if (outcomes.length === 0) {
    if (trigger !== "auto") {
      toast({ kind: "warn", title: "Aucun poste trouvé", detail: "Aucun poste n'a répondu sur le réseau." });
    }
    return;
  }

  if (failed.length === 0) {
    if (trigger === "launch") return;
    toast({
      kind: "ok",
      key: "sync-run",
      title: trigger === "auto" ? "Règles appliquées" : "Tout est synchronisé",
      detail:
        ok.length === 1
          ? `${ok[0].hostname} · ${ok[0].detail}`
          : `${ok.length} postes à jour.`,
    });
    return;
  }

  const head =
    trigger === "auto"
      ? `Règles enregistrées, ${failed.length} poste${plural(failed.length)} non mis à jour`
      : `${failed.length} poste${plural(failed.length)} sur ${outcomes.length} non synchronisé${plural(failed.length)}`;
  const detail =
    failed.length === 1
      ? `${failed[0].hostname} — ${failed[0].detail}`
      : failed.map((o) => o.hostname).join(", ") +
        (trigger === "auto" ? " — ils recevront les règles à la prochaine synchronisation." : "");
  toast({
    kind: "warn",
    key: "sync-run",
    title: head,
    detail,
    action: failed.length === 1 ? remedyFor(failed[0], retry) : retry ? { label: "Réessayer", run: retry } : null,
  });
}

export function relativeTime(iso) {
  if (!iso) return "";
  const s = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
  if (s < 45) return "à l'instant";
  const m = Math.round(s / 60);
  if (m < 60) return `il y a ${m} min`;
  const h = Math.round(m / 60);
  if (h < 24) return `il y a ${h} h`;
  const d = Math.round(h / 24);
  return `il y a ${d} j`;
}

/**
 * Per-poste sync state, for the sidebar dot and the poste detail line.
 * kind: "ok" | "pending" | "failed" | "never"
 */
export function syncState(agent) {
  if (!agent || agent.revoked) return { kind: "never", label: "" };
  if (agent.last_sync_ok === false) {
    return {
      kind: "failed",
      label: `Échec ${relativeTime(agent.last_sync_at)}`,
      detail: agent.last_sync_detail ?? "",
    };
  }
  if (agent.policy_dirty) {
    return { kind: "pending", label: "Modifications en attente", detail: "Ce poste n'a pas encore reçu les dernières règles." };
  }
  if (agent.last_sync_ok === true) {
    return { kind: "ok", label: `Synchronisé ${relativeTime(agent.last_sync_at)}`, detail: agent.last_sync_detail ?? "" };
  }
  return { kind: "never", label: "Jamais synchronisé", detail: "" };
}
