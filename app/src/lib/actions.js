// Tiny Svelte action — `use:autofocus` instead of the non-standard
// `autofocus` HTML attribute (which vite-plugin-svelte flags as an a11y
// smell, since it steals focus regardless of how the element appeared).
// These call sites are all elements that only exist because the admin
// just asked for them (an inline rename/create field appearing on click),
// so moving focus there is the expected, requested behavior.
export function autofocus(node) {
  node.focus();
}
