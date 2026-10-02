<script>
  import { createEventDispatcher } from "svelte";

  // `null` means "toujours bloqué" (no schedule at all) — the caller is
  // the one that decides whether that's even an option (see `mode` below).
  export let schedule = null;

  const dispatch = createEventDispatcher();

  const DAY_KEYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
  const DAY_LABELS = { mon: "Lun", tue: "Mar", wed: "Mer", thu: "Jeu", fri: "Ven", sat: "Sam", sun: "Dim" };
  const SLOTS = 48; // 30-minute increments across 24h
  const AXIS_HOURS = [0, 6, 12, 18, 24];

  // JS's Sunday-first getDay() (0..6) reindexed to our Monday-first keys.
  const TODAY_KEY = DAY_KEYS[(new Date().getDay() + 6) % 7];

  function emptyRanges() {
    return Object.fromEntries(DAY_KEYS.map((d) => [d, null]));
  }

  function timeStrToSlot(hhmmss) {
    const [h, m] = hhmmss.split(":").map(Number);
    if (h === 23 && m === 59) return SLOTS;
    return h * 2 + (m >= 30 ? 1 : 0);
  }

  function slotToTimeStr(slot) {
    if (slot >= SLOTS) return "23:59:59";
    const totalMin = slot * 30;
    const h = Math.floor(totalMin / 60);
    const m = totalMin % 60;
    return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:00`;
  }

  function scheduleToRanges(sched) {
    const ranges = emptyRanges();
    if (!sched) return ranges;
    for (const w of sched) {
      const from = timeStrToSlot(w.from);
      const to = timeStrToSlot(w.to);
      for (const d of w.days) ranges[d] = { from, to };
    }
    return ranges;
  }

  function rangesToSchedule(ranges) {
    const groups = new Map();
    for (const d of DAY_KEYS) {
      const r = ranges[d];
      if (!r) continue;
      const key = `${r.from}-${r.to}`;
      if (!groups.has(key)) groups.set(key, { from: r.from, to: r.to, days: [] });
      groups.get(key).days.push(d);
    }
    if (groups.size === 0) return null;
    return [...groups.values()].map((g) => ({ days: g.days, from: slotToTimeStr(g.from), to: slotToTimeStr(g.to) }));
  }

  function rangesEqual(a, b) {
    return DAY_KEYS.every((d) => {
      const ra = a[d];
      const rb = b[d];
      if (!ra && !rb) return true;
      if (!ra || !rb) return false;
      return ra.from === rb.from && ra.to === rb.to;
    });
  }

  // Segments to actually paint for one day's range — two when it wraps
  // past midnight (from > to), matching `TimeWindow::contains`'s own
  // overnight handling.
  function daySegments(range) {
    if (!range) return [];
    if (range.from <= range.to) return [{ from: range.from, to: range.to }];
    return [
      { from: 0, to: range.to },
      { from: range.from, to: SLOTS },
    ];
  }

  let ranges = scheduleToRanges(schedule);
  let mode = schedule ? "scheduled" : "always";

  function emit() {
    dispatch("change", mode === "always" ? null : rangesToSchedule(ranges));
  }

  function setMode(next) {
    mode = next;
    emit();
  }

  const PRESETS = [
    {
      label: "Jours d'école 8 h–18 h",
      build: () => {
        const r = emptyRanges();
        for (const d of ["mon", "tue", "wed", "thu", "fri"]) r[d] = { from: 16, to: 36 };
        return r;
      },
    },
    {
      label: "Soirées 21 h–7 h",
      build: () => {
        const r = emptyRanges();
        for (const d of DAY_KEYS) r[d] = { from: 42, to: 14 };
        return r;
      },
    },
    {
      label: "Week-end",
      build: () => {
        const r = emptyRanges();
        r.sat = { from: 0, to: SLOTS };
        r.sun = { from: 0, to: SLOTS };
        return r;
      },
    },
  ];

  function applyPreset(preset) {
    ranges = preset.build();
    mode = "scheduled";
    emit();
  }

  let dragDay = null;
  let dragStartSlot = 0;

  function slotAtEvent(e) {
    const rect = e.currentTarget.getBoundingClientRect();
    const fraction = (e.clientX - rect.left) / rect.width;
    return Math.min(SLOTS - 1, Math.max(0, Math.floor(fraction * SLOTS)));
  }

  function startPaint(day, e) {
    if (e.buttons !== 1) return;
    dragDay = day;
    dragStartSlot = slotAtEvent(e);
    ranges[day] = { from: dragStartSlot, to: dragStartSlot + 1 };
    ranges = ranges;
  }

  function continuePaint(day, e) {
    if (dragDay !== day || e.buttons !== 1) return;
    const slot = slotAtEvent(e);
    ranges[day] = { from: Math.min(dragStartSlot, slot), to: Math.max(dragStartSlot, slot) + 1 };
    ranges = ranges;
  }

  function endPaint() {
    if (dragDay === null) return;
    dragDay = null;
    emit();
  }

  function clearDay(day) {
    ranges[day] = null;
    ranges = ranges;
    emit();
  }

  $: activePreset = mode === "scheduled" ? PRESETS.find((p) => rangesEqual(ranges, p.build())) : null;
  $: nowFraction = (new Date().getHours() * 60 + new Date().getMinutes()) / (24 * 60);
</script>

<svelte:window on:mouseup={endPaint} />

<div class="segmented">
  <button class:active={mode === "always"} on:click={() => setMode("always")}>Toujours</button>
  <button class:active={mode === "scheduled"} on:click={() => setMode("scheduled")}>Selon un horaire</button>
</div>

{#if mode === "scheduled"}
  <div class="row" style="flex-wrap:wrap; gap:6px; margin-top:var(--space-3);">
    {#each PRESETS as preset}
      <button class="pill-btn" class:active={activePreset === preset} on:click={() => applyPreset(preset)}>{preset.label}</button>
    {/each}
  </div>

  <div class="sched-grid">
    {#each DAY_KEYS as day}
      <div class="sched-row">
        <span class="sched-label" class:today={day === TODAY_KEY}>{DAY_LABELS[day]}</span>
        <div
          class="sched-track"
          role="presentation"
          on:mousedown={(e) => startPaint(day, e)}
          on:mousemove={(e) => continuePaint(day, e)}
        >
          {#each daySegments(ranges[day]) as seg}
            <span class="sched-seg" style="left:{(seg.from / SLOTS) * 100}%; width:{((seg.to - seg.from) / SLOTS) * 100}%;"></span>
          {/each}
          {#if day === TODAY_KEY}
            <span class="sched-now" style="left:{nowFraction * 100}%;"></span>
          {/if}
        </div>
        {#if ranges[day]}
          <button class="toolbar-icon-btn" on:click={() => clearDay(day)} aria-label="Effacer {DAY_LABELS[day]}">×</button>
        {:else}
          <span style="width:26px;"></span>
        {/if}
      </div>
    {/each}
    <div class="sched-axis">
      {#each AXIS_HOURS as h}<span>{h} h</span>{/each}
    </div>
  </div>

  <div class="row" style="gap:var(--space-4); margin-top:var(--space-2);">
    <span class="row" style="gap:5px; font-size:var(--text-xs); color:var(--text-dim);"><span class="sched-swatch blocked"></span>Bloqué</span>
    <span class="row" style="gap:5px; font-size:var(--text-xs); color:var(--text-dim);"><span class="sched-swatch"></span>Autorisé</span>
    <span class="subtitle">Glisser pour peindre · pas de 30 min</span>
  </div>
{/if}
