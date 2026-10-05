<script lang="ts">
  // The whole stack on one page: each machine, its GPUs as labelled memory bars (who holds what), the Systems as
  // tiles under the GPU they run on (then several GPUs, the CPU, not set up). A tap opens the System's sheet.
  import { RUNNING, glyph, gb, shortLabel, statusColor, statusWord, tileLine } from './format';
  import type { WebGpu, WebMachine, WebState, WebSystem } from './types';

  let { web, stale, onopen }: { web: WebState; stale: string | null; onopen: (id: string) => void } = $props();

  const running = $derived(web.systems.filter(RUNNING).length);
  const faults = $derived(web.systems.filter((s) => s.status === 'fault').length);

  const systemsOf = (m: WebMachine) => web.systems.filter((s) => s.machine === m.id);
  const onGpu = (m: WebMachine, g: WebGpu) => systemsOf(m).filter((s) => s.gpus.length === 1 && s.gpus[0] === g.id);

  /** Systems not on exactly one of the machine's GPUs: several GPUs, the CPU, no GPU named, not set up. */
  function extraGroups(m: WebMachine): { title: string; systems: WebSystem[] }[] {
    const ids = new Set(m.gpus.map((g) => g.id));
    const out: { title: string; systems: WebSystem[] }[] = [];
    for (const s of systemsOf(m)) {
      if (s.gpus.length === 1 && ids.has(s.gpus[0])) continue;
      const title = !s.gpus.length
        ? s.status === 'not-set'
          ? 'Not set up'
          : 'Other'
        : s.gpus.length === 1 && s.gpus[0].toLowerCase() === 'cpu'
          ? 'CPU'
          : s.gpuNames.join(' + ');
      const g = out.find((x) => x.title === title);
      if (g) g.systems.push(s);
      else out.push({ title, systems: [s] });
    }
    return out;
  }

  function summary(m: WebMachine): string {
    if (m.state !== 'online') return '';
    const list = systemsOf(m);
    const on = list.filter(RUNNING).length;
    const bad = list.filter((s) => s.status === 'fault').length;
    return `${on} on${bad ? ` · ${bad} fault` : ''}`;
  }

  function meta(m: WebMachine): string {
    if (m.local) return `this machine${m.os ? ` · ${m.os}` : ''}`;
    if (m.state === 'online') return `remote${m.latencyMs !== undefined ? ` · ${Math.round(m.latencyMs)} ms` : ''}`;
    return m.state === 'connecting' ? 'connecting' : m.state === 'unauthorized' ? 'not authorized' : m.state === 'incompatible' ? 'other KLIF version' : 'unreachable';
  }

  const freeOf = (g: WebGpu) => Math.max(0, g.totalGiB - g.usedGiB);
  function legend(g: WebGpu): string {
    const parts = g.parts.map((p) => `${shortLabel(p.label)} ${gb(p.gib)}`);
    if (g.otherGiB > 0.05) parts.push(`other ${gb(g.otherGiB)}`);
    parts.push(`free ${gb(freeOf(g))}`);
    return `${parts.join(' · ')} GB`;
  }
</script>

{#snippet tile(s: WebSystem, wide: boolean, dead: boolean)}
  <button type="button" class="tile" class:wide class:dead style="--c:{statusColor(s)}" disabled={dead} onclick={() => onopen(s.id)}>
    <span class="tl"><b>{shortLabel(s.label)}</b><em><i class="g {glyph(s)}"></i>{statusWord(s)}</em></span>
    <span class="tm">{s.model ?? '—'}</span>
    <span class="t3" class:bad={s.status === 'fault' || s.status === 'invalid'}>{tileLine(s)}</span>
    {#if s.load}<span class="prog"><i style="width:{s.load.pct}%"></i></span>{/if}
  </button>
{/snippet}

{#snippet tiles(list: WebSystem[], dead = false)}
  <div class="tiles">
    {#each list as s, i (s.id)}{@render tile(s, list.length % 2 === 1 && i === list.length - 1, dead)}{/each}
  </div>
{/snippet}

<header class="top">
  <b>KLIF</b>
  {#if stale}
    <span class="live down" title={stale}><i></i>not answering</span>
  {:else}
    <span class="live"><i></i>{running} running{#if faults}<em> · {faults} fault</em>{/if}</span>
  {/if}
</header>
{#if stale}<p class="banner" role="status">{stale} Retrying.</p>{/if}

<main>
  {#each web.machines as m (m.id)}
    <section class:gone={m.state !== 'online'}>
      <h2>
        <span class="mn">{m.name}</span>
        <small>{meta(m)}</small>
        <span class="sum">{summary(m)}</span>
      </h2>
      {#if m.state !== 'online'}
        <p class="note">{m.error ?? 'KLIF cannot reach this machine now and keeps trying.'} These are its last known Systems.</p>
        {@render tiles(systemsOf(m), true)}
      {:else}
        {#each m.gpus as g (g.id)}
          <div class="gpu">
            <div class="gh"><span>{g.name}{g.unified ? ' · unified memory' : ''}</span><span class="num">{gb(freeOf(g))} GB free of {gb(g.totalGiB)}</span></div>
            <div class="map" role="img" aria-label={legend(g)}>
              {#each g.parts as p (p.system)}
                <i style="width:{(p.gib / g.totalGiB) * 100}%">
                  {#if p.gib / g.totalGiB >= 0.12}<span>{shortLabel(p.label)}</span><small>{gb(p.gib)}</small>{/if}
                </i>
              {/each}
              {#if g.otherGiB > 0.05}<i class="other" style="width:{(g.otherGiB / g.totalGiB) * 100}%"></i>{/if}
            </div>
            <p class="legend">{legend(g)}</p>
            {#if onGpu(m, g).length}{@render tiles(onGpu(m, g))}{/if}
          </div>
        {/each}
        {#each extraGroups(m) as grp (grp.title)}
          <div class="gpu">
            <div class="gh"><span>{grp.title}</span></div>
            {@render tiles(grp.systems)}
          </div>
        {/each}
      {/if}
    </section>
  {/each}
</main>

<style>
  .top {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: var(--k-bg);
    border-bottom: 1px solid var(--k-line);
  }
  .top b {
    font: 800 20px/1 var(--k-font-display);
    letter-spacing: 0.06em;
  }
  .live {
    display: flex;
    align-items: center;
    gap: 7px;
    font: 500 13px var(--k-font-data);
    color: var(--k-muted);
  }
  .live i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--k-accent);
  }
  .live em {
    font-style: normal;
    color: var(--k-danger);
  }
  .live.down {
    color: var(--k-warn);
  }
  .live.down i {
    background: transparent;
    border: 2px solid var(--k-warn);
  }
  .banner {
    margin: 0;
    padding: 10px 16px;
    border-bottom: 1px solid var(--k-line);
    background: color-mix(in srgb, var(--k-warn) 12%, transparent);
    font-size: 14px;
    line-height: 1.4;
  }
  section {
    padding: 4px 16px 16px;
    border-bottom: 1px solid var(--k-line);
  }
  h2 {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 4px 10px;
    margin: 14px 0 10px;
  }
  .mn {
    font: 700 22px/1.1 var(--k-font-display);
  }
  h2 small {
    font: 400 13px var(--k-font-ui);
    color: var(--k-muted);
  }
  .sum {
    margin-left: auto;
    font: 500 13px var(--k-font-data);
  }
  section.gone .mn,
  section.gone small {
    color: var(--k-muted);
  }
  .note {
    margin: 0 0 4px;
    padding: 10px 12px;
    border: 1px dashed var(--k-line);
    border-radius: var(--k-radius);
    font-size: 14px;
    line-height: 1.4;
    color: var(--k-muted);
  }
  .gpu + .gpu {
    margin-top: 18px;
  }
  .gh {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 6px;
    font-size: 13px;
    color: var(--k-muted);
  }
  .num {
    font-family: var(--k-font-data);
    white-space: nowrap;
  }
  .map {
    display: flex;
    height: 34px;
    border: 1px solid var(--k-line);
    border-radius: var(--k-radius);
    background: var(--k-surface);
    overflow: hidden;
  }
  .map i {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    background: color-mix(in srgb, var(--k-accent) 78%, var(--k-bg));
    color: var(--k-accent-ink);
    border-right: 2px solid var(--k-bg);
    font-style: normal;
    white-space: nowrap;
    overflow: hidden;
  }
  .map i span {
    font: 700 13px var(--k-font-display);
  }
  .map i small {
    font: 500 12px var(--k-font-data);
  }
  .map i.other {
    padding: 0;
    background: repeating-linear-gradient(135deg, var(--k-surface-raised) 0 4px, var(--k-line) 4px 6px);
  }
  .legend {
    margin: 5px 0 0;
    font: 500 12px var(--k-font-data);
    color: var(--k-muted);
  }
  .tiles {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-top: 10px;
  }
  .tile {
    position: relative;
    display: grid;
    gap: 3px;
    min-width: 0;
    min-height: 84px;
    padding: 10px 12px 12px;
    border: 1px solid var(--k-line);
    border-top: 3px solid var(--c);
    border-radius: var(--k-radius);
    background: var(--k-surface);
    text-align: left;
    cursor: pointer;
    overflow: hidden;
  }
  .tile.wide {
    grid-column: 1 / -1;
  }
  .tile.dead {
    border-style: dashed;
    background: transparent;
    cursor: default;
  }
  .tl {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .tl b {
    font: 700 17px var(--k-font-display);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tl em {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    font-style: normal;
    font-size: 13px;
    font-weight: 600;
    color: var(--c);
  }
  .tm {
    font-size: 14px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t3 {
    min-height: 1.2em;
    font: 500 13px var(--k-font-data);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t3.bad {
    color: var(--k-danger);
  }
  .dead .tm,
  .dead b,
  .dead .t3 {
    color: var(--k-muted);
  }
  .prog {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 3px;
    background: var(--k-surface-raised);
  }
  .prog i {
    display: block;
    height: 100%;
    background: var(--k-warn);
  }
</style>
