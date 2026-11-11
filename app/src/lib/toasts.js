import { writable } from "svelte/store";

// App-wide, non-blocking feedback ("DESKTOP-X synchronisé · 42 domaines
// bloqués"). Errors stay until dismissed or acted on; everything else
// fades on its own.
export const toasts = writable([]);

let nextId = 1;
const DEFAULT_TIMEOUT = { ok: 4000, info: 4000, warn: 8000, danger: 0 };

/**
 * @param {object} t
 * @param {"ok"|"info"|"warn"|"danger"} [t.kind]
 * @param {string} t.title
 * @param {string} [t.detail]
 * @param {{label: string, run: () => void}} [t.action]
 * @param {number} [t.timeout] ms; 0 = sticky
 * @param {string} [t.key] a toast with the same key replaces the previous one
 */
export function toast({ kind = "info", title, detail = "", action = null, timeout, key = null }) {
  const id = nextId++;
  const ms = timeout ?? DEFAULT_TIMEOUT[kind];
  toasts.update((list) => [...list.filter((x) => !key || x.key !== key), { id, kind, title, detail, action, key }]);
  if (ms > 0) setTimeout(() => dismiss(id), ms);
  return id;
}

export function dismiss(id) {
  toasts.update((list) => list.filter((x) => x.id !== id));
}
