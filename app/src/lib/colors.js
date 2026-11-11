// The picker's palette — also used as the deterministic fallback below,
// so a list nobody's assigned a color to yet still gets a stable, on-
// palette swatch instead of looking unstyled.
export const LIST_COLOR_PALETTE = ["#8a4fd1", "#e2822a", "#d6453d", "#2f7fd6", "#2a9d5c", "#c2398a"];

function hashColor(id) {
  let hash = 0;
  for (let i = 0; i < id.length; i++) {
    hash = (hash * 31 + id.charCodeAt(i)) >>> 0;
  }
  return LIST_COLOR_PALETTE[hash % LIST_COLOR_PALETTE.length];
}

// A list's real, persisted color when it has one; otherwise a swatch
// derived from its id, so every list still renders consistently (sidebar,
// detail header, elsewhere) before anyone's actually picked a color.
export function listColor(list) {
  return list?.color || hashColor(typeof list === "string" ? list : list.id);
}
