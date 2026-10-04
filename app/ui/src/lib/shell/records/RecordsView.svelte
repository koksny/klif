<script lang="ts">
  // Records: the best value each model file reached on each backend (vm.records), this machine and the nodes, as
  // cards or a ranked list. A shell surface over the skin in the full window, themed by the skin's --k-* variables:
  // the machine and its FP32 TFLOPS on top, metric tabs, filters, then the cards; a card opens its details, where
  // the shareable card is exported.
  import { player } from '../../state/player.svelte';
  import { ui } from '../../state/ui.svelte';
  import type { RecordEntry, RecordMetric } from '../../model/types';
  import MachinePlate from './MachinePlate.svelte';
  import RecordCard from './RecordCard.svelte';
  import RecordDetail from './RecordDetail.svelte';
  import RecordList from './RecordList.svelte';
  import { backendLabel, freshEvent, hardwareOf, presentMetrics, ranked } from './metrics';

  type View = 'cards' | 'list';
  const VIEW_KEY = 'klif.records.view';
  function readView(): View {
    try {
      return localStorage.getItem(VIEW_KEY) === 'list' ? 'list' : 'cards';
    } catch {
      return 'cards';
    }
  }

  const vm = $derived(player.vm);
  const entries = $derived(vm.records ?? []);
  const events = $derived(vm.recordEvents ?? []);
  const now = $derived(vm.now);

  let metricPick = $state<RecordMetric>('decodeTps');
  let view = $state<View>(readView());
  let machine = $state<string>('all'); // 'all' | 'local' | node id
  let backend = $state<string>('all');
  let query = $state('');
  let sort = $state<'best' | 'newest'>('best');
  let selectedKey = $state<string | null>(null);

  const metrics = $derived(presentMetrics(entries));
  const metric = $derived(metrics.some((m) => m.id === metricPick) ? metricPick : metrics[0].id);

  const machines = $derived.by(() => {
    const out: { id: string; name: string }[] = [];
    for (const e of entries) {
      const id = e.node ?? 'local';
      if (!out.some((x) => x.id === id)) out.push({ id, name: e.node ? e.machine : 'This machine' });
    }
    return out;
  });
  const backends = $derived([...new Set(entries.filter((e) => e.best[metric]).map((e) => backendLabel(e.backend)))].sort());

  // A filter whose value no longer exists (another metric, a node gone offline) falls back to all, so its chips,
  // hidden when there is one choice, never hide a filter that empties the screen.
  const machineSel = $derived(machine === 'all' || machines.some((m) => m.id === machine) ? machine : 'all');
  const backendSel = $derived(backend === 'all' || backends.includes(backend) ? backend : 'all');
  const filtered = $derived(
    entries.filter((e) => {
      if (machineSel !== 'all' && (e.node ?? 'local') !== machineSel) return false;
      if (backendSel !== 'all' && backendLabel(e.backend) !== backendSel) return false;
      const q = query.trim().toLowerCase();
      if (q && ![e.model.name, e.model.quant ?? '', e.model.file, e.backend, e.machine].some((s) => s.toLowerCase().includes(q))) return false;
      return true;
    }),
  );
  const byValue = $derived(ranked(filtered, metric));
  const rows = $derived(sort === 'best' ? byValue : [...byValue].sort((a, b) => b.best[metric]!.at - a.best[metric]!.at));
  const rankOf = (e: RecordEntry) => byValue.indexOf(e) + 1;
  const leader = $derived(byValue[0]?.best[metric]?.value ?? 0);
  const selected = $derived(selectedKey ? (entries.find((e) => e.key === selectedKey && e.best[metric]) ?? null) : null);

  const plateNode = $derived(machineSel !== 'all' && machineSel !== 'local' ? machineSel : undefined);
  const plateHw = $derived(hardwareOf(vm, plateNode));
  const plateName = $derived(plateNode ? (vm.nodes.find((n) => n.id === plateNode)?.name ?? plateNode) : 'This machine');

  function setView(v: View) {
    view = v;
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      // Per-viewer convenience only.
    }
  }

  function open(e: RecordEntry) {
    selectedKey = selectedKey === e.key ? null : e.key;
  }

  async function loadClimb(key: string, m: RecordMetric) {
    const lines = await player.config.recordsHistory(key, m);
    return lines.filter((l) => l.metric === m).map((l) => ({ at: l.at, value: l.new }));
  }

  function forgetFor(e: RecordEntry): (() => Promise<void>) | null {
    const node = e.node ? vm.nodes.find((n) => n.id === e.node) : undefined;
    if (node && !node.allow.includes('edit')) return null;
    const key = e.node && e.key.startsWith(`${e.node}/`) ? e.key.slice(e.node.length + 1) : e.key;
    return async () => {
      await player.actions.forgetRecord(key, e.node);
      selectedKey = null;
      ui.toast(`Forgot ${e.model.name} on ${backendLabel(e.backend)}`);
    };
  }
</script>

<div class="records" role="dialog" aria-label="Records" data-records>
  <div class="top" data-tauri-drag-region>
    <span class="title" data-tauri-drag-region>Records</span>
    <span class="hint" data-tauri-drag-region>The best each model file reached on each backend</span>
    <span class="grow" data-tauri-drag-region></span>
    <button
      type="button"
      class="moment"
      role="switch"
      aria-checked={vm.config.recordMoment}
      title="Celebrate a broken record over the skin ([ui] record_moment)"
      onclick={() => void player.actions.updateSettings({ recordMoment: !vm.config.recordMoment }).catch(() => {})}
    >
      <i></i>New record moment
    </button>
    <button type="button" class="back" onclick={() => ui.closeRecords()} title="Back to the Systems (Esc)">← Systems</button>
    {#if vm.host.frameless}
      <button type="button" class="wc" onclick={() => player.actions.minimize()} aria-label="Minimize">—</button>
      <button type="button" class="wc" onclick={() => player.actions.toggleMaximize()} aria-label="Maximize">□</button>
      <button type="button" class="wc" onclick={() => player.actions.closeWindow()} aria-label="Close">✕</button>
    {/if}
  </div>

  <div class="plate"><MachinePlate hw={plateHw} name={plateName} /></div>

  <div class="bar">
    <div class="tabs" role="tablist" aria-label="Metric">
      {#each metrics as m (m.id)}
        <button type="button" role="tab" aria-selected={metric === m.id} class:on={metric === m.id} onclick={() => (metricPick = m.id)}>{m.label}</button>
      {/each}
    </div>
    <span class="grow"></span>
    {#if machines.length > 1}
      <div class="chips" aria-label="Machine">
        <button type="button" class:on={machineSel === 'all'} onclick={() => (machine = 'all')}>All machines</button>
        {#each machines as mc (mc.id)}<button type="button" class:on={machineSel === mc.id} onclick={() => (machine = mc.id)}>{mc.name}</button>{/each}
      </div>
    {/if}
    {#if backends.length > 1}
      <div class="chips" aria-label="Backend">
        <button type="button" class:on={backendSel === 'all'} onclick={() => (backend = 'all')}>All</button>
        {#each backends as b}<button type="button" class:on={backendSel === b} onclick={() => (backend = b)}>{b}</button>{/each}
      </div>
    {/if}
    <input class="search" type="search" placeholder="Search models" bind:value={query} aria-label="Search models" />
    <div class="seg" aria-label="Sort">
      <button type="button" class:on={sort === 'best'} onclick={() => (sort = 'best')}>Best</button>
      <button type="button" class:on={sort === 'newest'} onclick={() => (sort = 'newest')}>Newest</button>
    </div>
    <div class="seg" aria-label="View">
      <button type="button" class:on={view === 'cards'} onclick={() => setView('cards')} title="Cards">▦</button>
      <button type="button" class:on={view === 'list'} onclick={() => setView('list')} title="List">☰</button>
    </div>
  </div>

  <div class="body" class:with-detail={!!selected}>
    <div class="content">
      {#if !entries.length}
        <div class="empty">
          <b>No records yet.</b>
          <p>
            KLIF keeps the best decode and prefill speed, time to first token and time per image that each model file
            reaches on each backend, from everyday use and from <code>klif-cli bench</code>. Run a model and they appear
            here.
          </p>
        </div>
      {:else if !rows.length}
        <div class="empty"><b>No record matches these filters.</b></div>
      {:else if view === 'cards'}
        <div class="cards">
          {#each rows as e (e.key)}
            <RecordCard
              entry={e}
              {metric}
              rank={rankOf(e)}
              {leader}
              event={freshEvent(events, e, metric, now)}
              {now}
              selected={e.key === selectedKey}
              onopen={() => open(e)}
            />
          {/each}
        </div>
      {:else}
        <RecordList {rows} {metric} {leader} {rankOf} {events} {now} {selectedKey} onopen={open} />
      {/if}
    </div>
    {#if selected}
      <RecordDetail
        entry={selected}
        {metric}
        {now}
        hw={hardwareOf(vm, selected.node)}
        fresh={freshEvent(events, selected, metric, now)}
        version={vm.host.appVersion}
        {loadClimb}
        save={(name, blob) => player.config.saveImage(name, blob)}
        forget={forgetFor(selected)}
        toast={(t) => ui.toast(t)}
        onclose={() => (selectedKey = null)}
      />
    {/if}
  </div>
</div>

<style>
  .records {
    position: absolute;
    inset: 0;
    z-index: 30;
    display: grid;
    grid-template-rows: auto auto auto minmax(0, 1fr);
    container: records / inline-size;
    background:
      radial-gradient(110% 80% at 75% 0%, color-mix(in srgb, var(--k-accent, #5ab6eb) 7%, transparent) 0%, transparent 60%),
      var(--k-bg, #0f1316);
    color: var(--k-ink, #eee);
    font: 400 13px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 42px;
    padding: 0 10px 0 18px;
    border-bottom: 1px solid var(--k-line, #2c363c);
  }
  .title {
    font: 700 15px var(--k-font-display, system-ui, sans-serif);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .hint {
    font-size: 12px;
    color: var(--k-muted, #8a8a8a);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grow {
    flex: 1;
  }
  button {
    font: inherit;
    color: inherit;
    cursor: pointer;
  }
  button:focus-visible,
  .search:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
    outline-offset: 1px;
  }
  .back {
    padding: 5px 12px;
    border: 1px solid var(--k-line, #2c363c);
    border-radius: var(--k-radius, 6px);
    background: transparent;
    font-size: 12px;
  }
  .moment {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px 4px 6px;
    border: 1px solid var(--k-line, #2c363c);
    border-radius: 999px;
    background: transparent;
    font-size: 12px;
    color: var(--k-muted, #8a8a8a);
  }
  .moment i {
    position: relative;
    width: 26px;
    height: 14px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--k-ink, #eee) 12%, transparent);
    transition: background 0.15s ease;
  }
  .moment i::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--k-muted, #8a8a8a);
    transition:
      transform 0.15s ease,
      background 0.15s ease;
  }
  .moment[aria-checked='true'] {
    color: var(--k-ink, #eee);
  }
  .moment[aria-checked='true'] i {
    background: color-mix(in srgb, var(--k-record, #f2a33a) 35%, transparent);
  }
  .moment[aria-checked='true'] i::after {
    transform: translateX(12px);
    background: var(--k-record, #f2a33a);
  }
  .wc {
    width: 30px;
    border: 0;
    background: transparent;
    color: var(--k-muted, #8a8a8a);
  }
  .plate {
    padding: 18px 22px 16px;
    border-bottom: 1px solid var(--k-line, #2c363c);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    padding: 10px 18px;
    border-bottom: 1px solid var(--k-line, #2c363c);
  }
  .tabs {
    display: flex;
    gap: 2px;
  }
  .tabs button {
    padding: 6px 12px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    font: 600 11px var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .tabs button.on {
    background: var(--k-surface, #1d252a);
    color: var(--k-ink, #eee);
    box-shadow: inset 0 -2px 0 var(--k-accent, #5ab6eb);
  }
  .chips,
  .seg {
    display: flex;
    gap: 4px;
  }
  .chips button {
    padding: 4px 10px;
    border: 1px solid var(--k-line, #2c363c);
    border-radius: 999px;
    background: transparent;
    font-size: 11.5px;
    color: var(--k-muted, #8a8a8a);
  }
  .chips button.on {
    border-color: color-mix(in srgb, var(--k-accent, #5ab6eb) 60%, transparent);
    background: color-mix(in srgb, var(--k-accent, #5ab6eb) 12%, transparent);
    color: var(--k-ink, #eee);
  }
  .seg {
    gap: 0;
    border: 1px solid var(--k-line, #2c363c);
    border-radius: var(--k-radius, 6px);
    overflow: hidden;
  }
  .seg button {
    padding: 4px 10px;
    border: 0;
    background: transparent;
    font-size: 12px;
    color: var(--k-muted, #8a8a8a);
  }
  .seg button.on {
    background: var(--k-surface, #1d252a);
    color: var(--k-ink, #eee);
  }
  .search {
    width: 150px;
    padding: 5px 9px;
    border: 1px solid var(--k-line, #2c363c);
    border-radius: var(--k-radius, 6px);
    background: transparent;
    color: var(--k-ink, #eee);
    font: 12px var(--k-font-ui, system-ui, sans-serif);
  }
  .body {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
  }
  .body.with-detail {
    grid-template-columns: minmax(0, 1fr) 340px;
  }
  .content {
    overflow-x: hidden;
    overflow-y: auto;
    padding: 16px 18px 24px;
    min-height: 0;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 14px;
  }
  .empty {
    max-width: 520px;
    margin: 40px auto;
    text-align: center;
    color: var(--k-muted, #8a8a8a);
  }
  .empty b {
    display: block;
    margin-bottom: 8px;
    font: 600 15px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  .empty code {
    font: 12px var(--k-font-data, monospace);
    color: var(--k-ink, #eee);
  }
  @container records (max-width: 900px) {
    .body.with-detail {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
    }
    .hint {
      display: none;
    }
  }
</style>
