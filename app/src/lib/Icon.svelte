<script>
  // Small hand-drawn icon set (24x24 viewBox, stroke-based) — no icon
  // library dependency. Sizing/color come from the .icon / .icon-lg
  // classes in app.css (stroke: currentColor).
  export let name = "";
  export let large = false;
</script>

<svg class="icon {large ? 'icon-lg' : ''}" viewBox="0 0 24 24" {...$$restProps}>
  {#if name === "dashboard"}
    <rect x="3" y="3" width="8" height="8" rx="2" />
    <rect x="13" y="3" width="8" height="5" rx="2" />
    <rect x="13" y="10" width="8" height="11" rx="2" />
    <rect x="3" y="13" width="8" height="8" rx="2" />
  {:else if name === "monitor"}
    <rect x="3" y="4" width="18" height="12" rx="2" />
    <path d="M9 20h6M12 16v4" />
  {:else if name === "users"}
    <circle cx="9" cy="8" r="3.2" />
    <path d="M3.5 20c0-3.3 2.5-6 5.5-6s5.5 2.7 5.5 6" />
    <path d="M16 9.3a3 3 0 1 0-1-5.9" />
    <path d="M15 14.3c2.5.5 4.5 2.8 4.5 5.7" />
  {:else if name === "list"}
    <circle cx="4.5" cy="6" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="4.5" cy="12" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="4.5" cy="18" r="1.1" fill="currentColor" stroke="none" />
    <path d="M9 6h11M9 12h11M9 18h11" />
  {:else if name === "history"}
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5V12l3.2 2" />
  {:else if name === "settings"}
    <path d="M4 6h9M17 6h3M4 12h3M9 12h11M4 18h13M19 18h1" />
    <circle cx="13" cy="6" r="2.1" />
    <circle cx="7" cy="12" r="2.1" />
    <circle cx="17" cy="18" r="2.1" />
  {:else if name === "plus"}
    <path d="M12 5v14M5 12h14" />
  {:else if name === "x"}
    <path d="M6 6l12 12M18 6L6 18" />
  {:else if name === "search"}
    <circle cx="10.5" cy="10.5" r="6.5" />
    <path d="M19 19l-4.3-4.3" />
  {:else if name === "warning"}
    <path d="M12 3.5 21.5 20h-19L12 3.5Z" />
    <path d="M12 10v4.2" />
    <circle cx="12" cy="17.3" r="0.9" fill="currentColor" stroke="none" />
  {:else if name === "check"}
    <path d="M4.5 12.5 9.5 17.5 19.5 6.5" />
  {:else if name === "clock-forward"}
    <circle cx="11" cy="12.5" r="7.5" />
    <path d="M11 8v4.5l3 2" />
    <path d="M18.5 6 21 6.5 20.5 9" />
  {:else if name === "shield"}
    <path d="M12 3.2 19 6v6.2c0 4.4-3 7.7-7 8.6-4-.9-7-4.2-7-8.6V6l7-2.8Z" />
  {:else if name === "inbox"}
    <path d="M3.5 12h5l1.5 3h4l1.5-3h5" />
    <path d="M5 5h14l2.5 7v7a1.5 1.5 0 0 1-1.5 1.5h-16A1.5 1.5 0 0 1 2.5 19v-7L5 5Z" />
  {:else if name === "wifi-off"}
    <path d="M2 8.5c2.4-2 5.2-3.2 8.2-3.5M22 8.5a17.6 17.6 0 0 0-4.9-3.1M6 12.3c1.6-1.2 3.5-1.9 5.5-2M18 12.3a9 9 0 0 0-2.3-1.6M9 16c.9-.6 2-1 3-1s2.1.4 3 1M12 20h.01" />
    <path d="M3 3l18 18" />
  {:else if name === "wifi"}
    <path d="M2 8.5c2.9-2.4 6.3-3.5 10-3.5s7.1 1.1 10 3.5M5.5 12.5c2-1.4 4.2-2 6.5-2s4.5.6 6.5 2M9 16.3c1-.5 2-.8 3-.8s2 .3 3 .8M12 20h.01" />
  {:else if name === "refresh"}
    <path d="M4 12a8 8 0 0 1 13.7-5.7L20 8.5M20 4v4.5h-4.5" />
    <path d="M20 12a8 8 0 0 1-13.7 5.7L4 15.5M4 20v-4.5h4.5" />
  {:else if name === "more"}
    <circle cx="5" cy="12" r="1.3" fill="currentColor" stroke="none" />
    <circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none" />
    <circle cx="19" cy="12" r="1.3" fill="currentColor" stroke="none" />
  {:else if name === "key"}
    <circle cx="8" cy="15" r="3.5" />
    <path d="M10.3 12.6 19 4M16.2 7.3l2.3 2.3M13.4 10.1l1.8 1.8" />
  {:else if name === "lock-open"}
    <rect x="5" y="11" width="14" height="9" rx="2" />
    <path d="M8 11V7a4 4 0 0 1 7.5-2" />
  {:else if name === "minus"}
    <path d="M5 12h14" />
  {/if}
</svg>
