<script lang="ts">
  // Loom: a prototype skin outside the app. A transformer in 3D (scene.ts) as the main drawing, amber phosphor,
  // the usual fixed KLIF skeleton around it. The mock engine drives it exactly as it drives the skins.
  import { onMount } from 'svelte';
  import { player } from '../../src/lib/state/player.svelte';
  import { ui } from '../../src/lib/state/ui.svelte';
  import { fmtClock, fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../src/lib/model/format';
  import { LoomScene, archOf } from './scene';
  import Lock from '../decode/Lock.svelte';

  player.init(ui.params);
  const scenario = new URLSearchParams(location.search).get('scenario') ?? 'live-low';

  const vm = $derived(player.vm);
  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const dz = $derived(phase === 'live' ? (vm.vram.dormant ?? null) : null);
  const sel = $derived(vm.systems.find((x) => x.id === vm.selected));
  const runSlot = $derived(s ? vm.systems.find((x) => x.id === s.system) : undefined);
  const model = $derived(s?.model ?? sel?.model);
  const kind = $derived(runSlot?.kind ?? sel?.kind ?? 'llm');
  const arch = $derived(archOf(model?.name));
  let shown = $state(0);
  let cacheHit = $state(0);

  type Hero = { label: string; note: string; value: string; unit: string; sub: string; tone: '' | 'dim' | 'red' };
  const hero = $derived.by<Hero>(() => {
    if (!s) return { label: 'STANDBY', note: '', value: '—', unit: kind === 'image' ? 'steps' : 'tok/s', sub: vm.lastSession ? `last ${vm.lastSession.model.name.toLowerCase()}` : 'nothing running', tone: 'dim' };
    if (phase === 'fault') return { label: 'FAULT', note: '', value: 'ERR', unit: '', sub: s.fault?.title ?? 'the server stopped', tone: 'red' };
    if (phase === 'stopping') return { label: 'STOPPING', note: '', value: '—', unit: '', sub: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB`, tone: 'dim' };
    if (phase === 'starting' || phase === 'loading') {
      const st = s.loading?.steps.find((x) => x.state === 'active');
      return { label: 'LOAD', note: 'layers build bottom-up', value: String(Math.floor((s.loading?.fraction ?? 0) * 100)), unit: '%', sub: st ? `${st.label.toLowerCase()}${st.detail ? ` · ${st.detail}` : ''}` : 'starting', tone: '' };
    }
    if (dz) return { label: `GPU ASLEEP${dz.powerState ? ` · ${dz.powerState}` : ''}`, note: 'layers paged out to ram', value: llm ? fmtTps(llm.decodeTps) : '—', unit: 'tok/s', sub: `${fmtGiB(dz.pagedOutGiB)} GiB paged out`, tone: 'dim' };
    if (llm?.activity === 'prefill' && llm.prefill) {
      const p = llm.prefill;
      return { label: 'PREFILL', note: 'the whole prompt in parallel', value: String(p.tokens > 0 ? Math.floor((p.doneTokens / p.tokens) * 100) : 0), unit: '%', sub: `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s${cacheHit > 0 ? ` · ${fmtInt(cacheHit)} from prompt cache` : ''}`, tone: '' };
    }
    if (llm) {
      const dil = llm.activity === 'decode' && shown > 0 ? `time ×1/${Math.max(1, Math.round(llm.decodeTps / shown))}` : '';
      return { label: 'DECODE', note: llm.activity === 'decode' ? `one token per pass · ${dil}` : 'idle', value: fmtTps(llm.decodeTps), unit: 'tok/s', sub: `${fmtInt(llm.generatedTokens)} tok generated`, tone: llm.activity === 'decode' ? '' : 'dim' };
    }
    if (img) {
      if (img.activity === 'generating' && img.steps > 0) return { label: 'DIFFUSION', note: 'one pass per step, real time', value: `${img.step}/${img.steps}`, unit: 'steps', sub: `${img.sPerIt.toFixed(2)} s/it · ${img.width}x${img.height}${img.edit ? ' · edit' : ''}`, tone: '' };
      return { label: 'DIFFUSION', note: '', value: '—', unit: 'steps', sub: 'waiting for the next job', tone: 'dim' };
    }
    return { label: 'LIVE', note: '', value: '—', unit: '', sub: 'waiting for data', tone: 'dim' };
  });

  const rows = $derived.by(() => {
    if (kind === 'image') {
      const j = img?.recent[img.recent.length - 1];
      return [
        ['last image', j ? `${fmtSeconds(j.seconds)} · ${j.width}x${j.height}` : '—'],
        ['images', img ? `${img.imagesThisSession} this session` : '—'],
        ['dit', `${arch.layers} layers`],
        ['mode', model?.mode?.toLowerCase() ?? '—'],
      ];
    }
    return [
      ['context', llm ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(llm.context.totalTokens)}` : model?.ctxTokens ? `— / ${fmtInt(model.ctxTokens)}` : '—'],
      ['layers', `${arch.layers}`],
      ['experts', arch.kind === 'moe' ? `${arch.active} + ${arch.shared} of ${arch.experts}` : 'dense'],
      ['speculative', llm?.spec ? `${Math.round(llm.spec.acceptancePct)}% · ${llm.spec.mode.toLowerCase()}` : (model?.specMode?.toLowerCase() ?? 'off')],
    ];
  });

  const modelText = $derived(
    model ? [model.name, model.quant, model.backend, model.device, model.ctxTokens ? `ctx ${Math.round(model.ctxTokens / 1024)}k` : model.imageSize, model.kvType ? `kv ${model.kvType}` : null].filter(Boolean).join(' · ').toLowerCase() : '',
  );
  const requests = $derived(llm ? llm.requests.slice(-8) : []);
  const jobs = $derived(img ? img.recent.slice(-10) : []);
  const maxJob = $derived(Math.max(1, ...jobs.map((j) => j.seconds)));
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const statusTone = $derived(phase === 'fault' ? 'red' : dz ? 'dim' : s ? 'live' : 'off');
  const statusText = $derived(dz ? 'GPU ASLEEP' : phase.toUpperCase());
  const ctxPct = $derived(llm && llm.context.totalTokens > 0 ? Math.round((llm.context.usedTokens / llm.context.totalTokens) * 100) : 0);

  let canvas: HTMLCanvasElement;
  let sceneEl: HTMLElement;
  const lab: Record<string, HTMLElement | undefined> = $state({});
  let scene: LoomScene | null = null;
  let error = $state('');

  onMount(() => {
    try {
      scene = new LoomScene(canvas);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return;
    }
    scene.setLabels(lab);
    const fit = () => {
      const r = sceneEl.getBoundingClientRect();
      scene!.resize(r.width, r.height);
    };
    const ro = new ResizeObserver(fit);
    ro.observe(sceneEl);
    fit();
    scene.setVm(player.vm);
    scene.start();
    const tick = setInterval(() => {
      shown = scene?.shownTps ?? 0;
      cacheHit = scene?.cacheHit ?? 0;
    }, 500);
    (window as unknown as { __loom: LoomScene }).__loom = scene;
    return () => {
      ro.disconnect();
      clearInterval(tick);
      scene?.stop();
    };
  });

  $effect(() => {
    const v = player.vm;
    scene?.setVm(v);
  });

  const SCEN = [
    ['idle', 'idle'],
    ['boot-medium', 'load'],
    ['prefill-high', 'prefill'],
    ['live-low', 'decode (moe)'],
    ['live-medium', 'decode (dense)'],
    ['krea', 'krea'],
    ['spill-high', 'spill'],
    ['dormant', 'asleep'],
    ['waking', 'waking'],
    ['fault-krea', 'fault'],
  ];
</script>

<div class="page">
  <div class="proto">
    <span class="tag">PROTOTYPE · LOOM</span>
    <span class="grp">{#each SCEN as [sc, label] (sc)}<a href={`?scenario=${sc}`} class:on={sc === scenario}>{label}</a>{/each}</span>
  </div>

  <div class="win">
    <header class="hdr">
      <div class="brand"><img src="/koksny-mark.png" alt="" /><span class="w">KLIF</span><span class="sl">LOOM</span></div>
      <div class="st {statusTone}"><i></i>{statusText}{#if s}<span class="sep">│</span><span class="clk">{fmtClock(s.uptimeS)}</span>{/if}</div>
    </header>

    <nav class="tiers">
      {#each vm.systems as t, i (t.id)}
        {@const running = s?.system === t.id && phase !== 'fault'}
        <button class="tier" class:sel={t.id === vm.selected} class:na={t.availability !== 'ready'} onclick={() => !s && player.actions.select(t.id)}>
          <span class="tk">F{i + 1}</span><span class="tn">{t.label}</span>{#if running}<span class="rn">■</span>{/if}
          <span class="tm">{t.model.name.toLowerCase()} · {t.model.quant.toLowerCase()}</span>
        </button>
      {/each}
    </nav>

    <div class="mline"><span class="pr">&gt;</span> {modelText}<span class="caret">▌</span></div>

    <section class="scene" bind:this={sceneEl}>
      <canvas bind:this={canvas}></canvas>
      <div class="crt"></div>
      <div class="hero {hero.tone}">
        <div class="hl">{hero.label}{#if hero.note}<span class="note">{hero.note}</span>{/if}</div>
        <div class="hv"><b><Lock value={hero.value} /></b><span class="u">{hero.unit}</span></div>
        <div class="hs">{hero.sub}</div>
      </div>
      <div class="lab" bind:this={lab.top}><span>{kind === 'image' ? 'latent patches' : 'logits · top-20 · sampled token'}</span></div>
      {#if llm?.spec}<div class="lab" bind:this={lab.draft}><span>draft · {llm.spec.mode.toLowerCase()} · {Math.round(llm.spec.acceptancePct)}% accepted</span></div>{/if}
      <div class="lab" bind:this={lab.bottom}><span>embedding · layer 1</span></div>
      <div class="lab" bind:this={lab.layer}><span>layer {Math.floor(arch.layers / 2) + 1} / {arch.layers}{arch.kind === 'moe' ? ` · ${arch.active}+${arch.shared} of ${arch.experts} experts` : arch.kind === 'dense' ? ' · dense' : ' · dit'}</span></div>
      {#if llm}<div class="lab kv" bind:this={lab.kv}><span>kv cache · {fmtInt(llm.context.usedTokens)} tok · {ctxPct}%</span></div>{/if}
      <div class="lab vr" bind:this={lab.vram}><span>vram {fmtGiB(vm.vram.usedGiB)} / {fmtGiB(vm.vram.totalGiB)}{vm.vram.spillMiB > 0 ? ' · spill' : ''}</span></div>
      {#if error}<div class="err">{error}</div>{/if}
    </section>

    <section class="rows">
      {#each rows as [k, v] (k)}<div class="kv"><span class="k">{k}</span><span class="v">{v}</span></div>{/each}
    </section>

    <section class="tline">
      <div class="cap">{kind === 'image' ? 'RECENT JOBS' : 'REQUEST TIMELINE'}<span class="dim">{s ? '' : ' · not running'}</span></div>
      {#if kind === 'image'}
        <div class="jobs">{#each jobs as j, i (i)}<span class="job" class:edit={j.edit} style="--h:{(j.seconds / maxJob) * 100}%"></span>{/each}</div>
      {:else}
        <div class="reqs">
          {#each Array.from({ length: 8 }, (_, i) => requests[i - (8 - requests.length)] ?? null) as r, i (i)}
            {#if r}
              {@const tot = r.prefillS + r.decodeS}
              <span class="req"><span class="pre" style="width:{tot > 0 ? (r.prefillS / tot) * 100 : 0}%"></span><span class="dec"></span></span>
            {:else}<span class="req empty"></span>{/if}
          {/each}
        </div>
      {/if}
    </section>

    <footer class="ctrl">
      <span class="chip">:{s?.endpoint.port ?? sel?.command?.port ?? '—'}{#if phase !== 'live'}<em> offline</em>{/if}</span>
      <span class="chip">{s?.apiKeySet ? '•••••••' : 'no key'}</span>
      <span class="grow"></span>
      {#if !s}
        <button class="btn go" onclick={() => player.actions.launch(vm.selected)}>▶ LAUNCH {sel?.label.replace('AGENT ', '')}</button>
      {:else if phase === 'fault'}
        <button class="btn hot" onclick={() => player.actions.restart()}>↻ RESTART</button>
      {:else}
        <button class="btn stop" onclick={() => player.actions.stop()}>■ {phase === 'loading' || phase === 'starting' ? 'CANCEL' : 'STOP'}</button>
      {/if}
      <button class="btn" disabled={!s}>RESTART</button>
      <button class="btn">TUNE</button>
      <button class="btn" disabled={phase !== 'live'}>{kind === 'image' ? 'WEB UI' : 'ENDPOINT'}</button>
      <button class="btn">CONSOLE</button>
    </footer>
    <div class="cline"><span class="pr">&gt;</span> {lastLine}</div>
  </div>
</div>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: #050302;
  }
  .page {
    --void: #050302;
    --panel: #0c0703;
    --rule: #2e1a06;
    --rule2: #4a2a0a;
    --amber: #ffb000;
    --hot: #ffd98a;
    --white: #fff4de;
    --ember: #ff6a1a;
    --muted: #8a5a1c;
    --red: #ff3b30;
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    font-family: 'IBM Plex Mono', Consolas, monospace;
    font-variant-numeric: tabular-nums;
    color: #e8a83c;
    background: var(--void);
    text-shadow: 0 0 6px rgba(255, 150, 0, 0.35);
  }
  .proto {
    flex: none;
    display: flex;
    gap: 18px;
    align-items: center;
    padding: 6px 14px;
    font-size: 12px;
    border-bottom: 1px dashed #3a2208;
    background: #080402;
    white-space: nowrap;
    overflow-x: auto;
    text-shadow: none;
  }
  .proto .tag {
    color: var(--ember);
    letter-spacing: 0.14em;
  }
  .proto .grp {
    display: flex;
    gap: 4px;
  }
  .proto a {
    color: #b07a30;
    text-decoration: none;
    padding: 2px 7px;
    border: 1px solid transparent;
  }
  .proto a.on {
    color: var(--white);
    border-color: var(--rule2);
    background: #1a0e03;
  }
  .win {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 0 14px 12px;
  }
  .win > * {
    flex: none;
  }
  .hdr {
    height: 54px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--rule);
    margin: 0 -14px;
    padding: 0 14px;
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }
  .brand img {
    width: 30px;
    height: 30px;
    align-self: center;
    border-radius: 6px;
    filter: sepia(1) saturate(4) hue-rotate(-12deg) brightness(0.95);
  }
  .brand .w,
  .brand .sl,
  .st,
  .tier .tn,
  .tier .tk,
  .hl,
  .hv,
  .cap,
  .kv .k,
  .btn,
  .lab {
    font-family: 'VT323', monospace;
  }
  .brand .w {
    font-size: 34px;
    letter-spacing: 0.18em;
    color: var(--white);
  }
  .brand .sl {
    font-size: 22px;
    color: var(--ember);
    letter-spacing: 0.3em;
  }
  .st {
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: 22px;
    letter-spacing: 0.14em;
    color: var(--muted);
  }
  .st i {
    width: 9px;
    height: 9px;
    background: currentColor;
    box-shadow: 0 0 10px currentColor;
  }
  .st.live {
    color: var(--amber);
  }
  .st.red {
    color: var(--red);
  }
  .st .sep {
    color: var(--rule2);
  }
  .st .clk {
    color: var(--white);
  }
  .tiers {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }
  .tier {
    all: unset;
    cursor: pointer;
    display: grid;
    grid-template-columns: auto 1fr auto;
    column-gap: 8px;
    padding: 6px 11px;
    border: 1px solid var(--rule);
    background: var(--panel);
    min-width: 0;
  }
  .tier .tk {
    color: var(--muted);
    font-size: 18px;
  }
  .tier .tn {
    font-size: 22px;
    letter-spacing: 0.08em;
    color: var(--hot);
    white-space: nowrap;
  }
  .tier .rn {
    color: var(--amber);
    font-size: 14px;
    align-self: center;
  }
  .tier .tm {
    grid-column: 1 / -1;
    font-size: 11.5px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tier.sel {
    border-color: var(--amber);
    background: linear-gradient(180deg, #2a1604, #160b02);
    box-shadow: 0 0 20px rgba(255, 160, 0, 0.15) inset;
  }
  .tier.sel .tn {
    color: var(--white);
  }
  .tier.na .tn {
    color: var(--muted);
  }
  .mline {
    height: 30px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border: 1px solid var(--rule);
    background: var(--panel);
    font-size: 13px;
    color: var(--hot);
    white-space: nowrap;
    overflow: hidden;
  }
  .pr {
    color: var(--ember);
  }
  .caret {
    color: var(--amber);
    animation: blink 1.1s steps(1) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
  .scene {
    position: relative;
    flex: 1 1 0;
    min-height: 280px;
    border: 1px solid var(--rule);
    overflow: hidden;
    background: var(--void);
  }
  .scene canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .crt {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      repeating-linear-gradient(0deg, rgba(0, 0, 0, 0.25) 0 1px, transparent 1px 3px),
      radial-gradient(ellipse at 50% 50%, transparent 60%, rgba(0, 0, 0, 0.7) 100%);
  }
  .hero {
    position: absolute;
    left: 24px;
    top: 18px;
    pointer-events: none;
  }
  .hl {
    font-size: 22px;
    letter-spacing: 0.2em;
    color: var(--amber);
  }
  .hl .note {
    margin-left: 14px;
    font-size: 18px;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .hv {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .hv b {
    font-weight: 400;
    font-size: 104px;
    line-height: 0.95;
    color: var(--white);
    text-shadow:
      0 0 14px rgba(255, 176, 0, 0.75),
      0 0 40px rgba(255, 106, 26, 0.45);
  }
  .hv .u {
    font-size: 34px;
    color: var(--amber);
  }
  .hs {
    font-size: 13px;
    color: var(--hot);
    opacity: 0.85;
  }
  .hero.dim .hv b {
    color: #6b4210;
    text-shadow: none;
  }
  .hero.red .hl,
  .hero.red .hs,
  .hero.red .hv b {
    color: var(--red);
    text-shadow: 0 0 18px rgba(255, 59, 48, 0.6);
  }
  .lab {
    position: absolute;
    left: 0;
    top: 0;
    pointer-events: none;
    font-size: 18px;
    letter-spacing: 0.06em;
    color: var(--hot);
    white-space: nowrap;
    will-change: transform;
  }
  .lab span {
    display: inline-block;
    transform: translate(10px, -50%);
    padding: 1px 6px;
    border-left: 1px solid var(--amber);
    background: rgba(5, 3, 2, 0.65);
  }
  .lab.kv span {
    color: var(--white);
    border-color: var(--white);
  }
  .lab.vr span {
    color: var(--amber);
  }
  .err {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--red);
  }
  .rows {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border: 1px solid var(--rule);
    background: var(--panel);
  }
  .kv {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 12px;
    border-right: 1px solid var(--rule);
    min-width: 0;
  }
  .kv:last-child {
    border-right: 0;
  }
  .kv .k {
    font-size: 18px;
    letter-spacing: 0.14em;
    color: var(--ember);
    text-transform: uppercase;
  }
  .kv .v {
    font-size: 13.5px;
    color: var(--white);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tline {
    height: 70px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 6px 12px;
    border: 1px solid var(--rule);
    background: var(--panel);
  }
  .cap {
    font-size: 18px;
    letter-spacing: 0.14em;
    color: var(--ember);
  }
  .cap .dim {
    color: var(--muted);
  }
  .reqs {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: 10px;
    align-items: center;
  }
  .req {
    display: flex;
    height: 14px;
    border: 1px solid var(--rule2);
  }
  .req .pre {
    background: #7a4408;
  }
  .req .dec {
    flex: 1;
    background: repeating-linear-gradient(90deg, var(--amber) 0 6px, rgba(255, 176, 0, 0.5) 6px 8px);
    box-shadow: 0 0 8px rgba(255, 176, 0, 0.5);
  }
  .req.empty {
    border-style: dashed;
    border-color: var(--rule);
  }
  .jobs {
    flex: 1;
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .job {
    flex: 1;
    height: var(--h);
    min-height: 3px;
    background: var(--amber);
  }
  .job.edit {
    background: repeating-linear-gradient(-45deg, var(--amber) 0 3px, #7a4408 3px 6px);
  }
  .ctrl {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    border: 1px solid var(--rule);
    background: var(--panel);
  }
  .grow {
    flex: 1;
  }
  .chip,
  .btn {
    all: unset;
    display: inline-flex;
    align-items: center;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--rule2);
    font-size: 13px;
    color: #d7962e;
    white-space: nowrap;
    cursor: pointer;
  }
  .btn {
    font-size: 20px;
    letter-spacing: 0.08em;
  }
  .chip {
    cursor: default;
  }
  .chip em {
    font-style: normal;
    color: var(--muted);
    margin-left: 6px;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--amber);
    color: var(--white);
  }
  .btn:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .btn.go {
    min-width: 200px;
    justify-content: center;
    background: var(--amber);
    border-color: var(--amber);
    color: #1a0d00;
    text-shadow: none;
    box-shadow: 0 0 24px rgba(255, 176, 0, 0.5);
  }
  .btn.stop,
  .btn.hot {
    min-width: 200px;
    justify-content: center;
    border-color: var(--ember);
    color: var(--ember);
  }
  .btn.hot {
    background: var(--red);
    border-color: var(--red);
    color: #1a0300;
    text-shadow: none;
  }
  .cline {
    height: 22px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
  }
</style>
