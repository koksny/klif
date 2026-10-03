<script lang="ts">
  // Decode: a prototype skin outside the app. The mock engine drives it exactly as it drives the skins, the
  // stream field (field.ts) is the main drawing, the rest is the usual fixed KLIF skeleton in terminal form.
  import { onMount } from 'svelte';
  import { player } from '../../src/lib/state/player.svelte';
  import { ui } from '../../src/lib/state/ui.svelte';
  import { fmtClock, fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../src/lib/model/format';
  import { DecodeField } from './field';
  import Lock from './Lock.svelte';

  player.init(ui.params);
  const q = new URLSearchParams(location.search);
  const scenario = q.get('scenario') ?? 'live-low';

  const vm = $derived(player.vm);
  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const dz = $derived(phase === 'live' ? (vm.vram.dormant ?? null) : null);
  const sel = $derived(vm.slots.find((x) => x.id === vm.selected));
  const runSlot = $derived(s ? vm.slots.find((x) => x.id === s.slot) : undefined);
  const model = $derived(s?.model ?? sel?.model);
  const kind = $derived(runSlot?.kind ?? sel?.kind ?? 'llm');

  type Hero = { label: string; dir: string; value: string; unit: string; sub: string; tone: '' | 'amber' | 'red' | 'dim' };
  const hero = $derived.by<Hero>(() => {
    if (!s) {
      const ls = vm.lastSession;
      return { label: 'STANDBY', dir: '', value: '—', unit: kind === 'image' ? 'steps' : 'tok/s', sub: ls ? `last ${ls.model.name.toLowerCase()} · ${fmtTps(ls.decodeTps ?? 0)} tok/s` : 'nothing running', tone: 'dim' };
    }
    if (phase === 'fault') return { label: 'FAULT', dir: '', value: '×', unit: '', sub: s.fault?.title ?? 'the server stopped', tone: 'red' };
    if (phase === 'stopping') return { label: 'STOPPING', dir: '', value: '—', unit: '', sub: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB`, tone: 'dim' };
    if (phase === 'starting' || phase === 'loading') {
      const st = s.loading?.steps.find((x) => x.state === 'active');
      return { label: 'LOAD', dir: 'the terminal boots', value: String(Math.floor((s.loading?.fraction ?? 0) * 100)), unit: '%', sub: st ? `${st.label.toLowerCase()}${st.detail ? ` · ${st.detail}` : ''}` : 'starting', tone: 'amber' };
    }
    if (dz) return { label: `GPU ASLEEP${dz.powerState ? ` · ${dz.powerState}` : ''}`, dir: '', value: llm ? fmtTps(llm.decodeTps) : '—', unit: 'tok/s', sub: `${fmtGiB(dz.pagedOutGiB)} GiB paged out to system ram`, tone: 'amber' };
    if (llm?.activity === 'prefill' && llm.prefill) {
      const p = llm.prefill;
      const pct = p.tokens > 0 ? Math.floor((p.doneTokens / p.tokens) * 100) : 0;
      return { label: 'PREFILL', dir: 'reading the prompt into context', value: String(pct), unit: '%', sub: `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s`, tone: '' };
    }
    if (llm) {
      return { label: 'DECODE', dir: llm.activity === 'decode' ? 'finding tokens in the noise' : 'idle', value: fmtTps(llm.decodeTps), unit: 'tok/s', sub: `${fmtInt(llm.generatedTokens)} tok generated`, tone: llm.activity === 'decode' ? '' : 'dim' };
    }
    if (img) {
      if (img.activity === 'generating' && img.steps > 0) return { label: 'DENOISE', dir: 'noise settling into an image', value: `${img.step}/${img.steps}`, unit: 'steps', sub: `${img.sPerIt.toFixed(2)} s/it · ${img.width}x${img.height}${img.edit ? ' · edit' : ''}`, tone: '' };
      return { label: 'DENOISE', dir: '', value: '—', unit: 'steps', sub: 'waiting for the next job', tone: 'dim' };
    }
    return { label: 'LIVE', dir: '', value: '—', unit: '', sub: 'waiting for data', tone: 'dim' };
  });

  const rows = $derived.by(() => {
    if (kind === 'image') {
      const j = img?.recent[img.recent.length - 1];
      return [
        ['last image', j ? `${fmtSeconds(j.seconds)} · ${j.width}x${j.height}${j.edit ? ' · edit' : ''}` : '—'],
        ['images', img ? `${img.imagesThisSession} this session` : '—'],
        ['size', model?.imageSize ?? '—'],
        ['mode', model?.mode?.toLowerCase() ?? '—'],
      ];
    }
    const pf = llm?.prefill;
    return [
      ['prefill', pf ? `${fmtInt(pf.tokens)} tok · ${fmtInt(pf.tps)} tok/s` : '—'],
      ['decode', llm ? `${fmtInt(llm.generatedTokens)} tok generated` : '—'],
      ['context', llm ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(llm.context.totalTokens)}` : model?.ctxTokens ? `— / ${fmtInt(model.ctxTokens)}` : '—'],
      ['speculative', llm?.spec ? `${Math.round(llm.spec.acceptancePct)}% · ${llm.spec.mode.toLowerCase()}` : (model?.specMode?.toLowerCase() ?? 'off')],
    ];
  });

  const modelText = $derived(
    model ? [model.name, model.quant, model.backend, model.device, model.ctxTokens ? `ctx ${Math.round(model.ctxTokens / 1024)}k` : model.imageSize, model.kvType ? `kv ${model.kvType}` : null, model.mode].filter(Boolean).join(' · ').toLowerCase() : '',
  );
  const requests = $derived(llm ? llm.requests.slice(-8) : []);
  const jobs = $derived(img ? img.recent.slice(-10) : []);
  const maxJob = $derived(Math.max(1, ...jobs.map((j) => j.seconds)));
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const clock = $derived(s ? fmtClock(s.uptimeS) : '');
  const statusTone = $derived(phase === 'fault' ? 'red' : dz || phase === 'loading' || phase === 'starting' ? 'amber' : s ? 'live' : 'off');
  const statusText = $derived(dz ? 'GPU ASLEEP' : phase.toUpperCase());

  let canvas: HTMLCanvasElement;
  let fieldEl: HTMLElement;
  let heroEl: HTMLElement;
  let field: DecodeField | null = null;
  let error = $state('');

  onMount(() => {
    let off = () => {};
    (async () => {
      await document.fonts.load('500 56px "Iosevka"').catch(() => undefined);
      try {
        field = new DecodeField(canvas, { margin: 0.27 });
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
        return;
      }
      const fit = () => {
        const r = fieldEl.getBoundingClientRect();
        field!.resize(r.width, r.height);
        field!.setHero(heroEl.getBoundingClientRect(), r);
      };
      const ro = new ResizeObserver(fit);
      ro.observe(fieldEl);
      fit();
      field.setVm(player.vm);
      field.start();
      (window as unknown as { __field: DecodeField }).__field = field;
      off = () => {
        ro.disconnect();
        field?.stop();
      };
    })();
    return () => off();
  });

  $effect(() => {
    const v = player.vm;
    field?.setVm(v);
    if (field && heroEl && fieldEl) field.setHero(heroEl.getBoundingClientRect(), fieldEl.getBoundingClientRect());
  });

  const SCEN = [
    ['idle', 'idle'],
    ['boot-medium', 'load'],
    ['prefill-high', 'prefill'],
    ['live-low', 'decode'],
    ['krea', 'krea'],
    ['spill-high', 'spill'],
    ['dormant', 'asleep'],
    ['waking', 'waking'],
    ['fault-krea', 'fault'],
  ];
  const href = (sc: string) => `?scenario=${sc}`;
</script>

<div class="page">
  <div class="proto">
    <span class="tag">PROTOTYPE · DECODE</span>
    <span class="grp">
      {#each SCEN as [sc, label] (sc)}<a href={href(sc)} class:on={sc === scenario}>{label}</a>{/each}
    </span>
    <span class="grp legend"><i class="lg n"></i>noise = free context <i class="lg p"></i>prompt <i class="lg w"></i>written</span>
  </div>

  <div class="win">
    <header class="hdr">
      <div class="brand"><img src="/koksny-mark.png" alt="" /><span class="w">KLIF</span><span class="sl">//decode</span></div>
      <div class="st {statusTone}">
        <i></i>{statusText}{#if clock}<span class="sep">│</span><span class="clk">{clock}</span>{/if}
      </div>
    </header>

    <nav class="tiers">
      {#each vm.slots as t, i (t.id)}
        {@const running = s?.slot === t.id && phase !== 'fault'}
        <button class="tier" class:sel={t.id === vm.selected} class:run={running} class:na={t.availability !== 'ready'} onclick={() => !s && player.actions.select(t.id)}>
          <span class="tk">[{i + 1}]</span><span class="tl">{t.label}</span>{#if running}<span class="rn">▮</span>{/if}
          <span class="tm">{t.model.name.toLowerCase()} · {t.model.quant.toLowerCase()}</span>
        </button>
      {/each}
    </nav>

    <div class="mline"><span class="pr">&gt;</span> {modelText}<span class="caret">█</span></div>

    <section class="field" bind:this={fieldEl}>
      <canvas bind:this={canvas}></canvas>
      <div class="scan"></div>
      <div class="hero {hero.tone}" bind:this={heroEl}>
        <div class="hl">{hero.label}{#if hero.dir}<span class="dir">{hero.dir}</span>{/if}</div>
        <div class="hv"><b><Lock value={hero.value} /></b><span class="u">{hero.unit}</span></div>
        <div class="hs"><Lock value={hero.sub} ms={420} /></div>
      </div>
      <div class="mhead">
        vram <b><Lock value={`${fmtGiB(vm.vram.usedGiB)}`} /></b> / {fmtGiB(vm.vram.totalGiB)} gib{#if vm.vram.spillMiB > 0}<span class="red"> · spill</span>{/if}
      </div>
      {#if error}<div class="err">{error}</div>{/if}
    </section>

    <section class="rows">
      {#each rows as [k, v] (k)}<div class="kv"><span class="k">{k}</span><span class="v"><Lock value={v} ms={300} /></span></div>{/each}
    </section>

    <section class="tline">
      <div class="cap">{kind === 'image' ? 'recent jobs' : 'request timeline'}<span class="dim">{s ? '' : ' · not running'}</span></div>
      {#if kind === 'image'}
        <div class="jobs">
          {#each jobs as j, i (i)}<span class="job" class:edit={j.edit} style="--h:{(j.seconds / maxJob) * 100}%"><em>{j.seconds.toFixed(0)}s</em></span>{/each}
        </div>
      {:else}
        <div class="reqs">
          {#each Array.from({ length: 8 }, (_, i) => requests[i - (8 - requests.length)] ?? null) as r, i (i)}
            {#if r}
              {@const tot = r.prefillS + r.decodeS}
              <span class="req" title="#{r.id}"><span class="pre" style="width:{tot > 0 ? (r.prefillS / tot) * 100 : 0}%"></span><span class="dec"></span></span>
            {:else}<span class="req empty"></span>{/if}
          {/each}
        </div>
      {/if}
    </section>

    <footer class="ctrl">
      <span class="chip">:{s?.endpoint.port ?? sel?.recipe?.port ?? '—'}{#if phase !== 'live'}<em> offline</em>{/if}</span>
      <span class="chip">{s?.apiKeySet ? '•••••••' : 'no key'}</span>
      <span class="grow"></span>
      {#if !s}
        <button class="btn go" onclick={() => player.actions.launch(vm.selected)}>▶ launch {sel?.label.toLowerCase().replace('agent ', '')}</button>
      {:else if phase === 'fault'}
        <button class="btn hot" onclick={() => player.actions.restart()}>↻ restart</button>
      {:else}
        <button class="btn stop" onclick={() => player.actions.stop()}>■ {phase === 'loading' || phase === 'starting' ? 'cancel' : 'stop'}</button>
      {/if}
      <button class="btn" disabled={!s}>↻ restart</button>
      <button class="btn">≡ tune</button>
      <button class="btn" disabled={phase !== 'live'}>↗ {kind === 'image' ? 'web ui' : 'endpoint'}</button>
      <button class="btn">&gt;_ console</button>
    </footer>
    <div class="cline"><span class="pr">&gt;</span> {lastLine}</div>
  </div>
</div>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: #010409;
  }
  .page {
    --void: #010409;
    --abyss: #030b16;
    --rule: #0b2545;
    --rule2: #12345e;
    --stream: #2e8bff;
    --arc: #5cc8ff;
    --ice: #e9f7ff;
    --muted: #4d6f93;
    --amber: #ffb547;
    --red: #ff4d6d;
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    font-family: 'Iosevka', 'JetBrains Mono', Consolas, monospace;
    font-variant-numeric: tabular-nums;
    color: #b9d3ea;
    background: var(--void);
  }
  .proto {
    flex: none;
    display: flex;
    align-items: center;
    gap: 22px;
    padding: 6px 14px;
    font-size: 12px;
    color: #5d7a99;
    border-bottom: 1px dashed #1a2c44;
    background: #02070e;
    white-space: nowrap;
    overflow-x: auto;
  }
  .proto .tag {
    color: var(--amber);
    letter-spacing: 0.14em;
  }
  .proto .grp {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .proto a {
    color: #7d9cbd;
    text-decoration: none;
    padding: 2px 7px;
    border: 1px solid transparent;
  }
  .proto .legend {
    gap: 6px;
    color: #6d8fb3;
  }
  .lg {
    display: inline-block;
    width: 9px;
    height: 9px;
    margin-left: 10px;
  }
  .lg.n {
    background: #1c4d99;
  }
  .lg.p {
    background: #338ad9;
  }
  .lg.w {
    background: #8ccbff;
  }
  .proto a.on {
    color: var(--ice);
    border-color: var(--rule2);
    background: #061a33;
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

  /* header */
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
    gap: 10px;
  }
  .brand img {
    width: 30px;
    height: 30px;
    align-self: center;
    border-radius: 6px;
    border: 1px solid var(--rule2);
  }
  .brand .w {
    font-size: 24px;
    font-weight: 700;
    letter-spacing: 0.2em;
    color: var(--ice);
  }
  .brand .sl {
    font-size: 15px;
    color: var(--stream);
    letter-spacing: 0.06em;
  }
  .st {
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: 14px;
    letter-spacing: 0.16em;
    color: var(--muted);
  }
  .st i {
    width: 8px;
    height: 8px;
    background: currentColor;
    box-shadow: 0 0 10px currentColor;
  }
  .st.live {
    color: var(--arc);
  }
  .st.amber {
    color: var(--amber);
  }
  .st.red {
    color: var(--red);
  }
  .st .sep {
    color: var(--rule2);
  }
  .st .clk {
    color: var(--ice);
    letter-spacing: 0.06em;
  }

  /* tiers */
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
    row-gap: 3px;
    padding: 7px 11px;
    border: 1px solid var(--rule);
    background: var(--abyss);
    min-width: 0;
  }
  .tier .tk {
    color: var(--muted);
    font-size: 13px;
  }
  .tier .tl {
    color: #cfe3f5;
    font-size: 14px;
    font-weight: 500;
    letter-spacing: 0.1em;
    white-space: nowrap;
  }
  .tier .rn {
    color: var(--arc);
    animation: blink 1s steps(1) infinite;
  }
  .tier .tm {
    grid-column: 1 / -1;
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tier.sel {
    border-color: var(--stream);
    background: linear-gradient(180deg, #06203f, #041428);
    box-shadow: 0 0 18px rgba(46, 139, 255, 0.18) inset;
  }
  .tier.sel .tl {
    color: var(--ice);
  }
  .tier.na .tl {
    color: var(--muted);
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  .mline {
    height: 30px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border: 1px solid var(--rule);
    background: var(--abyss);
    font-size: 13.5px;
    color: #cfe3f5;
    white-space: nowrap;
    overflow: hidden;
  }
  .pr {
    color: var(--stream);
  }
  .caret {
    color: var(--arc);
    animation: blink 1.1s steps(1) infinite;
    margin-left: -4px;
  }

  /* field */
  .field {
    position: relative;
    flex: 1 1 0;
    min-height: 260px;
    border: 1px solid var(--rule);
    overflow: hidden;
    background: #01040a;
  }
  .field canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .scan {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      repeating-linear-gradient(0deg, rgba(0, 0, 0, 0.22) 0 1px, transparent 1px 3px),
      radial-gradient(ellipse at 40% 45%, transparent 55%, rgba(0, 2, 6, 0.65) 100%);
    mix-blend-mode: multiply;
  }
  .hero {
    position: absolute;
    left: 26px;
    top: 22px;
    padding: 14px 22px 16px 18px;
    background: radial-gradient(ellipse at 30% 50%, rgba(1, 4, 10, 0.92) 40%, rgba(1, 4, 10, 0) 100%);
    min-width: 320px;
  }
  .hl {
    font-size: 13px;
    letter-spacing: 0.24em;
    color: var(--arc);
  }
  .hl .dir {
    margin-left: 14px;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .hv {
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-top: 2px;
  }
  .hv b {
    font-size: 86px;
    font-weight: 300;
    line-height: 1;
    color: var(--ice);
    text-shadow:
      0 0 18px rgba(92, 200, 255, 0.45),
      0 0 46px rgba(46, 139, 255, 0.28);
  }
  .hv .u {
    font-size: 24px;
    color: #7fa6cc;
    letter-spacing: 0.06em;
  }
  .hs {
    margin-top: 6px;
    font-size: 14px;
    color: #8fb3d6;
  }
  .hero.dim .hv b {
    color: #29486b;
    text-shadow: none;
  }
  .hero.amber .hl,
  .hero.amber .hs {
    color: var(--amber);
  }
  .hero.amber .hv b {
    color: #ffd9a1;
    text-shadow: 0 0 24px rgba(255, 181, 71, 0.35);
  }
  .hero.red .hl,
  .hero.red .hs {
    color: var(--red);
  }
  .hero.red .hv b {
    color: var(--red);
    text-shadow: 0 0 24px rgba(255, 77, 109, 0.4);
  }
  .mhead {
    position: absolute;
    right: 14px;
    top: 10px;
    font-size: 12.5px;
    letter-spacing: 0.08em;
    color: var(--muted);
    background: rgba(1, 4, 10, 0.85);
    padding: 3px 8px;
  }
  .mhead b {
    color: var(--ice);
    font-weight: 500;
  }
  .red {
    color: var(--red);
  }
  .err {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--red);
  }

  /* rows */
  .rows {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border: 1px solid var(--rule);
    background: var(--abyss);
  }
  .kv {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 12px;
    border-right: 1px solid var(--rule);
    min-width: 0;
  }
  .kv:last-child {
    border-right: 0;
  }
  .kv .k {
    font-size: 11.5px;
    letter-spacing: 0.2em;
    color: var(--stream);
    text-transform: uppercase;
  }
  .kv .v {
    font-size: 14px;
    color: var(--ice);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* timeline */
  .tline {
    height: 74px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 12px;
    border: 1px solid var(--rule);
    background: var(--abyss);
  }
  .cap {
    font-size: 11.5px;
    letter-spacing: 0.2em;
    color: var(--stream);
    text-transform: uppercase;
  }
  .cap .dim {
    color: var(--muted);
    letter-spacing: 0.06em;
    text-transform: none;
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
    height: 16px;
    border: 1px solid var(--rule2);
    overflow: hidden;
  }
  .req .pre {
    background: #164a8c;
  }
  .req .dec {
    flex: 1;
    background: repeating-linear-gradient(90deg, var(--arc) 0 6px, rgba(92, 200, 255, 0.55) 6px 8px);
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
    position: relative;
    flex: 1;
    height: var(--h);
    min-height: 3px;
    background: var(--arc);
  }
  .job.edit {
    background: repeating-linear-gradient(-45deg, var(--arc) 0 3px, #1d5fa8 3px 6px);
  }
  .job em {
    position: absolute;
    top: -15px;
    left: 0;
    right: 0;
    text-align: center;
    font-style: normal;
    font-size: 10.5px;
    color: var(--muted);
  }

  /* controls */
  .ctrl {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--rule);
    background: var(--abyss);
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
    font-size: 13.5px;
    color: #a9c7e3;
    white-space: nowrap;
    cursor: pointer;
  }
  .chip {
    cursor: default;
    color: #7fa6cc;
  }
  .chip em {
    font-style: normal;
    color: var(--muted);
    margin-left: 6px;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--stream);
    color: var(--ice);
  }
  .btn:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .btn.go {
    min-width: 200px;
    justify-content: center;
    background: var(--stream);
    border-color: var(--stream);
    color: #010915;
    font-weight: 700;
    letter-spacing: 0.08em;
    box-shadow: 0 0 22px rgba(46, 139, 255, 0.45);
  }
  .btn.stop,
  .btn.hot {
    min-width: 200px;
    justify-content: center;
    border-color: var(--red);
    color: var(--red);
    letter-spacing: 0.08em;
  }
  .btn.hot {
    background: var(--red);
    color: #12030a;
  }
  .cline {
    height: 22px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: #6d8fb3;
    white-space: nowrap;
    overflow: hidden;
  }
</style>
