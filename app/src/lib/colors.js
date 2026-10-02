// Deterministic, non-stored color per list — a visual convenience derived
// from the list's id so swatches stay consistent across the sidebar, its
// detail header, and anywhere else it's referenced. Not a real "list
// color" feature yet (that would be a user-chosen, persisted field).
const PALETTE = ["#8a4fd1", "#e2822a", "#d6453d", "#2f7fd6", "#2a9d5c", "#c2398a"];

export function listColor(id) {
  let hash = 0;
  for (let i = 0; i < id.length; i++) {
    hash = (hash * 31 + id.charCodeAt(i)) >>> 0;
  }
  return PALETTE[hash % PALETTE.length];
}
