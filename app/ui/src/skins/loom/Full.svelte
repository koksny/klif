<script lang="ts">
  // Loom, full window. The transformer in 3D (scene.ts) on an amber phosphor screen, inside the fixed KLIF
  // skeleton: header, System strip, model line, the scene (hero overlay + labels projected through the CRT
  // barrel), detail rows, request timeline / recent jobs, controls, console line. A phase change only changes
  // what the regions say and what the tower does, never where they are:
  //   idle      - the selected tier's tower, dim and orbiting; STANDBY; configured values; the VRAM fit ring
  //   loading   - layers build bottom-up with the load fraction; LOAD %
  //   live-llm  - decode (one token at a time, router fork, attention arcs, logits) or prefill (a curtain)
  //   live-img  - one DiT pass per sampling step, the latent patches resolving
  //   dormant   - the GPU asleep: paged-out layers ghosted, the helix and the fill ring dim
  //   fault     - the tower flickers red and stops; FAULT in the hero, exit and log in the rows
  import { prefersReducedMotion } from 'svelte/motion';
  import type { Actions, RequestRecord, ViewModel } from '../../lib/model/types';
  import { EXTERNAL_NOTE, EXTERNAL_TITLE, KIND_LABEL, canLaunch, canStop, doLaunch, idleState, isPendingLaunch, launchCtl, selectedSystem } from '../../lib/model/systems';
  import { strip } from '../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../lib/shell/SystemTabs/tabs';
  import { fmtClock, fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../lib/model/format';
  import Stage from './Stage.svelte';
  import Lock from './Lock.svelte';
  import Chip from './Chip.svelte';
  import WinCtl from './WinCtl.svelte';
  import type { Labels } from './scene';
  import { SHOWN_TPS } from './scene';
  import { layerLabel, shapeOf } from './shape';
  import { held, useSleep } from './state.svelte';
  import { availabilityText, fitOf, fmtAgo, fmtDur, fmtEta, lastSessionText, modelShort, modelText, shortTier } from './text';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  let w = $state(1280);
  let h = $state(1360);
  // One design pixel: the layout is drawn at 1280x1360 and scales with the window.
  const u = $derived(Math.max(0.78, Math.min(1.2, Math.min(w / 1280, h / 1360))));

  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const selSlot = $derived(selectedSystem(vm) ?? undefined);
  const tabs = $derived(tabsFor(vm));
  const kind = $derived(selSlot?.kind ?? 'llm');
  const model = $derived(s?.model ?? selSlot?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);
  const shape = $derived(shapeOf(vm));
  const arch = $derived(shape.arch);

  const loading = $derived(phase === 'starting' || phase === 'loading');
  const busy = $derived(loading || phase === 'stopping');
  const faulted = $derived(phase === 'fault');
  const online = $derived(phase === 'live');
  const sleep = useSleep(() => vm);
  const dz = $derived(online ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);
  /** The launch control: with conflicts it reads "STOP S1 & LAUNCH" and sends stopOthers. */
  const ctl = $derived(launchCtl(vm, selSlot, { short: true }));
  const idleWhy = $derived(idleState(selSlot));
  /** We may stop / restart it (not an external server, not a node that only lets us look). */
  const mine = $derived(!!selSlot && selSlot.controllable && !selSlot.external);
  const heldWhy = $derived(selSlot?.external ? 'External server: it runs where it was started.' : !selSlot?.controllable ? 'This node does not allow launching.' : '');
  const frameless = $derived(!!vm.host?.frameless);
  const panel = $derived(vm.host?.panel?.available ? vm.host.panel : null);
  const panelTip = $derived(`Panel mode: show on the small screen${panel?.target ? ` (${panel.target})` : ''}`);

  const tps = held(() => llm?.decodeTps ?? 0);
  const prefilling = $derived(!!llm?.prefill && llm.activity === 'prefill');
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || 0);
  const ctxPct = $derived(llm && llm.context.totalTokens > 0 ? Math.round((llm.context.usedTokens / llm.context.totalTokens) * 100) : 0);

  type Hero = { label: string; note: string; value: string; unit: string; sub: string; tone: '' | 'dim' | 'red'; subTone: '' | 'amb' };
  const hero = $derived.by<Hero>(() => {
    const base = { note: '', tone: '' as const, subTone: '' as const };
    if (!s) {
      const blocked = idleWhy.warn || selSlot?.status === 'not-set';
      const sub = !blocked ? (vm.lastSession ? lastSessionText(vm.lastSession, vm.systems) : 'nothing running') : `${selSlot?.label.toLowerCase() ?? ''}: ${(selSlot?.reason ?? idleWhy.text).toLowerCase()}`;
      return { ...base, label: 'STANDBY', value: '—', unit: kind === 'image' ? 'steps' : kind === 'llm' ? 'tok/s' : 'requests', sub, tone: 'dim', subTone: idleWhy.warn ? 'amb' : '' };
    }
    if (faulted) return { ...base, label: 'FAULT', note: s.fault ? fmtAgo(s.fault.sinceS) : '', value: 'ERR', unit: '', sub: s.fault?.title ?? 'the server stopped', tone: 'red' };
    if (phase === 'stopping') return { ...base, label: 'STOPPING', value: '—', unit: '', sub: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB`, tone: 'dim' };
    if (loading) {
      const st = s.loading?.steps.find((x) => x.state === 'active');
      return {
        ...base,
        label: 'LOAD',
        note: 'layers build bottom-up',
        value: String(Math.floor((s.loading?.fraction ?? 0) * 100)),
        unit: '%',
        sub: st ? `${st.label.toLowerCase()}${st.detail ? ` · ${st.detail}` : ''}` : 'starting',
      };
    }
    if (dz) {
      if (waking) return { ...base, label: 'GPU WAKING', note: 'layers return from system ram', value: String(Math.round(dz.restoredFrac * 100)), unit: '%', sub: `${fmtGiB(dz.pagedOutGiB)} GiB still in system ram`, subTone: 'amb' };
      return {
        ...base,
        label: `GPU ASLEEP${dz.powerState ? ` · ${dz.powerState}` : ''}`,
        note: 'layers paged out to ram',
        value: llm ? fmtTps(tps.current) : '—',
        unit: llm ? 'tok/s' : '',
        sub: `${fmtGiB(dz.pagedOutGiB)} GiB paged out · ${fmtDur(dz.sinceS)}`,
        tone: 'dim',
      };
    }
    if (prefilling && llm?.prefill) {
      const p = llm.prefill;
      const cached = p.cachedTokens ?? 0;
      return {
        ...base,
        label: 'PREFILL',
        note: 'the whole prompt in parallel',
        value: String(p.tokens > 0 ? Math.floor((p.doneTokens / p.tokens) * 100) : 0),
        unit: '%',
        sub: `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s · ${fmtEta(p.etaS)} left${cached > 0 ? ` · ${fmtInt(cached)} from prompt cache` : ''}`,
      };
    }
    if (llm) {
      const decoding = llm.activity === 'decode';
      // the scene draws at most SHOWN_TPS tokens a second: say how much slower than real time it runs
      const dil = prefersReducedMotion.current ? '' : tps.current > SHOWN_TPS ? ` · time ×1/${Math.round(tps.current / SHOWN_TPS)}` : ' · real time';
      return { ...base, label: 'DECODE', note: decoding ? `one token per pass${dil}` : 'idle', value: fmtTps(tps.current), unit: 'tok/s', sub: `${fmtInt(llm.generatedTokens)} tok generated`, tone: decoding ? '' : 'dim' };
    }
    if (gen) {
      const on = (gen.requestsInFlight ?? 0) > 0;
      return {
        ...base,
        label: KIND_LABEL[kind].toUpperCase(),
        note: on ? 'a request is running' : 'idle',
        value: gen.requestsTotal !== undefined ? fmtInt(gen.requestsTotal) : '—',
        unit: 'requests',
        sub: on ? `${gen.requestsInFlight} in flight` : gen.lastActivityS !== undefined ? `last activity ${fmtDur(gen.lastActivityS)} ago` : 'waiting for the next request',
        tone: on ? '' : 'dim',
      };
    }
    if (img) {
      if (img.activity === 'generating' && img.steps > 0)
        return { ...base, label: 'DIFFUSION', note: 'one pass per step, real time', value: `${img.step}/${img.steps}`, unit: 'steps', sub: `${img.sPerIt.toFixed(2)} s/it · ${img.width}x${img.height}${img.edit ? ' · edit' : ''}` };
      return { ...base, label: 'DIFFUSION', value: '—', unit: 'steps', sub: 'waiting for the next job', tone: 'dim' };
    }
    return { ...base, label: 'LIVE', value: '—', unit: '', sub: 'waiting for data', tone: 'dim' };
  });

  // Detail rows: four fixed facts per kind (a fault puts its exit and log in the same box).
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const failedStep = $derived(fault?.steps?.find((x) => x.state === 'failed') ?? null);
  const exitText = $derived(fault ? (fault.exitCode === undefined ? 'not reported' : fault.exitCodeHex ? `${fault.exitCodeHex} (${fault.exitCode})` : String(fault.exitCode)) : '');
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
    if (kind !== 'llm') {
      return [
        ['in flight', gen ? fmtInt(gen.requestsInFlight ?? 0) : '—'],
        ['last activity', gen?.lastActivityS !== undefined ? `${fmtDur(gen.lastActivityS)} ago` : '—'],
        ['model', model?.name ? `${model.name.toLowerCase()}${model.quant ? ` · ${model.quant.toLowerCase()}` : ''}` : '—'],
        ['reports', gen?.modelId?.toLowerCase() ?? '—'],
      ];
    }
    const heads = arch?.heads ? ` · ${arch.heads} heads${arch.kvHeads ? ` / ${arch.kvHeads} kv` : ''}` : '';
    const params = arch?.params ? ` · ${arch.params}${shape.kind === 'moe' ? '' : ' params'}` : '';
    return [
      ['context', llm ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(llm.context.totalTokens)} · ${ctxPct}%` : ctxTotal ? `— / ${fmtInt(ctxTotal)}` : '—'],
      ['layers', shape.known ? `${shape.layers}${heads}` : 'unknown until first load'],
      ['experts', shape.kind === 'moe' ? `${shape.active}${shape.shared ? ` + ${shape.shared}` : ''} of ${fmtInt(shape.experts)}${params}` : shape.known ? `dense${params}` : '—'],
      ['speculative', llm?.spec ? `${Math.round(llm.spec.acceptancePct)}% · ${llm.spec.mode.toLowerCase()}` : (model?.specMode?.toLowerCase() ?? 'off')],
    ];
  });

  // Request timeline (LLM, last 8; with a fault the last place marks it) / recent jobs (image, last 12).
  const reqN = $derived(faulted && llm ? 7 : 8);
  const requests = $derived(llm ? llm.requests.slice(-reqN) : []);
  const reqSlots = $derived(Array.from({ length: reqN }, (_, i) => requests[i - (reqN - requests.length)] ?? null));
  const diedInRequest = $derived(faulted && !!llm && llm.activity !== 'idle');
  const JOBS = 12;
  const jobs = $derived(img ? img.recent.slice(-JOBS) : []);
  const jobSlots = $derived(Array.from({ length: JOBS }, (_, i) => jobs[i - (JOBS - jobs.length)] ?? null));
  const maxJob = $derived(Math.max(1, ...jobs.map((j) => j.seconds)));
  const tlCaption = $derived.by(() => {
    if (!s) return ' · not running';
    if (loading) return kind === 'image' ? ' · no jobs yet' : ' · no requests yet';
    if (kind !== 'llm' && kind !== 'image') return faulted ? ' · failed' : ' · no per-request log for this kind';
    if (kind === 'image') {
      if (!img) return faulted ? ' · no jobs: failed during startup' : ' · no jobs yet';
      return jobs.length ? ` · bar height = time, last ${jobs.length} of ${fmtInt(img.imagesThisSession)}` : ' · no finished jobs yet';
    }
    if (faulted) return llm ? ` · ${requests.length ? `last ${requests.length}` : 'no finished requests'}, then ${diedInRequest ? 'the request that died' : 'the fault'}` : ' · no requests: failed during startup';
    if (!requests.length) return ' · no finished requests yet';
    return ` · last ${requests.length}, prefill then decode`;
  });

  // Scene labels.
  const lab: Labels = $state({});
  const vramLabel = $derived.by(() => {
    const v = vm.vram;
    const fit = !s ? fitOf(v, selSlot) : null;
    if (fit) return fit.spare >= 0 ? `fit ${fmtGiB(fit.top)} / ${fmtGiB(v.totalGiB)} · ${fmtGiB(fit.spare)} spare` : `fit ${fmtGiB(fit.top)} / ${fmtGiB(v.totalGiB)} · over by ${fmtGiB(-fit.spare)}`;
    if (dz) return `vram resident ${fmtGiB(v.usedGiB)} / ${fmtGiB(v.totalGiB)} · ${fmtGiB(dz.pagedOutGiB)} paged out`;
    return `vram ${fmtGiB(v.usedGiB)} / ${fmtGiB(v.totalGiB)}${v.spillMiB > 0 ? ` · spill ${fmtGiB(v.spillMiB / 1024)}` : ''}`;
  });
  const topLabel = $derived(
    kind === 'image' ? `latent patches${img?.activity === 'generating' && img.steps > 0 ? ` · step ${img.step} / ${img.steps}` : ''}` : kind !== 'llm' ? `${KIND_LABEL[kind].toLowerCase()} · output` : `logits${arch?.vocab ? ` · ${fmtInt(arch.vocab)} vocab` : ''} · top-20 · sampled token`,
  );
  const bottomLabel = $derived(kind === 'image' ? 'patch embedding · block 1' : kind !== 'llm' ? 'input · layer 1' : `embedding${arch?.embd ? ` · ${fmtInt(arch.embd)} wide` : ''} · layer 1`);

  const statusTone = $derived(faulted ? 'red' : dz || loading ? 'amb' : s ? 'live' : 'off');
  const statusText = $derived(dz ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : s ? phase.toUpperCase() : 'IDLE');
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const port = $derived(s?.endpoint.port ?? selSlot?.command?.port);

  function reqTitle(r: RequestRecord): string {
    return `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${fmtSeconds(r.prefillS)} · ${fmtInt(r.generatedTokens)} tok in ${fmtSeconds(r.decodeS)}`;
  }
</script>

<div class="full" bind:clientWidth={w} bind:clientHeight={h} style="--u:{u}">
  <header class="hdr" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region><img src="/koksny-mark.png" alt="" draggable="false" /><span class="w">KLIF</span><span class="sl">LOOM</span></div>
    <span class="grow" data-tauri-drag-region></span>
    <div class="st {statusTone}" data-tauri-drag-region>
      <i></i>{statusText}<span class="sep">│</span>
      {#if s}<span class="clk">{fmtClock(loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS)}</span>{:else}<span class="ver">v{vm.host?.appVersion ?? '—'}</span>{/if}
    </div>
    {#if panel}
      <button class="pbtn" class:on={panel.active} onclick={() => actions.togglePanel()} title={panelTip} aria-label={panelTip} aria-pressed={panel.active}>▭ PANEL</button>
    {/if}
    {#if frameless}<WinCtl {actions} maximized={!!vm.host?.maximized} />{/if}
  </header>

  <!-- System strip: click selects (never stops anything), double-click launches a System that is ready and has
       nothing in its way. Scrolls sideways; + adds a System. -->
  <div class="strip">
    <nav class="tiers" aria-label="Systems" use:strip={vm.selected}>
      {#each tabs as t (t.id)}
        {@const ts = t.system}
        {@const na = ts.availability !== 'ready' && ts.status !== 'not-set'}
        <button
          class="tier"
          class:sel={t.id === vm.selected}
          class:na
          class:fault={ts.status === 'fault'}
          aria-pressed={t.id === vm.selected}
          data-sel={t.id === vm.selected}
          title={`${t.label}${t.nodeName ? ` on ${t.nodeName}` : ''}: ${ts.model.name || 'no preset'} (${ts.status}${ts.reason ? `: ${ts.reason}` : ''})`}
          onclick={() => actions.select(t.id)}
          ondblclick={() => {
            if (canLaunch(ts) && ts.conflicts.length === 0) void actions.launch(ts.id).catch(() => {});
          }}
        >
          <span class="tn"><TabLabel tab={t} /></span>
          <span class="tm" class:bad={na}>{ts.status === 'not-set' ? 'no preset' : ts.status === 'unreachable' ? `${t.nodeName ?? 'node'} unreachable` : na ? `${modelShort(ts.model)} · ${availabilityText(ts.availability)}` : modelShort(ts.model)}</span>
        </button>
      {/each}
    </nav>
    <button class="tier add" type="button" title="Add a System" aria-label="Add a System" onclick={() => actions.openTune(undefined, { add: true })}>+</button>
  </div>

  <div class="mline" class:dim={!s}><span class="pr">&gt;</span> <span class="mt">{modelText(model)}</span><span class="caret">▌</span></div>

  <section class="scene">
    <Stage {vm} {shape} variant="full" labels={lab} />
    <div class="crt"></div>
    <div class="hero {hero.tone}" bind:this={lab.hero}>
      <div class="hl">{hero.label}{#if hero.note}<span class="note">{hero.note}</span>{/if}</div>
      <div class="hv"><b><Lock value={hero.value} /></b><span class="u">{hero.unit}</span></div>
      <div class="hs" class:amb={hero.subTone === 'amb'}>{hero.sub}</div>
    </div>
    <div class="lab" bind:this={lab.top}><span><Chip text={topLabel} /></span></div>
    {#if llm?.spec && shape.kind !== 'dit'}<div class="lab" bind:this={lab.draft}><span><Chip text={`draft · ${llm.spec.mode.toLowerCase()} · ${Math.round(llm.spec.acceptancePct)}% accepted`} /></span></div>{/if}
    <div class="lab" bind:this={lab.bottom}><span><Chip text={bottomLabel} /></span></div>
    <div class="lab" class:unk={!shape.known} bind:this={lab.layer}><span><Chip text={layerLabel(shape)} /></span></div>
    {#if llm && !faulted}<div class="lab kv" bind:this={lab.kv}><span><Chip text={`kv cache · ${fmtInt(llm.context.usedTokens)} tok · ${ctxPct}%`} /></span></div>{/if}
    <div class="lab vr" class:red={vm.vram.spillMiB > 0 || vramLabel.includes('over by')} bind:this={lab.vram}><span><Chip text={vramLabel} /></span></div>
  </section>

  <section class="rows" class:dim={!s} class:flt={!!fault}>
    {#if fault}
      <div class="kv"><span class="k">fault</span><span class="v">{fmtAgo(fault.sinceS)}{failedStep ? ` · at ${failedStep.label.toLowerCase()}` : ''}</span></div>
      <div class="kv"><span class="k">exit</span><span class="v">{exitText}</span></div>
      <div class="kv log"><span class="k">log</span><span class="v mono" title={fault.logTail.join('\n')}>{fault.logTail[fault.logTail.length - 1] ?? '—'}</span></div>
    {:else}
      {#each rows as [k, v] (k)}<div class="kv"><span class="k">{k}</span><span class="v" title={v}>{v}</span></div>{/each}
    {/if}
  </section>

  <section class="tline">
    <div class="cap">{kind === 'image' ? 'RECENT JOBS' : 'REQUEST TIMELINE'}<span class="dim">{tlCaption}</span></div>
    {#if kind === 'image'}
      <div class="jobs">
        {#each jobSlots as j, i (i)}
          {#if j}<span class="job" class:edit={j.edit} style="--h:{(j.seconds / maxJob) * 100}%" title={`${fmtSeconds(j.seconds)} · ${j.width}x${j.height}${j.edit ? ' · edit' : ''}`}></span>{:else}<span class="job empty"></span>{/if}
        {/each}
        {#if faulted && img}<span class="job failed" title="The server died"></span>{/if}
      </div>
    {:else}
      <div class="reqs">
        {#each reqSlots as r, i (i)}
          {#if r}
            {@const tot = r.prefillS + r.decodeS}
            <span class="req" title={reqTitle(r)}><span class="pre" style="width:{tot > 0 ? (r.prefillS / tot) * 100 : 0}%"></span><span class="dec"></span></span>
          {:else}<span class="req empty"></span>{/if}
        {/each}
        {#if faulted && llm}<span class="req failed" title={diedInRequest ? 'The server died during this request' : 'The server died with no request in flight'}></span>{/if}
      </div>
    {/if}
  </section>

  <footer class="ctrl">
    <button class="chip" onclick={() => actions.copyEndpoint(selSlot?.id)} disabled={!online} title={online && s ? `Copy http://${s.endpoint.host}:${s.endpoint.port}` : 'Endpoint offline'}>
      :{port ?? '—'}{#if !online}<em>offline</em>{/if}
    </button>
    <button class="chip" onclick={() => actions.copyApiKey()} disabled={!online || !s?.apiKeySet} title={s?.apiKeySet ? 'Copy API key' : 'No API key set'}>
      {#if s?.apiKeySet}•••••••<em>copy</em>{:else}<em class="nk">no key</em>{/if}
    </button>
    <span class="grow"></span>
    {#if selSlot?.external}
      <button class="btn act ext" disabled title={EXTERNAL_TITLE}>{EXTERNAL_NOTE.toUpperCase()}</button>
    {:else if !s && isPendingLaunch(selSlot)}
      <button class="btn act stop" onclick={() => actions.stop(selSlot?.id)} disabled={!canStop(selSlot)} title={selSlot?.reason ?? 'Cancel the launch'}>■ CANCEL {shortTier(selSlot?.label ?? '')}</button>
    {:else if !s}
      <button class="btn act go" onclick={() => doLaunch(actions, selSlot, ctl)} disabled={!ctl.enabled} title={ctl.enabled ? (ctl.stopOthers ? `${ctl.text}: ${selSlot?.reason ?? ''}` : `Launch ${selSlot?.label ?? ''}`) : `${selSlot?.label ?? ''}: ${ctl.blocked}`}>
        ▶ {ctl.stopOthers ? ctl.text.toUpperCase() : `LAUNCH ${shortTier(selSlot?.label ?? '')}`}
      </button>
    {:else if faulted}
      <button class="btn act hot" onclick={() => actions.restart(selSlot?.id)} disabled={!mine} title="Launch the System that failed again">↻ RESTART {shortTier(selSlot?.label ?? '')}</button>
    {:else if phase === 'stopping'}
      <button class="btn act stop" disabled>■ STOPPING</button>
    {:else}
      <button class="btn act stop" onclick={() => actions.stop(selSlot?.id)} disabled={!canStop(selSlot)} title={heldWhy || (loading ? 'Cancel the launch' : 'Stop the server')}>■ {loading ? 'CANCEL' : 'STOP'} {shortTier(selSlot?.label ?? '')}</button>
    {/if}
    {#if faulted}
      <button class="btn w7" onclick={() => actions.dismiss(selSlot?.id)} disabled={!mine} title="Back to the launcher">DISMISS</button>
    {:else}
      <button class="btn w7" onclick={() => actions.restart(selSlot?.id)} disabled={!s || busy || !mine} title={heldWhy || 'Stop and launch again with the current settings'}>RESTART</button>
    {/if}
    <button class="btn w4" onclick={() => actions.openTune(selSlot?.id)} title={s && !faulted ? 'Change the settings; Restart to apply them' : 'Change what this System launches'}>TUNE</button>
    <button class="btn w7 rec" onclick={() => actions.openRecords()} title="Records: the best each model reached" aria-label="Records"><svg class="ri" viewBox="0 0 16 16" aria-hidden="true"><path d="M2.5 13.5V8h3v5.5M5.5 13.5V2.5h4v11M9.5 13.5V6h4v7.5" /></svg><span class="rl">RECORDS</span></button>
    <button class="btn w8" onclick={() => actions.openEndpoint(selSlot?.id)} disabled={!online} title={kind === 'image' ? 'Open the sd-server web UI' : 'Open the endpoint'}>{kind === 'image' ? 'WEB UI' : 'ENDPOINT'}</button>
    <button class="btn w8" onclick={() => actions.toggleConsole(faulted ? true : undefined)}>{faulted ? 'FULL LOG' : 'CONSOLE'}</button>
  </footer>
  <button class="cline" onclick={() => actions.toggleConsole(true)} title="Open the console"><span class="pr">&gt;</span> <span class="lt">{lastLine}</span></button>
</div>

<style>
  .full {
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
    /* Archivo Expanded (the width axis at 125%) for display and caps labels; JetBrains Mono for data. */
    --disp: 'Archivo Variable', 'Archivo', sans-serif;
    --mono: 'JetBrains Mono Variable', Consolas, monospace;
    --fs-data: max(11px, calc(var(--u) * 12.5px));
    --fs-small: max(10px, calc(var(--u) * 11.5px));
    --fs-label: max(10px, calc(var(--u) * 11px));
    --gap: calc(var(--u) * 8px);
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    padding: 0 calc(var(--u) * 14px) calc(var(--u) * 12px);
    font-family: var(--mono);
    font-size: var(--fs-data);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: #e8a83c;
    background: var(--void);
    text-shadow: 0 0 6px rgba(255, 150, 0, 0.3);
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
    text-shadow: inherit;
  }
  button:focus-visible {
    outline: 1px solid var(--amber);
    outline-offset: 2px;
  }
  .grow {
    flex: 1 1 auto;
    align-self: stretch;
  }
  /* The display voice: Archivo Expanded caps. */
  .brand .w,
  .brand .sl,
  .st,
  .pbtn,
  .tier .tn,
  .hl,
  .hv,
  .cap,
  .kv .k,
  .btn {
    font-family: var(--disp);
    font-stretch: 125%;
  }
  .brand .sl,
  .st,
  .pbtn,
  .tier .tn,
  .hl,
  .cap,
  .kv .k,
  .btn {
    font-weight: 600;
    text-transform: uppercase;
  }

  /* Header */
  .hdr {
    height: calc(var(--u) * 54px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 14px);
    border-bottom: 1px solid var(--rule);
    margin: 0 calc(var(--u) * -14px);
    padding: 0 calc(var(--u) * 14px);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 12px);
  }
  .brand img {
    width: calc(var(--u) * 28px);
    height: calc(var(--u) * 28px);
    border-radius: calc(var(--u) * 6px);
    filter: sepia(1) saturate(4) hue-rotate(-12deg) brightness(0.95);
    pointer-events: none;
  }
  .brand .w {
    font-size: calc(var(--u) * 22px);
    font-weight: 600;
    letter-spacing: 0.26em;
    color: var(--white);
  }
  .brand .sl {
    font-size: calc(var(--u) * 10.5px);
    color: var(--ember);
    letter-spacing: 0.34em;
  }
  .st {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 10px);
    font-size: var(--fs-label);
    letter-spacing: 0.18em;
    color: var(--muted);
    white-space: nowrap;
  }
  .st i {
    width: calc(var(--u) * 7px);
    height: calc(var(--u) * 7px);
    background: currentColor;
    box-shadow: 0 0 8px currentColor;
  }
  .st.live {
    color: var(--amber);
  }
  .st.amb {
    color: var(--ember);
  }
  .st.red {
    color: var(--red);
  }
  .st .sep {
    color: var(--rule2);
    font-weight: 300;
  }
  .st .clk,
  .st .ver {
    font-family: var(--mono);
    font-stretch: 100%;
    font-size: var(--fs-data);
    font-weight: 400;
    letter-spacing: 0.04em;
    color: var(--white);
  }
  .st .ver {
    color: var(--muted);
  }
  .pbtn {
    height: calc(var(--u) * 28px);
    padding: 0 calc(var(--u) * 10px);
    border: 1px solid var(--rule2);
    font-size: var(--fs-label);
    letter-spacing: 0.16em;
    color: #d7962e;
    white-space: nowrap;
  }
  .pbtn:hover {
    border-color: var(--amber);
    color: var(--white);
  }
  .pbtn.on {
    border-color: var(--amber);
    color: var(--amber);
  }

  /* Tier strip */
  .strip {
    height: calc(var(--u) * 52px);
    display: flex;
    gap: var(--gap);
    min-width: 0;
  }
  .tiers {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    gap: var(--gap);
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tiers::-webkit-scrollbar {
    display: none;
  }
  .tiers .tier {
    flex: 1 0 calc(var(--u) * 170px);
  }
  .tier.add {
    flex: none;
    width: calc(var(--u) * 44px);
    display: grid;
    place-items: center;
    padding: 0;
    color: var(--amber);
    font-size: calc(var(--u) * 20px);
  }
  .tier {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-content: center;
    row-gap: calc(var(--u) * 5px);
    column-gap: calc(var(--u) * 8px);
    padding: 0 calc(var(--u) * 12px);
    border: 1px solid var(--rule);
    background: var(--panel);
    min-width: 0;
    text-align: left;
  }
  .tier:hover:not(.sel) {
    border-color: var(--rule2);
  }
  .tier .tn {
    grid-column: 1 / -1;
    display: flex;
    min-width: 0;
    font-size: var(--fs-label);
    letter-spacing: 0.16em;
    color: var(--hot);
    white-space: nowrap;
    overflow: hidden;
  }
  .tier .tm {
    grid-column: 1 / -1;
    font-size: var(--fs-small);
    font-weight: 300;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tier .tm.bad {
    color: var(--ember);
  }
  .tier.sel {
    border-color: var(--amber);
    background: linear-gradient(180deg, #2a1604, #160b02);
    box-shadow: 0 0 20px rgba(255, 160, 0, 0.15) inset;
  }
  .tier.sel .tn {
    color: var(--white);
  }
  .tier.sel .tm {
    color: #c08a3c;
  }
  .tier.na .tn {
    color: var(--muted);
  }
  .tier.fault {
    border-color: var(--red);
    background: linear-gradient(180deg, #2a0a04, #160402);
    box-shadow: 0 0 20px rgba(255, 59, 48, 0.15) inset;
  }

  /* Model line */
  .mline {
    height: calc(var(--u) * 30px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8px);
    padding: 0 calc(var(--u) * 12px);
    border: 1px solid var(--rule);
    background: var(--panel);
    color: var(--hot);
    font-weight: 300;
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim .mt {
    color: #c08a3c;
  }
  .mt {
    overflow: hidden;
    text-overflow: ellipsis;
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

  /* The scene: absorbs the remaining height */
  .scene {
    position: relative;
    flex: 1 1 0;
    min-height: calc(var(--u) * 280px);
    border: 1px solid var(--rule);
    overflow: hidden;
    background: var(--void);
  }
  .crt {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      repeating-linear-gradient(0deg, rgba(0, 0, 0, 0.25) 0 1px, transparent 1px 3px),
      radial-gradient(ellipse at 50% 50%, transparent 60%, rgba(0, 0, 0, 0.7) 100%);
  }
  /* Shrink-to-fit, so the scene's labels can step around it. */
  .hero {
    position: absolute;
    left: calc(var(--u) * 24px);
    top: calc(var(--u) * 20px);
    max-width: calc(100% - var(--u) * 48px);
    pointer-events: none;
  }
  .hl {
    font-size: var(--fs-label);
    letter-spacing: 0.2em;
    color: var(--amber);
    white-space: nowrap;
  }
  .hl .note {
    margin-left: calc(var(--u) * 14px);
    font-family: var(--mono);
    font-stretch: 100%;
    font-size: var(--fs-small);
    font-weight: 300;
    text-transform: none;
    letter-spacing: 0.02em;
    color: var(--muted);
  }
  .hv {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 12px);
    margin: calc(var(--u) * 6px) 0 calc(var(--u) * 8px);
  }
  .hv b {
    font-weight: 300;
    font-size: calc(var(--u) * 72px);
    line-height: 1;
    letter-spacing: -0.01em;
    font-variant-numeric: tabular-nums;
    color: var(--white);
    text-shadow:
      0 0 14px rgba(255, 176, 0, 0.6),
      0 0 36px rgba(255, 106, 26, 0.35);
  }
  .hv .u {
    font-size: calc(var(--u) * 20px);
    font-weight: 400;
    letter-spacing: 0.06em;
    color: var(--amber);
  }
  /* A dark backing like the labels': the VRAM rings pass behind this line. */
  .hs {
    display: block;
    width: fit-content;
    max-width: 100%;
    position: relative;
    left: calc(var(--u) * -6px);
    padding: 1px calc(var(--u) * 6px);
    background: rgba(5, 3, 2, 0.6);
    font-size: var(--fs-data);
    font-weight: 300;
    color: var(--hot);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hs.amb {
    color: var(--ember);
  }
  .hero.dim .hv b {
    color: #6b4210;
    text-shadow: none;
  }
  .hero.red .hl,
  .hero.red .hs,
  .hero.red .hv b {
    color: var(--red);
    text-shadow: 0 0 16px rgba(255, 59, 48, 0.5);
  }
  .hero.red .hs {
    white-space: normal;
    max-width: calc(var(--u) * 620px);
  }
  /* Projected labels: a light amber chip, words in Archivo Expanded caps, numbers in mono. */
  .lab {
    position: absolute;
    left: 0;
    top: 0;
    pointer-events: none;
    font-size: max(10px, calc(var(--u) * 10.5px));
    color: var(--hot);
    white-space: nowrap;
    will-change: transform;
  }
  .lab span {
    display: inline-block;
    transform: translate(10px, -50%);
    padding: 2px 7px 2px 6px;
    border-left: 1px solid rgba(255, 176, 0, 0.75);
    background: rgba(5, 3, 2, 0.45);
    opacity: 0.92;
  }
  .lab span :global(span) {
    display: inline;
    padding: 0;
    border: 0;
    background: none;
    transform: none;
  }
  .lab.kv span {
    color: var(--white);
    border-color: rgba(255, 244, 222, 0.85);
  }
  .lab.vr span {
    color: var(--amber);
  }
  .lab.vr.red span {
    color: var(--red);
    border-color: var(--red);
  }
  .lab.unk span {
    color: #c08a3c;
    border-left-style: dashed;
  }

  /* Detail rows */
  .rows {
    height: calc(var(--u) * 56px);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border: 1px solid var(--rule);
    background: var(--panel);
  }
  .rows.flt {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) minmax(0, 2fr);
    border-color: rgba(255, 59, 48, 0.7);
    background: linear-gradient(180deg, #1a0604, var(--panel));
  }
  .kv {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 6px);
    padding: 0 calc(var(--u) * 12px);
    border-right: 1px solid var(--rule);
    min-width: 0;
  }
  .kv:last-child {
    border-right: 0;
  }
  .kv .k {
    font-size: var(--fs-label);
    letter-spacing: 0.18em;
    color: var(--ember);
  }
  .kv .v {
    color: var(--white);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v {
    color: #c08a3c;
  }
  .rows.flt .k {
    color: var(--red);
  }
  .rows.flt .kv {
    border-color: rgba(255, 59, 48, 0.3);
  }
  .rows.flt .v.mono {
    font-size: var(--fs-small);
    color: var(--hot);
  }

  /* Request timeline / recent jobs */
  .tline {
    height: calc(var(--u) * 70px);
    display: flex;
    flex-direction: column;
    gap: calc(var(--u) * 10px);
    padding: calc(var(--u) * 9px) calc(var(--u) * 12px) calc(var(--u) * 8px);
    border: 1px solid var(--rule);
    background: var(--panel);
  }
  .cap {
    font-size: var(--fs-label);
    letter-spacing: 0.18em;
    color: var(--ember);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cap .dim {
    font-family: var(--mono);
    font-stretch: 100%;
    font-size: var(--fs-small);
    font-weight: 300;
    text-transform: none;
    color: var(--muted);
    letter-spacing: 0.02em;
  }
  .reqs {
    flex: 1;
    display: flex;
    gap: calc(var(--u) * 10px);
    align-items: center;
  }
  .req {
    flex: 1 1 0;
    display: flex;
    height: calc(var(--u) * 14px);
    border: 1px solid var(--rule2);
    min-width: 0;
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
  .req.failed,
  .job.failed {
    border: 1px dashed var(--red);
    background: repeating-linear-gradient(-45deg, rgba(255, 59, 48, 0.55) 0 1px, transparent 1px 6px);
  }
  .jobs {
    flex: 1;
    display: flex;
    align-items: flex-end;
    gap: calc(var(--u) * 8px);
    min-height: 0;
  }
  .job {
    flex: 1 1 0;
    height: var(--h);
    min-height: 3px;
    background: var(--amber);
    box-shadow: 0 0 8px rgba(255, 176, 0, 0.35);
  }
  .job.edit {
    background: repeating-linear-gradient(-45deg, var(--amber) 0 3px, #7a4408 3px 6px);
  }
  .job.empty {
    height: 3px;
    background: none;
    box-shadow: none;
    border-top: 1px dashed var(--rule2);
  }
  .job.failed {
    height: 100%;
  }

  /* Controls: every place keeps one width (in em, so it follows the type), nothing beside it moves. */
  .ctrl {
    height: calc(var(--u) * 46px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 6px);
    padding: 0 calc(var(--u) * 8px);
    border: 1px solid var(--rule);
    background: var(--panel);
  }
  .chip,
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: calc(var(--u) * 6px);
    height: calc(var(--u) * 30px);
    padding: 0 calc(var(--u) * 10px);
    border: 1px solid var(--rule2);
    color: #d7962e;
    white-space: nowrap;
    flex: none;
  }
  .chip {
    font-size: var(--fs-small);
  }
  .chip em {
    font-style: normal;
    font-weight: 300;
    color: var(--muted);
    margin-left: calc(var(--u) * 4px);
  }
  .chip em.nk {
    margin-left: 0;
  }
  .btn {
    font-size: var(--fs-label);
    letter-spacing: 0.16em;
    padding: 0;
  }
  .btn.w4 {
    width: 6.4em;
  }
  .btn.w7 {
    width: 9em;
  }
  .btn.w8 {
    width: 10em;
  }
  /* Records: the label, or (window too narrow for the row to hold it) just the icon */
  .btn.rec .ri {
    display: none;
    width: calc(var(--u) * 15px);
    height: calc(var(--u) * 15px);
    min-width: 12px;
    min-height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  @media (max-width: 939px) {
    .btn.rec {
      width: calc(var(--u) * 30px);
    }
    .btn.rec .rl {
      display: none;
    }
    .btn.rec .ri {
      display: block;
    }
  }
  .chip:hover:not(:disabled),
  .btn:hover:not(:disabled) {
    border-color: var(--amber);
    color: var(--white);
  }
  .chip:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .btn:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .btn.act {
    width: 15.5em;
    overflow: hidden;
  }
  .btn.go {
    background: var(--amber);
    border-color: var(--amber);
    color: #1a0d00;
    text-shadow: none;
    box-shadow: 0 0 20px rgba(255, 176, 0, 0.4);
  }
  .btn.go:hover:not(:disabled) {
    color: #1a0d00;
    background: var(--hot);
  }
  .btn.go:disabled {
    opacity: 1;
    background: #1a0e03;
    border-color: var(--rule2);
    color: var(--muted);
    box-shadow: none;
  }
  .btn.ext,
  .btn.ext:disabled {
    opacity: 1;
    background: transparent;
    border-color: var(--rule2);
    color: var(--muted);
  }
  .btn.stop {
    border-color: var(--ember);
    color: var(--ember);
  }
  .btn.hot {
    background: var(--red);
    border-color: var(--red);
    color: #1a0300;
    text-shadow: none;
  }
  .btn.hot:hover {
    color: #1a0300;
    background: #ff6a5c;
  }
  .cline {
    height: calc(var(--u) * 22px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8px);
    font-family: var(--mono);
    font-size: var(--fs-small);
    font-weight: 300;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-align: left;
  }
  .cline .lt {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cline:hover .lt {
    color: var(--hot);
  }
  @media (prefers-reduced-motion: reduce) {
    .caret {
      animation: none;
    }
  }
</style>
