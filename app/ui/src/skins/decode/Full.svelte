<script lang="ts">
  // Decode, full window. The fixed KLIF skeleton in terminal form, scaled by --u (1 design px), the same in
  // every phase (only the contents change): header, tier strip, model line, the terminal field (absorbs the
  // remaining height; the hero sits inside it top left, the VRAM margin header top right), detail rows,
  // request timeline / recent jobs, controls, console line.
  //   idle      - standby hero over the churning noise, the selected tier's configured values, Launch
  //   loading   - the terminal prints in, startup % and steps in the hero, other tiers locked
  //   live-llm  - the context fill as dim text, decode finds tokens at the tok/s rate, prefill sweeps
  //   live-img  - the noise settles into an ASCII picture step by step, recent jobs
  //   dormant   - (live, vram.dormant set) frozen amber; waking: it thaws as the VRAM comes back
  //   fault     - red glitch, the context dissolves, the hero shows the fault and the log tail
  // Controls keep their places: the primary button (Launch / Cancel / Stop / Restart after a fault), Restart
  // (Dismiss after a fault), Tune (always), Endpoint / Web UI, Console (Full log after a fault).
  import type { RequestRecord, Slot, ViewModel, Actions, LoadStep } from '../../lib/model/types';
  import { fmtClock, fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../lib/model/format';
  import Field from './Field.svelte';
  import Lock from './Lock.svelte';
  import WinCtl from './WinCtl.svelte';
  import { useSleep } from './sleep.svelte';
  import { availabilityText, baselineOf, fmtAgo, fmtDur, fmtEta, modelLine, phaseWord, tierWord } from './text';

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

  // GPU dormant (live session, vram.dormant set): asleep between requests, or waking while the VRAM is restored.
  const sleep = useSleep(() => vm);
  const dz = $derived(phase === 'live' ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);

  const prefillActive = $derived(!!llm?.prefill && llm.activity === 'prefill');
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const exitText = $derived.by(() => {
    if (!fault) return '';
    if (fault.exitCode === undefined) return 'no exit code';
    return fault.exitCodeHex ? `exit ${fault.exitCodeHex} (${fault.exitCode})` : `exit ${fault.exitCode}`;
  });

  // ---- hero: label (+ what the terminal is doing), one figure that decodes on change, a status line --------
  type Hero = {
    label: string;
    dir: string;
    value: string;
    unit: string;
    sub: string;
    tone: '' | 'amber' | 'red' | 'dim';
    subTone?: 'amber';
    steps?: LoadStep[];
    log?: string[];
  };
  const hero = $derived.by<Hero>(() => {
    if (!s) {
      const ls = vm.lastSession;
      const unit = kind === 'image' ? 'steps' : 'tok/s';
      if (!selReady) return { label: 'STANDBY', dir: '', value: '—', unit, sub: `${tierWord(sel?.label ?? '')} · ${sel?.reason ?? availabilityText(sel?.availability ?? 'unsupported')}`.toLowerCase(), tone: 'dim', subTone: 'amber' };
      if (!ls) return { label: 'STANDBY', dir: 'pick a tier and launch', value: '—', unit, sub: 'nothing running', tone: 'dim' };
      const speed = ls.decodeTps !== undefined ? ` · ${fmtTps(ls.decodeTps)} tok/s` : ls.secondsPerImage !== undefined ? ` · ${ls.secondsPerImage.toFixed(1)} s/image` : '';
      const end = ls.ended === 'fault' ? `fault ${fmtAgo(ls.endedAgoS)}` : `stopped ${fmtAgo(ls.endedAgoS)}`;
      return { label: 'STANDBY', dir: 'pick a tier and launch', value: '—', unit, sub: `last ${ls.model.name.toLowerCase()}${speed} · ${end}`, tone: 'dim' };
    }
    if (faulted) {
      const tail = (fault?.logTail ?? []).slice(-5);
      return { label: 'FAULT', dir: `${fault ? fmtAgo(fault.sinceS) : ''}${exitText ? ` · ${exitText}` : ''}`, value: '×', unit: '', sub: fault?.title ?? 'the server stopped', tone: 'red', steps: fault?.steps, log: tail };
    }
    if (phase === 'stopping') return { label: 'STOPPING', dir: 'releasing the context', value: '—', unit: '', sub: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB`, tone: 'dim' };
    if (loading) {
      const st = s.loading?.steps.find((x) => x.state === 'active');
      return {
        label: 'LOAD',
        dir: 'the terminal boots',
        value: String(Math.floor((s.loading?.fraction ?? 0) * 100)),
        unit: '%',
        sub: st ? `${st.label.toLowerCase()}${st.detail ? ` · ${st.detail}` : ''}` : phase,
        tone: 'amber',
        steps: s.loading?.steps,
      };
    }
    if (dz && waking) {
      return {
        label: `GPU WAKING${dz.powerState ? ` · ${dz.powerState}` : ''}`,
        dir: prefillActive ? 'a request waits for the gpu' : 'the terminal thaws',
        value: String(Math.round(dz.restoredFrac * 100)),
        unit: '%',
        sub: `${fmtGiB(dz.pagedOutGiB)} GiB still in system ram`,
        tone: 'amber',
      };
    }
    if (dz) {
      return {
        label: `GPU ASLEEP${dz.powerState ? ` · ${dz.powerState}` : ''}`,
        dir: `frozen for ${fmtDur(dz.sinceS)}`,
        value: llm ? fmtTps(llm.decodeTps) : '—',
        unit: llm ? 'tok/s' : '',
        sub: `${fmtGiB(dz.pagedOutGiB)} GiB paged out to system ram`,
        tone: 'amber',
      };
    }
    if (llm && prefillActive && llm.prefill) {
      const p = llm.prefill;
      const pct = p.tokens > 0 ? Math.floor((p.doneTokens / p.tokens) * 100) : 0;
      return { label: 'PREFILL', dir: 'reading the prompt into context', value: String(pct), unit: '%', sub: `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s · ${fmtEta(p.etaS)} left`, tone: '' };
    }
    if (llm) {
      const on = llm.activity === 'decode';
      return { label: 'DECODE', dir: on ? 'finding tokens in the noise' : 'idle', value: fmtTps(llm.decodeTps), unit: 'tok/s', sub: `${fmtInt(llm.generatedTokens)} tok generated`, tone: on ? '' : 'dim' };
    }
    if (img) {
      if (img.activity === 'generating' && img.steps > 0)
        return {
          label: `DENOISE${img.edit ? ' · EDIT' : ''}`,
          dir: 'noise settling into an image',
          value: `${img.step}/${img.steps}`,
          unit: 'steps',
          sub: `${img.sPerIt.toFixed(2)} s/it · ${img.width}x${img.height} · ${fmtSeconds(img.elapsedS)}`,
          tone: '',
        };
      return { label: 'DENOISE', dir: 'idle', value: '—', unit: 'steps', sub: 'waiting for the next job', tone: 'dim' };
    }
    return { label: 'LIVE', dir: '', value: '—', unit: '', sub: 'waiting for data', tone: 'dim' };
  });
  const stepMark = (st: LoadStep) => (st.state === 'done' ? '✓' : st.state === 'active' ? '▸' : st.state === 'failed' ? '×' : '·');

  // ---- detail rows: four fixed facts per kind, the same labels in every phase ---------------------------------
  const rows = $derived.by<[string, string][]>(() => {
    if (kind === 'image') {
      const j = img?.recent[img.recent.length - 1];
      return [
        ['last image', j ? `${fmtSeconds(j.seconds)} · ${j.width}x${j.height}${j.edit ? ' · edit' : ''}` : img ? 'none yet' : '—'],
        ['images', img ? `${fmtInt(img.imagesThisSession)} this session` : '—'],
        ['size', model?.imageSize ?? '—'],
        ['mode', model?.mode?.toLowerCase() ?? '—'],
      ];
    }
    const pf = llm?.prefill;
    const total = llm?.context.totalTokens || model?.ctxTokens || 0;
    return [
      ['prefill', pf ? (prefillActive ? `${pf.cachedTokens ? `${fmtInt(pf.cachedTokens)} cached · ` : ''}running ${fmtSeconds(pf.elapsedS)}` : `${fmtInt(pf.tokens)} tok · ${fmtInt(pf.tps)} tok/s`) : llm ? 'no request yet' : '—'],
      ['decode', llm ? (prefillActive ? 'waiting for prefill' : `${fmtInt(llm.generatedTokens)} tok generated`) : '—'],
      ['context', llm ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(total)} · ${Math.round((llm.context.usedTokens / Math.max(1, total)) * 100)}%` : total ? `— / ${fmtInt(total)}` : '—'],
      ['speculative', llm?.spec ? `${Math.round(llm.spec.acceptancePct)}% · ${llm.spec.mode.toLowerCase()}` : llm ? 'off' : (model?.specMode?.toLowerCase() ?? 'off')],
    ];
  });

  // ---- request timeline (8 places; a fault marks the last) / recent jobs (12 places) ---------------------------
  const reqN = $derived(faulted && llm ? 7 : 8);
  const requests = $derived(llm ? llm.requests.slice(-reqN) : []);
  const reqSlots = $derived(Array.from({ length: reqN }, (_, i) => requests[i - (reqN - requests.length)] ?? null));
  const jobN = $derived(faulted && img ? 11 : 12);
  const jobs = $derived(img ? img.recent.slice(-jobN) : []);
  const jobSlots = $derived(Array.from({ length: jobN }, (_, i) => jobs[i - (jobN - jobs.length)] ?? null));
  const maxJob = $derived(Math.max(1, ...jobs.map((j) => j.seconds)));
  const diedInWork = $derived(faulted && ((!!llm && llm.activity !== 'idle') || (!!img && img.activity === 'generating')));
  const tlCaption = $derived.by(() => {
    if (!s) return ' · not running';
    if (loading) return kind === 'image' ? ' · no jobs yet' : ' · no requests yet';
    const n = kind === 'image' ? jobs.length : requests.length;
    const what = kind === 'image' ? 'jobs' : 'requests';
    if (faulted) {
      if (!llm && !img) return ` · no ${what}: failed during startup`;
      return ` · ${n ? `last ${n}` : `no finished ${what}`}, then ${diedInWork ? `the ${kind === 'image' ? 'job' : 'request'} that died` : 'the fault'}`;
    }
    if (n === 0) return ` · no finished ${what} yet`;
    return kind === 'image' ? ` · bar height = time, last ${n} of ${fmtInt(img?.imagesThisSession ?? n)}` : ` · last ${n}, prefill dark, decode light`;
  });
  const reqTitle = (r: RequestRecord) =>
    `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${fmtSeconds(r.prefillS)} · ${fmtInt(r.generatedTokens)} tok in ${fmtSeconds(r.decodeS)}`;

  // ---- header, model line, VRAM margin header, console line ----------------------------------------------------
  const statusText = $derived(dz ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : phaseWord(phase));
  const statusTone = $derived(faulted ? 'red' : dz || loading ? 'amber' : s ? 'live' : 'off');
  const modelText = $derived(modelLine(model, kind));
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const preview = $derived(!s && sel?.expectedVram?.length ? sel.expectedVram.reduce((a, l) => a + l.gib, 0) : null);
  const previewTop = $derived(preview !== null ? Math.max(baselineOf(vm.vram), vm.vram.usedGiB) + preview : 0);
  const lowFree = $derived(!!s && vm.vram.totalGiB - vm.vram.usedGiB < vm.vram.warnBelowGiB);
  const fieldLabel = $derived.by(() => {
    const ctx = llm && llm.context.totalTokens > 0 ? `${Math.round((llm.context.usedTokens / llm.context.totalTokens) * 100)}% of the context in use` : 'no context in use';
    return `Context window as a terminal: ${ctx}. Right margin: VRAM ${fmtGiB(vm.vram.usedGiB)} of ${fmtGiB(vm.vram.totalGiB)} GiB.`;
  });

  let heroEl = $state<HTMLElement | null>(null);
</script>

<div class="full" bind:clientWidth={w} bind:clientHeight={h} style="--u:{u}px">
  <!-- Header (window drag region; draws its own window controls when the host is frameless) -->
  <header class="hdr" class:frameless data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region title="Koksny.com LOCAL INFERENCE FORNICATOR">
      <img src="/koksny-mark.png" alt="" draggable="false" /><span class="wm">KLIF</span><span class="sl">//decode</span>
    </div>
    <span class="grow" data-tauri-drag-region></span>
    <div class="st {statusTone}" data-tauri-drag-region>
      <i class:pulse={waking || loading}></i>{statusText}
      {#if s}
        <span class="sep">│</span>
        {#if loading}<span class="ck"><em>elapsed</em> {fmtClock(s.loading?.elapsedS ?? s.uptimeS)}</span>{:else}<span class="ck"><em>up</em> {fmtClock(s.uptimeS)}</span>{/if}
      {:else if vm.host?.appVersion}
        <span class="sep">│</span><span class="ck ver">v{vm.host.appVersion}</span>
      {/if}
    </div>
    {#if panel}
      <button class="pbtn" class:on={panel.active} onclick={() => actions.togglePanel?.()} title={panelTip} aria-label={panelTip} aria-pressed={panel.active}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="2.5" y="4" width="19" height="12.5" /><path d="M8.5 20.5h7M12 16.5v4" /><path d="M14 9.5h5v4h-5z" /></svg>
        <span class="pl">panel</span>
      </button>
    {/if}
    {#if frameless}<WinCtl {actions} maximized={!!vm.host?.maximized} />{/if}
  </header>

  <!-- Tier strip: select; double-click launches while nothing runs or after a fault; locked while starting/stopping -->
  <div class="tiers" role="tablist" aria-label="Tier">
    {#each vm.slots as t, i (t.id)}
      {@const selected = t.id === vm.selected}
      {@const locked = busy && t.id !== s?.slot}
      {@const na = t.availability !== 'ready'}
      {@const running = !!s && s.slot === t.id && !faulted}
      <button
        class="tier"
        class:sel={selected}
        class:na
        class:locked
        class:fault={faulted && s?.slot === t.id}
        role="tab"
        aria-selected={selected}
        disabled={locked}
        title={locked ? `${t.label}: locked while ${s?.model.name ?? 'the session'} is ${phase}` : na ? `${t.label}: ${t.reason ?? availabilityText(t.availability)}` : `${t.label}: ${t.model.name}`}
        onclick={() => actions.select(t.id)}
        ondblclick={() => {
          if ((!s || faulted) && !na) actions.launch(t.id);
        }}
      >
        <span class="tk">[{i + 1}]</span><span class="tl">{t.label}</span>
        {#if running}<span class="rn" class:amb={loading || !!dz}>▮</span>{:else if locked}<span class="lk">locked</span>{:else if na}<span class="nb">!</span>{/if}
        <span class="tm">{na ? `${t.model.name} · ${availabilityText(t.availability)}`.toLowerCase() : `${t.model.name} · ${t.model.quant}`.toLowerCase()}</span>
      </button>
    {/each}
  </div>

  <div class="mline" class:dim={!s}><span class="pr">&gt;</span><span class="mt">{modelText}</span><span class="caret">█</span></div>

  <!-- The terminal: noise = free context, dim text = context in use, the right margin = VRAM -->
  <section class="field">
    <Field {vm} fontPx={Math.max(11, u * 12.7)} margin={0.27} hero={heroEl} label={fieldLabel} />
    <div class="scan"></div>
    <div class="hero {hero.tone}" bind:this={heroEl}>
      <div class="hl">{hero.label}{#if hero.dir}<span class="dir">{hero.dir}</span>{/if}</div>
      <div class="hv"><b><Lock value={hero.value} /></b>{#if hero.unit}<span class="u">{hero.unit}</span>{/if}</div>
      <div class="hs" class:amb={hero.subTone === 'amber'}><Lock value={hero.sub} ms={420} /></div>
      {#if hero.steps?.length}
        <div class="hsteps">
          {#each hero.steps as st (st.id)}<span class={st.state}>{stepMark(st)} {st.label.toLowerCase()}</span>{/each}
        </div>
      {/if}
      {#if hero.log?.length}
        <div class="hlog">{#each hero.log as line, i (i)}<div title={line}>{line}</div>{/each}</div>
      {/if}
    </div>
    <div class="mhead">
      {#if dz}
        <span class="amb">resident</span> <b><Lock value={fmtGiB(vm.vram.usedGiB)} /></b> / {fmtGiB(vm.vram.totalGiB)} gib
      {:else}
        vram <b class:amb={lowFree && vm.vram.spillMiB <= 0} class:red={vm.vram.spillMiB > 0}><Lock value={fmtGiB(vm.vram.usedGiB)} /></b> / {fmtGiB(vm.vram.totalGiB)} gib
        {#if vm.vram.spillMiB > 0}<span class="red"> · spill</span>{:else if preview !== null}<span class:red={previewTop > vm.vram.totalGiB}> · fit {fmtGiB(previewTop)}</span>{/if}
      {/if}
    </div>
  </section>

  <section class="rows" class:dim={!s}>
    {#each rows as [k, v] (k)}<div class="kv"><span class="k">{k}</span><span class="v"><Lock value={v} ms={300} /></span></div>{/each}
  </section>

  <section class="tline">
    <div class="cap">{kind === 'image' ? 'recent jobs' : 'request timeline'}<span class="dim">{tlCaption}</span></div>
    {#if kind === 'image'}
      <div class="jobs">
        {#each jobSlots as j, i (i)}
          {#if j}
            <span class="job" class:edit={j.edit} style="--h:{(j.seconds / maxJob) * 100}%" title="{fmtSeconds(j.seconds)} · {j.width}x{j.height}{j.edit ? ' · edit' : ''}"><em>{j.seconds.toFixed(0)}s</em></span>
          {:else}<span class="job empty"></span>{/if}
        {/each}
        {#if faulted && img}<span class="job failed" title={diedInWork ? 'The server died during this job' : 'The server died between jobs'}></span>{/if}
      </div>
    {:else}
      <div class="reqs">
        {#each reqSlots as r, i (i)}
          {#if r}
            {@const tot = r.prefillS + r.decodeS}
            <span class="req" class:cur={i === reqN - 1 && !faulted} title={reqTitle(r)}><span class="pre" style="width:{tot > 0 ? (r.prefillS / tot) * 100 : 0}%"></span><span class="dec"></span></span>
          {:else}<span class="req empty"></span>{/if}
        {/each}
        {#if faulted && llm}<span class="req failed" title={diedInWork ? 'The server died during this request' : 'The server died with no request in flight'}></span>{/if}
      </div>
    {/if}
  </section>

  <footer class="ctrl">
    <button class="chip" onclick={() => actions.copyEndpoint()} disabled={!online} title={online && s ? `Copy http://${s.endpoint.host}:${s.endpoint.port}` : 'Endpoint offline'}>
      :{s?.endpoint.port ?? sel?.recipe?.port ?? '—'}{#if !online}<em>offline</em>{/if}
    </button>
    <button class="chip" onclick={() => actions.copyApiKey()} disabled={!online || !s?.apiKeySet} title={s?.apiKeySet ? 'Copy API key' : 'No API key set'}>
      {#if s?.apiKeySet}•••••••<em>copy</em>{:else}no key{/if}
    </button>
    <span class="grow"></span>
    {#if !s}
      <button class="btn act go" onclick={() => actions.launch(vm.selected)} disabled={!selReady} title={selReady ? `Launch ${sel?.label ?? ''}` : `${sel?.label ?? ''}: ${sel?.reason ?? availabilityText(sel?.availability ?? 'unsupported')}`}>
        ▶ launch {tierWord(sel?.label ?? '')}
      </button>
    {:else if faulted}
      <button class="btn act hot" onclick={() => actions.restart()} title="Launch {sessionSlot?.label ?? ''} again">↻ restart {tierWord(sessionSlot?.label ?? '')}</button>
    {:else}
      <button class="btn act stop" onclick={() => actions.stop()} disabled={phase === 'stopping'}>■ {loading ? 'cancel' : phase === 'stopping' ? 'stopping' : 'stop'}</button>
    {/if}
    {#if faulted}
      <button class="btn" onclick={() => actions.dismiss?.()} title="Back to the launcher">← dismiss</button>
    {:else}
      <button class="btn" onclick={() => actions.restart()} disabled={!s || busy} title="Stop and launch again with the current settings">↻ restart</button>
    {/if}
    <button class="btn" onclick={() => actions.openTune(vm.selected)} title={s && s.slot === vm.selected && !faulted ? 'Change the settings; Restart to apply them' : 'Change what this tier launches'}>≡ tune</button>
    <button class="btn" onclick={() => actions.openEndpoint()} disabled={!online} title={kind === 'image' ? 'Open the sd-server web UI' : 'Open the endpoint'}>↗ {kind === 'image' ? 'web ui' : 'endpoint'}</button>
    <button class="btn" onclick={() => actions.toggleConsole(faulted ? true : undefined)}>&gt;_ {faulted ? 'full log' : 'console'}</button>
  </footer>
  <button class="cline" class:hot={faulted} onclick={() => actions.toggleConsole(true)} title="Open console"><span class="pr">&gt;</span><span class="lt">{lastLine}</span></button>
</div>

<style>
  .full {
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
    --fs-xs: max(10px, calc(var(--u) * 9.75));
    --fs-s: max(10.5px, calc(var(--u) * 10.25));
    --fs-m: max(11.5px, calc(var(--u) * 11.5));
    --fs-l: max(12px, calc(var(--u) * 12));
    --gap: calc(var(--u) * 7);
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    padding: 0 calc(var(--u) * 12) calc(var(--u) * 10);
    font-family: 'Iosevka', 'JetBrains Mono', Consolas, monospace;
    font-size: var(--fs-m);
    font-variant-numeric: tabular-nums;
    color: #b9d3ea;
    background: var(--void);
    overflow: hidden;
    user-select: none;
  }
  .full > :global(*) {
    flex: none;
    min-width: 0;
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
    outline: 1px solid var(--arc);
    outline-offset: 2px;
  }
  .amb {
    color: var(--amber);
  }
  .red {
    color: var(--red);
  }
  .grow {
    flex: 1 1 auto;
    align-self: stretch;
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  /* header */
  .hdr {
    height: calc(var(--u) * 46);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 10);
    border-bottom: 1px solid var(--rule);
    margin: 0 calc(var(--u) * -12);
    padding: 0 calc(var(--u) * 12);
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 9);
    white-space: nowrap;
  }
  .brand img {
    width: calc(var(--u) * 25);
    height: calc(var(--u) * 25);
    align-self: center;
    border-radius: calc(var(--u) * 5);
    border: 1px solid var(--rule2);
    pointer-events: none;
  }
  .wm {
    font-size: calc(var(--u) * 20);
    font-weight: 700;
    letter-spacing: 0.2em;
    color: var(--ice);
  }
  .sl {
    font-size: max(11px, calc(var(--u) * 12.7));
    color: var(--stream);
    letter-spacing: 0.06em;
  }
  .st {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8);
    font-size: var(--fs-l);
    letter-spacing: 0.16em;
    color: var(--muted);
    white-space: nowrap;
  }
  .st i {
    width: calc(var(--u) * 7);
    height: calc(var(--u) * 7);
    min-width: 6px;
    min-height: 6px;
    background: currentColor;
    box-shadow: 0 0 10px currentColor;
  }
  .st i.pulse {
    animation: blink 1s steps(1) infinite;
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
  .st .ck {
    color: var(--ice);
    letter-spacing: 0.06em;
  }
  .st .ck em {
    font-style: normal;
    color: var(--muted);
    letter-spacing: 0.08em;
  }
  .st .ck.ver {
    color: var(--muted);
  }
  .pbtn {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 6);
    height: calc(var(--u) * 24);
    padding: 0 calc(var(--u) * 9) 0 calc(var(--u) * 7);
    border: 1px solid var(--rule2);
    color: #7fa6cc;
    font-size: var(--fs-s);
    letter-spacing: 0.08em;
    white-space: nowrap;
    flex: none;
  }
  .pbtn:hover {
    border-color: var(--stream);
    color: var(--ice);
  }
  .pbtn.on {
    border-color: var(--arc);
    color: var(--arc);
  }
  .pbtn svg {
    width: calc(var(--u) * 14);
    height: calc(var(--u) * 14);
    min-width: 12px;
    min-height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.7;
  }
  .frameless .pbtn .pl {
    display: none;
  }

  /* tiers */
  .tiers {
    height: calc(var(--u) * 44);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--gap);
  }
  .tier {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-content: center;
    column-gap: calc(var(--u) * 7);
    row-gap: calc(var(--u) * 3);
    padding: 0 calc(var(--u) * 9);
    border: 1px solid var(--rule);
    background: var(--abyss);
    text-align: left;
    min-width: 0;
  }
  .tier:hover:not(.sel):not(:disabled) {
    border-color: var(--rule2);
  }
  .tier .tk {
    color: var(--muted);
    font-size: var(--fs-m);
  }
  .tier .tl {
    color: #cfe3f5;
    font-size: var(--fs-l);
    font-weight: 500;
    letter-spacing: 0.1em;
    white-space: nowrap;
    overflow: hidden;
  }
  .tier .rn {
    color: var(--arc);
    animation: blink 1s steps(1) infinite;
  }
  .tier .rn.amb {
    color: var(--amber);
  }
  .tier .lk,
  .tier .nb {
    font-size: var(--fs-xs);
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .tier .nb {
    color: var(--amber);
  }
  .tier .tm {
    grid-column: 1 / -1;
    font-size: var(--fs-s);
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
  .tier.sel .tm {
    color: #7fa6cc;
  }
  .tier.na .tl,
  .tier.locked .tl {
    color: var(--muted);
  }
  .tier.na .tm {
    color: #a07a3e;
  }
  .tier.locked {
    cursor: default;
    opacity: 0.6;
  }
  .tier.fault {
    border-color: var(--red);
    background: linear-gradient(180deg, #2a0812, #16040a);
    box-shadow: none;
  }

  /* model line */
  .mline {
    height: calc(var(--u) * 25);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 7);
    padding: 0 calc(var(--u) * 10);
    border: 1px solid var(--rule);
    background: var(--abyss);
    color: #cfe3f5;
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim {
    color: #8fb3d6;
  }
  .mline .mt {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .pr {
    color: var(--stream);
  }
  .caret {
    color: var(--arc);
    animation: blink 1.1s steps(1) infinite;
    margin-left: -0.3em;
  }

  /* field */
  .field {
    position: relative;
    flex: 1 1 0 !important;
    min-height: calc(var(--u) * 200);
    border: 1px solid var(--rule);
    overflow: hidden;
    background: #01040a;
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
    left: calc(var(--u) * 22);
    top: calc(var(--u) * 19);
    padding: calc(var(--u) * 12) calc(var(--u) * 19) calc(var(--u) * 14) calc(var(--u) * 15);
    background: radial-gradient(ellipse at 30% 50%, rgba(1, 4, 10, 0.92) 40%, rgba(1, 4, 10, 0) 100%);
    min-width: calc(var(--u) * 270);
    max-width: 62%;
  }
  .hl {
    font-size: var(--fs-m);
    letter-spacing: 0.24em;
    color: var(--arc);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hl .dir {
    margin-left: calc(var(--u) * 12);
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .hv {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 10);
    margin-top: calc(var(--u) * 2);
    white-space: nowrap;
  }
  .hv b {
    font-size: calc(var(--u) * 73);
    font-weight: 300;
    line-height: 1;
    color: var(--ice);
    text-shadow:
      0 0 18px rgba(92, 200, 255, 0.45),
      0 0 46px rgba(46, 139, 255, 0.28);
  }
  .hv .u {
    font-size: calc(var(--u) * 20);
    color: #7fa6cc;
    letter-spacing: 0.06em;
  }
  .hs {
    margin-top: calc(var(--u) * 5);
    font-size: var(--fs-l);
    color: #8fb3d6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hs.amb {
    color: var(--amber);
  }
  .hsteps {
    display: flex;
    flex-wrap: wrap;
    gap: calc(var(--u) * 4) calc(var(--u) * 12);
    margin-top: calc(var(--u) * 8);
    font-size: var(--fs-s);
    color: var(--muted);
  }
  .hsteps .done {
    color: #8fb3d6;
  }
  .hsteps .active {
    color: var(--ice);
  }
  .hsteps .failed {
    color: var(--red);
  }
  .hlog {
    margin-top: calc(var(--u) * 10);
    padding-left: calc(var(--u) * 10);
    border-left: 1px solid rgba(255, 77, 109, 0.45);
    display: grid;
    row-gap: calc(var(--u) * 3);
    font-size: var(--fs-xs);
    color: #d9a5b0;
  }
  .hlog div {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
  .hero.red .hs,
  .hero.red .hs :global(.lock) {
    white-space: pre-wrap;
  }
  .hero.red .hv b {
    color: var(--red);
    text-shadow: 0 0 24px rgba(255, 77, 109, 0.4);
  }
  .mhead {
    position: absolute;
    right: calc(var(--u) * 12);
    top: calc(var(--u) * 8);
    font-size: var(--fs-s);
    letter-spacing: 0.08em;
    color: var(--muted);
    background: rgba(1, 4, 10, 0.85);
    padding: calc(var(--u) * 3) calc(var(--u) * 7);
    white-space: nowrap;
  }
  .mhead b {
    color: var(--ice);
    font-weight: 500;
  }
  .mhead b.amb {
    color: var(--amber);
  }
  .mhead b.red {
    color: var(--red);
  }

  /* rows */
  .rows {
    height: calc(var(--u) * 44);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border: 1px solid var(--rule);
    background: var(--abyss);
  }
  .kv {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 3);
    padding: 0 calc(var(--u) * 10);
    border-right: 1px solid var(--rule);
    min-width: 0;
  }
  .kv:last-child {
    border-right: 0;
  }
  .kv .k {
    font-size: var(--fs-xs);
    letter-spacing: 0.2em;
    color: var(--stream);
    text-transform: uppercase;
  }
  .kv .v {
    font-size: var(--fs-l);
    color: var(--ice);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v {
    color: #7fa6cc;
  }

  /* timeline */
  .tline {
    height: calc(var(--u) * 63);
    display: flex;
    flex-direction: column;
    gap: calc(var(--u) * 7);
    padding: calc(var(--u) * 7) calc(var(--u) * 10);
    border: 1px solid var(--rule);
    background: var(--abyss);
  }
  .cap {
    font-size: var(--fs-xs);
    letter-spacing: 0.2em;
    color: var(--stream);
    text-transform: uppercase;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
    gap: calc(var(--u) * 9);
    align-items: center;
  }
  .req {
    display: flex;
    height: calc(var(--u) * 14);
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
  .req.cur {
    border-color: #8fb3d6;
  }
  .req.empty {
    border-style: dashed;
    border-color: var(--rule);
  }
  .req.failed,
  .job.failed {
    border: 1px dashed var(--red);
    background: repeating-linear-gradient(-45deg, rgba(255, 77, 109, 0.5) 0 1px, transparent 1px 6px);
  }
  .jobs {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(12, minmax(0, 1fr));
    align-items: end;
    gap: calc(var(--u) * 7);
  }
  .job {
    position: relative;
    height: var(--h);
    min-height: 3px;
    background: var(--arc);
  }
  .job.edit {
    background: repeating-linear-gradient(-45deg, var(--arc) 0 3px, #1d5fa8 3px 6px);
  }
  .job.empty {
    height: 3px;
    background: none;
    border-top: 1px dashed var(--rule2);
  }
  .job.failed {
    height: 100%;
  }
  .job em {
    position: absolute;
    top: calc(var(--u) * -13);
    left: 0;
    right: 0;
    text-align: center;
    font-style: normal;
    font-size: max(9.5px, calc(var(--u) * 9));
    color: var(--muted);
  }

  /* controls */
  .ctrl {
    height: calc(var(--u) * 41);
    display: flex;
    align-items: center;
    gap: var(--gap);
    padding: 0 calc(var(--u) * 8);
    border: 1px solid var(--rule);
    background: var(--abyss);
    overflow: hidden;
  }
  .chip,
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: calc(var(--u) * 6);
    height: calc(var(--u) * 25);
    padding: 0 calc(var(--u) * 10);
    border: 1px solid var(--rule2);
    font-size: var(--fs-m);
    color: #a9c7e3;
    white-space: nowrap;
    flex: none;
  }
  .chip {
    color: #7fa6cc;
  }
  .chip em {
    font-style: normal;
    color: var(--muted);
  }
  .chip:disabled,
  .btn:disabled {
    cursor: default;
  }
  .btn:disabled {
    opacity: 0.35;
  }
  .chip:hover:not(:disabled),
  .btn:hover:not(:disabled) {
    border-color: var(--stream);
    color: var(--ice);
  }
  /* the primary place: one width for launch / cancel / stop / restart, so nothing beside it moves */
  .btn.act {
    width: calc(var(--u) * 172);
    letter-spacing: 0.08em;
    overflow: hidden;
  }
  .btn.go {
    background: var(--stream);
    border-color: var(--stream);
    color: #010915;
    font-weight: 700;
    box-shadow: 0 0 22px rgba(46, 139, 255, 0.45);
  }
  .btn.go:hover:not(:disabled) {
    color: #010915;
    border-color: var(--arc);
  }
  .btn.go:disabled {
    opacity: 1;
    background: #0a1a2e;
    border-color: var(--rule2);
    color: var(--muted);
    box-shadow: none;
    font-weight: 500;
  }
  .btn.stop {
    border-color: var(--red);
    color: var(--red);
  }
  .btn.hot {
    background: var(--red);
    border-color: var(--red);
    color: #12030a;
    font-weight: 700;
  }
  .btn.stop:hover:not(:disabled),
  .btn.hot:hover:not(:disabled) {
    border-color: #ff8fa3;
  }
  .btn.hot:hover:not(:disabled) {
    color: #12030a;
  }
  .cline {
    height: calc(var(--u) * 18);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 7);
    font-size: var(--fs-xs);
    color: #6d8fb3;
    white-space: nowrap;
    overflow: hidden;
    text-align: left;
  }
  .cline .lt {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cline:hover .lt {
    color: var(--ice);
  }
  .cline.hot .lt {
    color: #d9a5b0;
  }
  @media (prefers-reduced-motion: reduce) {
    .caret,
    .tier .rn,
    .st i.pulse {
      animation: none;
    }
  }
</style>
