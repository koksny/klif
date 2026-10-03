<script lang="ts">
  // Rings, full window: an optical laboratory. The specimen (Bubble Rings) is painted full-bleed behind the whole
  // window; the UI is frosted glass over it. FIXED SKELETON, scaled by --u (1 design px at 1024x1152): the same
  // regions at the same places in every phase, only their contents change:
  //   header      glass bar: KLIF · RINGS, status pill, uptime / version, PANEL, window controls (frameless)
  //   tier tabs   a segmented glass control, four equal pills (aria-disabled while a session exists)
  //   model line
  //   stage       the sphere + three ring gauges at 38% of the width; the hero card and two small cards right
  //   strip       four glass cells (LLM: prefill, decode, context, speculative; CGI: last image, images, size, mode)
  //   timeline    8 request capsules (cyan prefill, mint decode) / 12 job bars (height = time, edits hatched)
  //   controls    endpoint + API key chips; the primary pill (one width: Launch / Cancel / Stop / Restart after a
  //               fault); Restart (Dismiss after a fault), Tune (always), Endpoint / Web UI, Console (Full log)
  //   console     the last log line
  import type { Actions, RequestRecord, Slot, ViewModel } from '../../lib/model/types';
  import { fmtClock, fmtInt, fmtSeconds, tierShort } from '../../lib/model/format';
  import Specimen from './Specimen.svelte';
  import WinCtl from './WinCtl.svelte';
  import { extentOf } from './gauges';
  import { heroOf } from './hero';
  import { useSleep } from './sleep.svelte';
  import { availabilityText, modelRest, phaseWord, shapeText } from './text';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  let w = $state(1024);
  let h = $state(1152);
  const u = $derived(Math.max(0.72, Math.min(1.2, Math.min(w / 1024, h / 1152))));

  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const slotOf = (id: string): Slot | undefined => vm.slots.find((x) => x.id === id);
  const sessionSlot = $derived(s ? slotOf(s.slot) : undefined);
  const sel = $derived(slotOf(vm.selected) ?? vm.slots[0]);
  const kind = $derived(sessionSlot?.kind ?? (s?.image ? 'image' : s?.llm ? 'llm' : (sel?.kind ?? 'llm')));
  const model = $derived(s?.model ?? sel?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const loading = $derived(phase === 'starting' || phase === 'loading');
  const busy = $derived(loading || phase === 'stopping');
  const faulted = $derived(phase === 'fault');
  const online = $derived(phase === 'live');
  const selReady = $derived(sel?.availability === 'ready');
  const frameless = $derived(!!vm.host?.frameless);
  const panel = $derived(vm.host?.panel?.available ? vm.host.panel : null);
  const panelTip = $derived(`Panel mode: show on the small screen${panel?.target ? ` (${panel.target})` : ''}`);

  const sleep = useSleep(() => vm);
  const dz = $derived(online ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);

  const hero = $derived(heroOf(vm, { kind, sel, dz, waking, full: true }));
  const heroSize = $derived(hero.value.length <= 4 ? 120 : hero.value.length === 5 ? 98 : 84);

  // ---- the two small cards under the hero ----------------------------------------------------------------------
  const cards = $derived.by<[string, string][]>(() => {
    if (kind === 'image') {
      const on = !!img && img.activity === 'generating';
      const j = img?.recent[img.recent.length - 1];
      return [
        ['job', on ? `${fmtSeconds(img!.elapsedS)} · ${img!.width}×${img!.height}${img!.edit ? ' · edit' : ''}` : j ? `last ${fmtSeconds(j.seconds)} · ${j.width}×${j.height}` : img ? 'no job yet' : '—'],
        ['model', shapeText(model, 'image')],
      ];
    }
    const total = llm?.context.totalTokens || model?.ctxTokens || 0;
    return [
      ['context', llm ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(total)}` : total ? `— / ${fmtInt(total)}` : '—'],
      ['model', shapeText(model, 'llm')],
    ];
  });

  // ---- detail strip: four fixed facts per kind, the same labels in every phase -----------------------------------
  const prefillActive = $derived(!!llm?.prefill && llm.activity === 'prefill');
  const strip = $derived.by<[string, string][]>(() => {
    if (kind === 'image') {
      const j = img?.recent[img.recent.length - 1];
      return [
        ['last image', j ? `${fmtSeconds(j.seconds)} · ${j.width}×${j.height}${j.edit ? ' · edit' : ''}` : img ? 'none yet' : '—'],
        ['images', img ? `${fmtInt(img.imagesThisSession)} this session` : '—'],
        ['size', model?.imageSize ?? '—'],
        ['mode', model?.mode ?? '—'],
      ];
    }
    const pf = llm?.prefill;
    const total = llm?.context.totalTokens || model?.ctxTokens || 0;
    return [
      ['prefill', pf ? (prefillActive ? `${pf.cachedTokens ? `${fmtInt(pf.cachedTokens)} cached · ` : ''}running ${fmtSeconds(pf.elapsedS)}` : `${fmtInt(pf.tokens)} tok · ${fmtInt(pf.tps)} tok/s`) : llm ? 'no request yet' : '—'],
      ['decode', llm ? (prefillActive ? 'waiting for prefill' : `${fmtInt(llm.generatedTokens)} tok generated`) : '—'],
      ['context', llm ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(total)} · ${Math.round((llm.context.usedTokens / Math.max(1, total)) * 100)}%` : total ? `— / ${fmtInt(total)}` : '—'],
      ['speculative', llm?.spec ? `${Math.round(llm.spec.acceptancePct)}% · ${llm.spec.mode}` : llm ? 'off' : (model?.specMode ?? 'off')],
    ];
  });

  // ---- timeline: 8 request places (a fault marks the last) / 12 job places ---------------------------------------
  const reqN = $derived(faulted && llm ? 7 : 8);
  const requests = $derived(llm ? llm.requests.slice(-reqN) : []);
  const reqSlots = $derived(Array.from({ length: reqN }, (_, i) => requests[i - (reqN - requests.length)] ?? null));
  const jobN = $derived(faulted && img ? 11 : 12);
  const jobs = $derived(img ? img.recent.slice(-jobN) : []);
  const jobSlots = $derived(Array.from({ length: jobN }, (_, i) => jobs[i - (jobN - jobs.length)] ?? null));
  const maxJob = $derived(Math.max(1, ...jobs.map((j) => j.seconds)));
  const diedInWork = $derived(faulted && ((!!llm && llm.activity !== 'idle') || (!!img && img.activity === 'generating')));
  const tlCaption = $derived.by(() => {
    if (!s) return 'not running';
    if (loading) return kind === 'image' ? 'no jobs yet' : 'no requests yet';
    const n = kind === 'image' ? jobs.length : requests.length;
    const what = kind === 'image' ? 'jobs' : 'requests';
    if (faulted) {
      if (!llm && !img) return `no ${what}: failed during startup`;
      return `${n ? `last ${n}` : `no finished ${what}`}, then ${diedInWork ? `the ${kind === 'image' ? 'job' : 'request'} that died` : 'the fault'}`;
    }
    if (n === 0) return `no finished ${what} yet`;
    return kind === 'image' ? `bar height = time · last ${n} of ${fmtInt(img?.imagesThisSession ?? n)}` : `last ${n} · prefill cyan, decode mint`;
  });
  const reqTitle = (r: RequestRecord) =>
    `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${fmtSeconds(r.prefillS)} · ${fmtInt(r.generatedTokens)} tok in ${fmtSeconds(r.decodeS)}`;

  // ---- header, model line, console line --------------------------------------------------------------------------
  const statusText = $derived(dz ? (waking ? 'GPU waking' : 'GPU asleep') : phaseWord(phase));
  const statusTone = $derived(faulted ? 'danger' : dz || loading || phase === 'stopping' ? 'warn' : s ? 'live' : 'off');
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');

  // ---- stage geometry: the sphere at 38% of the width, as large as the gauges, the caption and the hero allow ----
  let stageEl: HTMLElement;
  let st = $state({ x: 0, y: 0, w: 0, h: 0 });
  const measure = () => {
    if (stageEl) st = { x: stageEl.offsetLeft, y: stageEl.offsetTop, w: stageEl.offsetWidth, h: stageEl.offsetHeight };
  };
  $effect(() => {
    const ro = new ResizeObserver(measure);
    ro.observe(stageEl);
    measure();
    return () => ro.disconnect();
  });
  $effect(() => {
    void [w, h, u];
    measure();
  });
  const HERO_W = 316;
  const heroW = $derived(Math.min(HERO_W * u, st.w * 0.42));
  const geo = $derived.by(() => {
    const pad = 12 * u;
    const e0 = extentOf('full', 0, u);
    const e1 = extentOf('full', 1, u);
    const a = { up: e1.up - e0.up, down: e1.down - e0.down, side: e1.side - e0.side };
    const cx = w * 0.38;
    const heroLeft = st.x + st.w - heroW;
    const rH = (st.h - 2 * pad - e0.up - e0.down) / (a.up + a.down);
    const rL = (cx - st.x - pad - e0.side) / a.side;
    const rR = (heroLeft - 18 * u - cx - e0.side) / a.side;
    const r = Math.max(40, Math.min(rH, rL, rR));
    const up = e0.up + a.up * r;
    const down = e0.down + a.down * r;
    const cy = st.y + (st.h - up - down) / 2 + up;
    return { cx, cy, r };
  });
</script>

<div class="full" bind:clientWidth={w} bind:clientHeight={h} style="--u:{u}px">
  <Specimen {vm} {kind} {sel} {dz} {waking} cx={geo.cx} cy={geo.cy} r={st.h > 0 ? geo.r : 0} variant="full" k={u} />

  <!-- Header (window drag region; draws its own window controls when the host is frameless) -->
  <header class="hdr glass" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region title="Koksny.com LOCAL INFERENCE FORNICATOR"><span class="wm">KLIF</span><span class="sl">RINGS</span></div>
    <span class="st {statusTone}" data-tauri-drag-region><i class:pulse={waking || loading}></i>{statusText}</span>
    <span class="grow" data-tauri-drag-region></span>
    <span class="ck" data-tauri-drag-region>
      {#if s}<em>{loading ? 'elapsed' : 'up'}</em> {fmtClock(loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS)}{:else if vm.host?.appVersion}<em>v</em>{vm.host.appVersion}{/if}
    </span>
    {#if panel}
      <button class="pbtn" class:on={panel.active} onclick={() => actions.togglePanel?.()} title={panelTip} aria-label={panelTip} aria-pressed={panel.active}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="2.5" y="4" width="19" height="12.5" rx="2" /><path d="M8.5 20.5h7M12 16.5v4" /></svg>
        <span>PANEL</span>
      </button>
    {/if}
    {#if frameless}<WinCtl {actions} maximized={!!vm.host?.maximized} />{/if}
  </header>

  <!-- Tier tabs: select while nothing runs (double-click launches); aria-disabled while a session exists -->
  <div class="tiers glass" role="tablist" aria-label="Tier">
    {#each vm.slots as t (t.id)}
      {@const selected = t.id === vm.selected}
      {@const na = t.availability !== 'ready'}
      {@const running = !!s && s.slot === t.id && !faulted}
      <button
        class="tier"
        class:sel={selected}
        class:na
        class:fault={faulted && s?.slot === t.id}
        role="tab"
        aria-selected={selected}
        aria-disabled={!!s}
        title={s ? `${t.label}: ${s.slot === t.id ? `${phase}` : `locked while ${s.model.name} is ${phase}`}` : na ? `${t.label}: ${t.reason ?? availabilityText(t.availability)}` : `${t.label}: ${t.model.name}`}
        onclick={() => {
          if (!vm.session) actions.select(t.id);
        }}
        ondblclick={() => {
          if (!vm.session && !na) actions.launch(t.id);
        }}
      >
        <span class="tl">{t.label}{#if running}<i class="rn" class:warn={loading || !!dz}></i>{/if}</span>
        <span class="tm" class:bad={na}>{na ? `${t.model.name} · ${availabilityText(t.availability)}` : `${t.model.name} · ${t.model.quant}`}</span>
      </button>
    {/each}
  </div>

  <div class="mline" class:dim={!s}><span class="mn">{model?.name ?? ''}</span><span class="mr">{modelRest(model, kind)}</span></div>

  <!-- Stage: the gauges are drawn around the sphere by Specimen; the hero sits on the right -->
  <section class="stage" bind:this={stageEl}>
    <div class="hstack" style="width:{heroW}px">
      <div class="hero glass {hero.tone}" class:fault={faulted}>
        <div class="hl">{hero.label}</div>
        <div class="hbody">
          <div class="hv" style="--hs:{heroSize}"><b>{hero.value}</b>{#if hero.unit}<span class="hu">{hero.unit}</span>{/if}</div>
          <div class="hs" class:warn={hero.subTone === 'warn'}>{hero.sub}</div>
        </div>
        {#if hero.exit}
          <div class="hlog"><div class="ex">{hero.exit}</div>{#each hero.log ?? [] as line, i (i)}<div title={line}>{line}</div>{/each}</div>
        {/if}
      </div>
      {#if !faulted}
        {#each cards as [k, v] (k)}
          <div class="card glass" class:dim={!s}><span class="k">{k}</span><span class="v" title={v}>{v}</span></div>
        {/each}
      {/if}
    </div>
  </section>

  <section class="strip" class:dim={!s}>
    {#each strip as [k, v] (k)}<div class="cell glass"><span class="k">{k}</span><span class="v" title={v}>{v}</span></div>{/each}
  </section>

  <section class="tline glass">
    <div class="cap">{kind === 'image' ? 'Recent jobs' : 'Requests'}<span class="dim">{tlCaption}</span></div>
    {#if kind === 'image'}
      <div class="jobs">
        {#each jobSlots as j, i (i)}
          {#if j}
            <span class="job" class:edit={j.edit} style="--h:{(j.seconds / maxJob) * 100}%" title="{fmtSeconds(j.seconds)} · {j.width}×{j.height}{j.edit ? ' · edit' : ''}"></span>
          {:else}<span class="job empty"></span>{/if}
        {/each}
        {#if faulted && img}<span class="job failed" title={diedInWork ? 'The server died during this job' : 'The server died between jobs'}></span>{/if}
      </div>
    {:else}
      <div class="reqs">
        {#each reqSlots as r, i (i)}
          {#if r}
            {@const tot = r.prefillS + r.decodeS}
            <span class="req" class:cur={i === reqN - 1 && !faulted} title={reqTitle(r)}><span class="pre" style="width:{tot > 0 ? Math.max(6, (r.prefillS / tot) * 100) : 0}%"></span><span class="dec"></span></span>
          {:else}<span class="req empty"></span>{/if}
        {/each}
        {#if faulted && llm}<span class="req failed" title={diedInWork ? 'The server died during this request' : 'The server died with no request in flight'}></span>{/if}
      </div>
    {/if}
  </section>

  <footer class="ctrl glass">
    <button class="chip" onclick={() => actions.copyEndpoint()} disabled={!online} title={online && s ? `Copy http://${s.endpoint.host}:${s.endpoint.port}` : 'Endpoint offline'}>
      <span class="ck2">PORT</span>:{s?.endpoint.port ?? sel?.recipe?.port ?? '—'}{#if !online}<em>offline</em>{/if}
    </button>
    <button class="chip" onclick={() => actions.copyApiKey()} disabled={!online || !s?.apiKeySet} title={s?.apiKeySet ? 'Copy API key' : 'No API key set'}>
      <span class="ck2">KEY</span>{#if s?.apiKeySet}•••••••<em>copy</em>{:else}<em>none</em>{/if}
    </button>
    <span class="grow"></span>
    {#if !s}
      <button class="act go" onclick={() => actions.launch(vm.selected)} disabled={!selReady} title={selReady ? `Launch ${sel?.label ?? ''}` : `${sel?.label ?? ''}: ${sel?.reason ?? availabilityText(sel?.availability ?? 'unsupported')}`}>
        <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.5 2.2l6 3.8-6 3.8z" /></svg>Launch {tierShort(sel?.label ?? '')}
      </button>
    {:else if faulted}
      <button class="act hot" onclick={() => actions.restart()} title="Launch {sessionSlot?.label ?? ''} again">
        <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M9.6 4.2A4 4 0 1 0 10 7" /><path d="M10 1.8v2.6H7.4" /></svg>Restart {tierShort(sessionSlot?.label ?? '')}
      </button>
    {:else}
      <button class="act stop" onclick={() => actions.stop()} disabled={phase === 'stopping'} title={loading ? 'Cancel the launch' : phase === 'stopping' ? 'Stopping' : 'Stop the server'}>
        <svg viewBox="0 0 12 12" aria-hidden="true"><rect x="3" y="3" width="6" height="6" rx="1" /></svg>{loading ? 'Cancel' : phase === 'stopping' ? 'Stopping' : 'Stop'}
        {tierShort(sessionSlot?.label ?? '')}
      </button>
    {/if}
    {#if faulted}
      <button class="btn w1" onclick={() => actions.dismiss?.()} title="Back to the launcher">Dismiss</button>
    {:else}
      <button class="btn w1" onclick={() => actions.restart()} disabled={!s || busy} title="Stop and launch again with the current settings">Restart</button>
    {/if}
    <button class="btn" onclick={() => actions.openTune(vm.selected)} title={s && s.slot === vm.selected && !faulted ? 'Change the settings; Restart to apply them' : 'Change what this tier launches'}>Tune</button>
    <button class="btn w2" onclick={() => actions.openEndpoint()} disabled={!online} title={kind === 'image' ? 'Open the sd-server web UI' : 'Open the endpoint'}>{kind === 'image' ? 'Web UI' : 'Endpoint'}</button>
    <button class="btn w2" onclick={() => actions.toggleConsole(faulted ? true : undefined)}>{faulted ? 'Full log' : 'Console'}</button>
  </footer>
  <button class="cline" class:hot={faulted} onclick={() => actions.toggleConsole(true)} title="Open the console"><span class="pr">›</span><span class="lt">{lastLine}</span></button>
</div>

<style>
  .full {
    --ink: #e3f7ff;
    --muted: #8fa9b8;
    --faint: #5d7484;
    --cyan: #39e1ff;
    --mint: #4cff99;
    --warn: #ffb347;
    --danger: #ff5c7a;
    --edge: rgba(120, 220, 255, 0.16);
    --edge2: rgba(120, 220, 255, 0.3);
    --fill: rgba(255, 255, 255, 0.045);
    --rad: calc(var(--u) * 14);
    --caps: 'Barlow Condensed', 'Barlow', system-ui, sans-serif;
    --mono: 'Share Tech Mono', ui-monospace, monospace;
    --fs-xs: max(10.5px, calc(var(--u) * 11.5));
    --fs-s: max(11px, calc(var(--u) * 12.5));
    --fs-m: max(12px, calc(var(--u) * 14));
    --fs-l: max(13px, calc(var(--u) * 16));
    --gap: calc(var(--u) * 10);
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    padding: calc(var(--u) * 10) calc(var(--u) * 14);
    font-family: 'Barlow', 'Segoe UI', system-ui, sans-serif;
    font-size: var(--fs-m);
    font-variant-numeric: tabular-nums;
    color: var(--ink);
    background: #191a26;
    overflow: hidden;
    user-select: none;
  }
  .full > :global(*:not(.specimen)) {
    flex: none;
    min-width: 0;
    position: relative;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 1px solid var(--cyan);
    outline-offset: 2px;
  }
  .glass {
    background: var(--fill);
    -webkit-backdrop-filter: blur(14px) saturate(120%);
    backdrop-filter: blur(14px) saturate(120%);
    border: 1px solid var(--edge);
    border-radius: var(--rad);
  }
  .grow {
    flex: 1 1 auto;
    align-self: stretch;
  }
  .k,
  .cap,
  .hl,
  .st,
  .sl,
  .tl,
  .pbtn,
  .act,
  .btn,
  .ck2 {
    font-family: var(--caps);
    font-weight: 600;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  /* header */
  .hdr {
    height: calc(var(--u) * 50);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 14);
    padding: 0 calc(var(--u) * 11) 0 calc(var(--u) * 20);
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 12);
    white-space: nowrap;
  }
  .wm {
    font-size: max(15px, calc(var(--u) * 19));
    font-weight: 600;
    letter-spacing: 0.32em;
  }
  .sl {
    padding-left: calc(var(--u) * 12);
    border-left: 1px solid var(--edge2);
    font-size: var(--fs-s);
    font-weight: 500;
    letter-spacing: 0.3em;
    color: var(--muted);
  }
  .st {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 8);
    height: calc(var(--u) * 26);
    padding: 0 calc(var(--u) * 12) 0 calc(var(--u) * 10);
    border-radius: 999px;
    border: 1px solid var(--edge);
    background: rgba(255, 255, 255, 0.03);
    font-size: var(--fs-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .st i {
    width: calc(var(--u) * 7);
    height: calc(var(--u) * 7);
    min-width: 6px;
    min-height: 6px;
    border-radius: 50%;
    background: #4a5a66;
  }
  .st i.pulse {
    animation: pulse 1.2s ease-in-out infinite;
  }
  .st.live {
    color: var(--ink);
    border-color: rgba(76, 255, 153, 0.3);
  }
  .st.live i {
    background: var(--mint);
    box-shadow: 0 0 8px var(--mint);
  }
  .st.warn {
    color: var(--warn);
    border-color: rgba(255, 179, 71, 0.3);
  }
  .st.warn i {
    background: var(--warn);
    box-shadow: 0 0 8px var(--warn);
  }
  .st.danger {
    color: var(--danger);
    border-color: rgba(255, 92, 122, 0.4);
  }
  .st.danger i {
    background: var(--danger);
    box-shadow: 0 0 8px var(--danger);
  }
  .ck {
    font-size: var(--fs-l);
    color: var(--ink);
    white-space: nowrap;
  }
  .ck em {
    font-style: normal;
    font-family: var(--caps);
    font-weight: 600;
    font-size: var(--fs-xs);
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--muted);
    margin-right: calc(var(--u) * 3);
  }
  .pbtn {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 7);
    height: calc(var(--u) * 28);
    padding: 0 calc(var(--u) * 12) 0 calc(var(--u) * 10);
    border-radius: 999px;
    border: 1px solid var(--edge);
    background: rgba(255, 255, 255, 0.04);
    font-size: var(--fs-xs);
    color: var(--muted);
    white-space: nowrap;
    flex: none;
  }
  .pbtn svg {
    width: calc(var(--u) * 15);
    height: calc(var(--u) * 15);
    min-width: 12px;
    min-height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
  }
  .pbtn:hover {
    color: var(--ink);
    border-color: var(--edge2);
  }
  .pbtn.on {
    color: var(--cyan);
    border-color: rgba(57, 225, 255, 0.55);
  }
  .hdr :global(.wctl) {
    margin-left: calc(var(--u) * 2);
  }

  /* tier tabs: one glass control, four equal pills */
  .tiers {
    height: calc(var(--u) * 62);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: calc(var(--u) * 5);
    padding: calc(var(--u) * 5);
  }
  .tier {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 3);
    min-width: 0;
    padding: 0 calc(var(--u) * 14);
    border-radius: calc(var(--u) * 10);
    border: 1px solid transparent;
    text-align: left;
    transition:
      background-color 0.18s ease,
      border-color 0.18s ease;
  }
  .tier:hover:not(.sel):not([aria-disabled='true']) {
    background: rgba(255, 255, 255, 0.04);
  }
  .tier[aria-disabled='true'] {
    cursor: default;
  }
  .tl {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8);
    font-size: var(--fs-s);
    color: var(--muted);
    white-space: nowrap;
  }
  .rn {
    width: calc(var(--u) * 7);
    height: calc(var(--u) * 7);
    min-width: 6px;
    min-height: 6px;
    border-radius: 50%;
    background: var(--mint);
    box-shadow: 0 0 8px var(--mint);
  }
  .rn.warn {
    background: var(--warn);
    box-shadow: 0 0 8px var(--warn);
  }
  .tm {
    font-size: var(--fs-xs);
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tm.bad {
    color: var(--warn);
  }
  .tier.sel {
    border-color: rgba(57, 225, 255, 0.6);
    background: rgba(57, 225, 255, 0.1);
    box-shadow: 0 0 18px rgba(57, 225, 255, 0.12) inset;
  }
  .tier.sel .tl {
    color: var(--ink);
  }
  .tier.sel .tm {
    color: var(--muted);
  }
  .tier.na {
    opacity: 0.4;
  }
  .tier.fault {
    border-color: rgba(255, 92, 122, 0.7);
    background: rgba(255, 92, 122, 0.1);
  }

  /* model line */
  .mline {
    height: calc(var(--u) * 20);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 10);
    padding: 0 calc(var(--u) * 20);
    white-space: nowrap;
    overflow: hidden;
  }
  .mn {
    font-weight: 500;
    color: var(--ink);
    flex: none;
  }
  .mr {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mline.dim .mn {
    color: #b9cdd8;
  }

  /* stage: absorbs the remaining height; the specimen behind it is drawn full-bleed */
  .stage {
    flex: 1 1 0 !important;
    min-height: calc(var(--u) * 280);
  }
  .hstack {
    position: absolute;
    right: 0;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  .hero {
    height: calc(var(--u) * 240);
    display: flex;
    flex-direction: column;
    padding: calc(var(--u) * 20) calc(var(--u) * 22);
    overflow: hidden;
  }
  .hero.fault {
    height: calc(var(--u) * 388);
    border-color: rgba(255, 92, 122, 0.45);
    background: linear-gradient(180deg, rgba(255, 92, 122, 0.1), rgba(255, 255, 255, 0.03) 60%);
  }
  .hl {
    font-size: var(--fs-m);
    color: var(--cyan);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hbody {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }
  .hv {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 10);
    white-space: nowrap;
  }
  .hv b {
    font-weight: 200;
    font-size: calc(var(--u) * var(--hs));
    line-height: 0.92;
    letter-spacing: -0.02em;
    color: var(--ink);
    text-shadow: 0 0 30px rgba(57, 225, 255, 0.25);
  }
  .hu {
    font-size: var(--fs-l);
    color: var(--muted);
  }
  .hs {
    margin-top: calc(var(--u) * 12);
    font-size: var(--fs-m);
    line-height: 1.3;
    color: var(--muted);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .hs.warn {
    color: var(--warn);
  }
  .hero.dim .hv b {
    color: #6f8796;
    text-shadow: none;
  }
  .hero.warn .hl {
    color: var(--warn);
  }
  .hero.warn .hv b {
    color: #ffe2b8;
    text-shadow: 0 0 30px rgba(255, 179, 71, 0.25);
  }
  .hero.danger .hl,
  .hero.danger .hv b {
    color: var(--danger);
    text-shadow: 0 0 30px rgba(255, 92, 122, 0.35);
  }
  .hero.danger .hs {
    color: var(--ink);
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }
  .hlog {
    padding-top: calc(var(--u) * 10);
    border-top: 1px solid rgba(255, 92, 122, 0.25);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    line-height: 1.45;
    color: var(--muted);
  }
  .hlog .ex {
    color: var(--danger);
  }
  .hlog div {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .card {
    height: calc(var(--u) * 64);
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 4);
    padding: 0 calc(var(--u) * 22);
  }
  .k {
    font-size: var(--fs-xs);
    color: var(--muted);
    white-space: nowrap;
  }
  .card .v,
  .cell .v {
    font-size: var(--fs-l);
    color: var(--ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .card.dim .v,
  .strip.dim .v {
    color: #b9cdd8;
  }

  /* detail strip */
  .strip {
    height: calc(var(--u) * 66);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--gap);
  }
  .cell {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 4);
    min-width: 0;
    padding: 0 calc(var(--u) * 16);
  }

  /* timeline */
  .tline {
    height: calc(var(--u) * 92);
    display: flex;
    flex-direction: column;
    gap: calc(var(--u) * 10);
    padding: calc(var(--u) * 12) calc(var(--u) * 18);
  }
  .cap {
    font-size: var(--fs-xs);
    color: var(--cyan);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cap .dim {
    margin-left: calc(var(--u) * 12);
    font-family: 'Barlow', system-ui, sans-serif;
    font-weight: 400;
    letter-spacing: 0.02em;
    text-transform: none;
    color: var(--muted);
  }
  .reqs {
    flex: 1;
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 10);
  }
  .req {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    gap: 2px;
    height: calc(var(--u) * 18);
    padding: 2px;
    border-radius: 999px;
    border: 1px solid var(--edge);
    background: rgba(255, 255, 255, 0.03);
    overflow: hidden;
  }
  .req .pre {
    border-radius: 999px 0 0 999px;
    background: var(--cyan);
    box-shadow: 0 0 8px rgba(57, 225, 255, 0.45);
  }
  .req .dec {
    flex: 1;
    border-radius: 0 999px 999px 0;
    background: rgba(76, 255, 153, 0.75);
  }
  .req.cur .dec {
    background: var(--mint);
    box-shadow: 0 0 8px rgba(76, 255, 153, 0.5);
  }
  .req.empty {
    border-style: dashed;
    background: none;
  }
  .req.failed,
  .job.failed {
    border: 1px dashed var(--danger);
    background: repeating-linear-gradient(-45deg, rgba(255, 92, 122, 0.5) 0 1.5px, transparent 1.5px 6px);
  }
  .jobs {
    flex: 1;
    display: flex;
    align-items: flex-end;
    gap: calc(var(--u) * 8);
    min-height: 0;
  }
  .job {
    flex: 1 1 0;
    min-width: 0;
    height: var(--h);
    min-height: 4px;
    border-radius: calc(var(--u) * 5) calc(var(--u) * 5) 2px 2px;
    background: linear-gradient(180deg, var(--cyan), rgba(57, 225, 255, 0.45));
    box-shadow: 0 0 10px rgba(57, 225, 255, 0.3);
  }
  .job.edit {
    background: repeating-linear-gradient(-45deg, var(--cyan) 0 3px, rgba(57, 225, 255, 0.3) 3px 6px);
  }
  .job.empty {
    height: 4px;
    background: none;
    box-shadow: none;
    border-top: 1px dashed var(--edge2);
    border-radius: 0;
  }
  .job.failed {
    height: 100%;
    border-radius: calc(var(--u) * 5);
  }

  /* controls */
  .ctrl {
    height: calc(var(--u) * 60);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8);
    padding: 0 calc(var(--u) * 10);
    border-radius: 999px;
  }
  .chip,
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: calc(var(--u) * 7);
    height: calc(var(--u) * 38);
    padding: 0 calc(var(--u) * 15);
    border-radius: 999px;
    border: 1px solid var(--edge);
    background: rgba(255, 255, 255, 0.04);
    color: var(--ink);
    white-space: nowrap;
    flex: none;
    transition:
      border-color 0.15s ease,
      background-color 0.15s ease;
  }
  .chip {
    font-size: var(--fs-m);
  }
  .ck2 {
    font-size: var(--fs-xs);
    color: var(--muted);
    margin-right: calc(var(--u) * 2);
  }
  .chip em {
    font-style: normal;
    color: var(--muted);
    margin-left: calc(var(--u) * 3);
  }
  .btn {
    font-size: var(--fs-s);
  }
  .btn.w1 {
    width: calc(var(--u) * 96);
  }
  .btn.w2 {
    width: calc(var(--u) * 104);
  }
  .chip:hover:not(:disabled),
  .btn:hover:not(:disabled) {
    border-color: var(--edge2);
    background: rgba(57, 225, 255, 0.08);
  }
  .chip:disabled {
    cursor: default;
    color: var(--muted);
  }
  .btn:disabled {
    cursor: default;
    opacity: 0.4;
  }
  /* the primary place: one width for Launch / Cancel / Stop / Restart */
  .act {
    flex: none;
    width: calc(var(--u) * 220);
    height: calc(var(--u) * 42);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: calc(var(--u) * 9);
    border-radius: 999px;
    font-size: var(--fs-m);
    white-space: nowrap;
    overflow: hidden;
    transition:
      background-color 0.15s ease,
      box-shadow 0.15s ease;
  }
  .act svg {
    width: calc(var(--u) * 12);
    height: calc(var(--u) * 12);
    min-width: 10px;
    min-height: 10px;
    fill: currentColor;
    stroke: none;
    flex: none;
  }
  .act.go {
    background: var(--cyan);
    color: #04202a;
    box-shadow: 0 0 24px rgba(57, 225, 255, 0.35);
  }
  .act.go:hover:not(:disabled) {
    background: #7aecff;
  }
  .act.go:disabled {
    cursor: default;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid var(--edge);
    color: var(--muted);
    box-shadow: none;
  }
  .act.stop {
    border: 1.5px solid var(--cyan);
    color: var(--cyan);
    background: rgba(57, 225, 255, 0.06);
  }
  .act.stop:hover:not(:disabled) {
    background: rgba(57, 225, 255, 0.14);
  }
  .act.stop:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .act.hot {
    background: var(--danger);
    color: #2a0610;
    box-shadow: 0 0 24px rgba(255, 92, 122, 0.35);
  }
  .act.hot svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }
  .act.hot:hover {
    background: #ff8098;
  }

  /* console line */
  .cline {
    height: calc(var(--u) * 18);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8);
    padding: 0 calc(var(--u) * 20);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-align: left;
  }
  .cline .pr {
    color: var(--cyan);
  }
  .cline .lt {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cline:hover .lt {
    color: var(--muted);
  }
  .cline.hot .lt {
    color: #c98a98;
  }
  @media (prefers-reduced-motion: reduce) {
    .st i.pulse {
      animation: none;
    }
  }
</style>
