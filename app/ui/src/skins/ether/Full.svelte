<script lang="ts">
  // Ether, full window: a gallery in the void. The cloud fills the whole window; the UI is fine engraving on
  // glass over it (hairlines only, no boxes). One fixed skeleton, scaled by --u (1 design px at 1024x1152),
  // the same in every phase (only the contents change):
  //   header      KLIF ETHER · status · uptime (version when idle) · PANEL · window controls
  //   system tabs one per System, model under each; selected = gradient underline, status dot before the label
  //   model line  name · quant · backend · device · ctx · kv
  //   stage       the cloud shows through; the hero and its facts on the right, over a soft scrim
  //   VRAM        a hairline with the gradient fill (idle: the selected tier's fit)
  //   detail      four cells (LLM: prefill, decode, context, speculative; image: last image, images, size, mode)
  //   timeline    the last 8 requests as capsules / the last 12 jobs as bars
  //   controls    endpoint + key chips, the primary act (one width), Restart, Tune, Records, Endpoint, Console
  //   console     the last log line
  import type { Actions, RequestRecord, ViewModel } from '../../lib/model/types';
  import { EXTERNAL_NOTE, EXTERNAL_TITLE, canLaunch, canStop, doLaunch, idleState, isPendingLaunch, launchCtl, selectedSystem, systemLabel } from '../../lib/model/systems';
  import { strip as scrollStrip } from '../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../lib/shell/SystemTabs/tabs';
  import { fmtClock, fmtCtx, fmtGiB, fmtInt, fmtPct, fmtSeconds } from '../../lib/model/format';
  import Cloud from './Cloud.svelte';
  import WinCtl from './WinCtl.svelte';
  import { factsOf, heroOf, type Ctx } from './hero';
  import { held, useSleep } from './state.svelte';
  import { availabilityText, fitOf, modelLine, modelShort } from './text';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  let w = $state(1024);
  let h = $state(1152);
  const u = $derived(Math.max(0.72, Math.min(1.25, Math.min(w / 1024, h / 1152))));
  // The cloud's centre at 40% of the window width (the original composition has it 0.9 screen heights from the
  // left edge) and at the middle of the stage (originally the middle of the window).
  const shift = $derived(0.9 - (0.4 * w) / Math.max(1, h));
  let stageEl = $state<HTMLElement | null>(null);
  let stageMid = $state(0.45);
  $effect(() => {
    void u;
    const el = stageEl;
    const hh = h;
    if (el && hh > 0) stageMid = (el.offsetTop + el.offsetHeight / 2) / hh;
  });
  const lift = $derived(0.5 - stageMid);

  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const sel = $derived(selectedSystem(vm) ?? undefined);
  const tabs = $derived(tabsFor(vm));
  const kind = $derived(sel?.kind ?? 'llm');
  const model = $derived(s?.model ?? sel?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);

  const loading = $derived(phase === 'starting' || phase === 'loading');
  const busy = $derived(loading || phase === 'stopping');
  const faulted = $derived(phase === 'fault');
  const online = $derived(phase === 'live');
  /** The launch control: with conflicts it reads "Stop S1 & launch" and sends stopOthers. */
  const ctl = $derived(launchCtl(vm, sel, { short: true }));
  const mine = $derived(!!sel && sel.controllable && !sel.external);
  const heldWhy = $derived(sel?.external ? 'External server: it runs where it was started.' : !sel?.controllable ? 'This node does not allow launching.' : '');
  const frameless = $derived(!!vm.host?.frameless);
  const panel = $derived(vm.host?.panel?.available ? vm.host.panel : null);
  const panelTip = $derived(`Panel mode: show on the small screen${panel?.target ? ` (${panel.target})` : ''}`);

  const sleep = useSleep(() => vm);
  const dz = $derived(online ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);
  const tps = held(() => vm.session?.llm?.decodeTps ?? 0);
  const prefilling = $derived(!!llm?.prefill && llm.activity === 'prefill');

  const ctx = $derived<Ctx>({ vm, s, kind, sel, model, dz, waking, tps: tps.current, compact: false });
  const hero = $derived(heroOf(ctx));
  const facts = $derived(factsOf(ctx, 3));

  // ---- header -------------------------------------------------------------------------------------------------
  const statusText = $derived(dz ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : s ? phase.toUpperCase() : 'IDLE');
  const statusTone = $derived(faulted ? 'red' : dz ? (waking ? 'sleep pulse' : 'sleep') : busy ? 'live pulse' : s ? 'live' : 'off');
  const clock = $derived(s ? fmtClock(loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS) : vm.host?.appVersion ? `v${vm.host.appVersion}` : '');

  // ---- detail strip: four fixed cells per kind ---------------------------------------------------------------
  const strip = $derived.by<[string, string][]>(() => {
    if (kind === 'image') {
      const j = img?.recent[img.recent.length - 1];
      return [
        ['last image', j ? `${fmtSeconds(j.seconds)}${j.edit ? ' · edit' : ''}` : img ? 'none yet' : '—'],
        ['images', img ? fmtInt(img.imagesThisSession) : '—'],
        ['size', model?.imageSize ?? '—'],
        ['mode', model?.mode ?? '—'],
      ];
    }
    if (kind !== 'llm') {
      return [
        ['requests', gen?.requestsTotal !== undefined ? fmtInt(gen.requestsTotal) : '—'],
        ['in flight', gen ? fmtInt(gen.requestsInFlight ?? 0) : '—'],
        ['model', model?.name ?? '—'],
        ['reports', gen?.modelId ?? '—'],
      ];
    }
    const pf = llm?.prefill;
    const total = llm?.context.totalTokens || model?.ctxTokens || 0;
    const kv = model?.kvType ? ` · kv ${model.kvType}` : '';
    return [
      ['prefill', pf ? (prefilling ? `${fmtInt(pf.tokens)} tok · ${fmtSeconds(pf.elapsedS)}` : `${fmtInt(pf.tokens)} tok · ${fmtInt(pf.tps)} tok/s`) : llm ? 'no request yet' : '—'],
      ['decode', llm ? `${fmtInt(llm.totals.generatedTokens)} tok · ${fmtInt(llm.totals.requests)} req` : '—'],
      ['context', llm && total ? `${fmtPct(llm.context.usedTokens / total)} full${kv}` : total ? `${fmtCtx(total)}${kv}` : '—'],
      ['speculative', llm?.spec ? `${llm.spec.mode}${llm.spec.active ? ' · drafting' : ''}` : (model?.specMode ?? 'off')],
    ];
  });

  // ---- VRAM hairline -------------------------------------------------------------------------------------------
  const vtotal = $derived(Math.max(0.01, vm.vram.totalGiB));
  const pct = (gib: number) => (Math.min(vtotal, Math.max(0, gib)) / vtotal) * 100;
  const fit = $derived(!s ? fitOf(vm.vram, sel) : null);
  const spill = $derived(vm.vram.spillMiB / 1024);
  const lowFree = $derived(!!s && vm.vram.totalGiB - vm.vram.usedGiB < vm.vram.warnBelowGiB);
  const vramNote = $derived.by(() => {
    if (fit) return fit.spare >= 0 ? { t: `fits · ${fmtGiB(fit.spare)} GiB spare`, tone: '' } : { t: `over by ${fmtGiB(-fit.spare)} GiB`, tone: 'red' };
    if (spill > 0) return { t: `spill ${fmtGiB(spill)} GiB`, tone: 'red' };
    if (dz) return { t: `${fmtGiB(dz.pagedOutGiB)} GiB paged out`, tone: '' };
    if (lowFree) return { t: `${fmtGiB(vm.vram.totalGiB - vm.vram.usedGiB)} GiB free`, tone: 'warn' };
    return { t: '', tone: '' };
  });

  // ---- timeline: the last 8 requests (a fault marks the last place) / the last 12 jobs ---------------------------
  const reqN = $derived(faulted && llm ? 7 : 8);
  const requests = $derived(llm ? llm.requests.slice(-reqN) : []);
  const reqSlots = $derived(Array.from({ length: reqN }, (_, i) => requests[i - (reqN - requests.length)] ?? null));
  const jobN = $derived(faulted && img ? 11 : 12);
  const jobs = $derived(img ? img.recent.slice(-jobN) : []);
  const jobSlots = $derived(Array.from({ length: jobN }, (_, i) => jobs[i - (jobN - jobs.length)] ?? null));
  const maxJob = $derived(Math.max(1, ...jobs.map((j) => j.seconds)));
  const diedInWork = $derived(faulted && ((!!llm && llm.activity !== 'idle') || (!!img && img.activity === 'generating') || (gen?.requestsInFlight ?? 0) > 0));
  const tlNote = $derived.by(() => {
    if (!s) return 'not running';
    if (loading) return kind === 'image' ? 'no jobs yet' : 'no requests yet';
    if (kind !== 'llm' && kind !== 'image') return faulted ? 'failed' : 'no per-request log for this kind';
    const what = kind === 'image' ? 'jobs' : 'requests';
    const n = kind === 'image' ? jobs.length : requests.length;
    if (faulted) {
      if (!llm && !img) return `no ${what}: failed during startup`;
      return `${n ? `last ${n}` : `no finished ${what}`}, then ${diedInWork ? `the ${kind === 'image' ? 'job' : 'request'} that died` : 'the fault'}`;
    }
    if (!n) return `no finished ${what} yet`;
    return kind === 'image' ? `bar height = time · last ${n} of ${fmtInt(img?.imagesThisSession ?? n)}` : `last ${n} · prefill teal, decode blue`;
  });
  const reqTitle = (r: RequestRecord) =>
    `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${fmtSeconds(r.prefillS)} · ${fmtInt(r.generatedTokens)} tok in ${fmtSeconds(r.decodeS)}`;

  // ---- controls ------------------------------------------------------------------------------------------------
  const port = $derived(s?.endpoint.port ?? sel?.command?.port);
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const tabTitle = (t: (typeof tabs)[number]) => {
    const ts = t.system;
    const where = t.nodeName ? ` on ${t.nodeName}` : '';
    if (ts.status === 'not-set') return `${t.label}${where}: no preset. Open Tune to choose one.`;
    if (ts.availability !== 'ready' || ts.status === 'unreachable') return `${t.label}${where}: ${ts.reason ?? availabilityText(ts.availability)}`;
    // The same condition as the double-click handler: a launch with conflicts needs the "Stop X & launch" button.
    const hint = !canLaunch(ts) ? '' : ts.conflicts.length === 0 ? ', double-click to launch' : `, needs ${ts.conflicts.map((id) => systemLabel(vm, id)).join(' and ')} stopped`;
    return `${t.label}${where}: ${ts.model.name} (${ts.status}${hint})`;
  };
</script>

<div class="full" bind:clientWidth={w} bind:clientHeight={h} style="--u:{u}">
  <Cloud {vm} {shift} {lift} />
  <div class="veil top"></div>
  <div class="veil bottom"></div>

  <div class="ui">
    <!-- Header (window drag region; draws its own window controls when the host is frameless) -->
    <header class="hdr" class:frameless data-tauri-drag-region>
      <span class="wm" data-tauri-drag-region title="Koksny.com LOCAL INFERENCE FORNICATOR">KLIF</span>
      <span class="sk" data-tauri-drag-region>ETHER</span>
      <span class="st {statusTone}" data-tauri-drag-region><i></i>{statusText}</span>
      <span class="grow" data-tauri-drag-region></span>
      {#if clock}<span class="clk" class:ver={!s} data-tauri-drag-region>{clock}</span>{/if}
      {#if panel}
        <button class="tbtn pbtn" class:on={panel.active} onclick={() => actions.togglePanel()} title={panelTip} aria-label={panelTip} aria-pressed={panel.active}>PANEL</button>
      {/if}
      {#if frameless}<WinCtl {actions} maximized={!!vm.host?.maximized} />{/if}
    </header>
    <div class="hair"></div>

    <!-- System tabs: click selects (never stops anything), double-click launches a System that is ready and has
         nothing in its way. They scroll sideways; + adds a System. -->
    <div class="sysbar">
      <div class="tiers" role="tablist" aria-label="Systems" use:scrollStrip={vm.selected}>
        {#each tabs as t (t.id)}
          {@const ts = t.system}
          {@const selected = t.id === vm.selected}
          {@const na = ts.availability !== 'ready' && ts.status !== 'not-set'}
          <button
            class="tab"
            class:sel={selected}
            class:na
            role="tab"
            aria-selected={selected}
            title={tabTitle(t)}
            onclick={() => actions.select(t.id)}
            ondblclick={() => {
              if (canLaunch(ts) && ts.conflicts.length === 0) void actions.launch(ts.id).catch(() => {});
            }}
          >
            <span class="tl"><TabLabel tab={t} /></span>
            <span class="tm">{ts.status === 'not-set' ? 'no preset' : ts.status === 'unreachable' ? `${t.nodeName ?? 'node'} unreachable` : na ? `${ts.model.name} · ${availabilityText(ts.availability)}` : modelShort(ts.model)}</span>
          </button>
        {/each}
      </div>
      <button class="tab add" type="button" title="Add a System" aria-label="Add a System" onclick={() => actions.openTune(undefined, { add: true })}>+</button>
    </div>

    <div class="mline" title={modelLine(model, kind)}>{modelLine(model, kind)}</div>

    <!-- Stage: the cloud shows through; the hero and its facts on the right -->
    <section class="stage" bind:this={stageEl}>
      <div class="col {hero.tone}">
        <div class="hl">{hero.label}</div>
        <div class="hv"><b>{hero.value}</b>{#if hero.unit}<span class="hu">{hero.unit}</span>{/if}</div>
        <div class="hs" class:warn={hero.warn} title={hero.sub}>{hero.sub}</div>
        <div class="hx">
          {#if hero.frac !== null}<div class="prog" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(hero.frac * 100)}><i style="width:{hero.frac * 100}%"></i></div>{/if}
          {#if hero.steps?.length}
            <div class="steps">
              {#each hero.steps as st (st.id)}<i class={st.state} title="{st.label}{st.detail ? ` · ${st.detail}` : ''}"></i>{/each}
            </div>
          {/if}
          {#if hero.log.length}
            <div class="log">{#each hero.log as line, i (i)}<div title={line}>{line}</div>{/each}</div>
          {/if}
        </div>
        <div class="facts" class:dim={!s}>
          {#each facts as [k, v] (k)}<div class="fact"><span class="k">{k}</span><span class="v" title={v}>{v}</span></div>{/each}
        </div>
      </div>
    </section>

    <!-- VRAM -->
    <div class="vram">
      <span class="k">VRAM</span>
      <span class="track">
        {#if dz}
          <span class="fill sleep" style="width:{pct(vm.vram.usedGiB)}%"></span>
          <span class="paged" style="left:{pct(vm.vram.usedGiB)}%; width:{pct(vm.vram.usedGiB + dz.pagedOutGiB) - pct(vm.vram.usedGiB)}%"></span>
        {:else}
          <span class="fill" class:red={spill > 0} class:warn={lowFree && spill <= 0} style="width:{pct(vm.vram.usedGiB)}%"></span>
        {/if}
        {#if fit}<span class="ghost" class:over={fit.spare < 0} style="left:{pct(fit.base)}%; width:{pct(fit.top) - pct(fit.base)}%"></span>{/if}
      </span>
      <span class="v"><b>{fmtGiB(vm.vram.usedGiB)}</b> / {fmtGiB(vm.vram.totalGiB)} GiB{#if vramNote.t}<em class={vramNote.tone}>{` · ${vramNote.t}`}</em>{/if}</span>
    </div>

    <!-- Detail strip -->
    <section class="strip" class:dim={!s}>
      {#each strip as [k, v] (k)}<div class="cell"><span class="k">{k}</span><span class="v" title={v}>{v}</span></div>{/each}
    </section>

    <!-- Request timeline / recent jobs -->
    <section class="tline">
      <div class="cap"><span class="k">{kind === 'image' ? 'RECENT JOBS' : 'REQUESTS'}</span><span class="n">{tlNote}</span></div>
      {#if kind === 'image'}
        <div class="jobs">
          {#each jobSlots as j, i (i)}
            {#if j}
              <span class="job" style="--o:{0.4 + (0.6 * (i + 1)) / jobN}" title="{fmtSeconds(j.seconds)} · {j.width}x{j.height}{j.edit ? ' · edit' : ''}">
                <i class:edit={j.edit} style="height:{Math.max(6, (j.seconds / maxJob) * 100)}%"></i><em>{j.seconds.toFixed(0)}s</em>
              </span>
            {:else}<span class="job empty"><i></i><em>&nbsp;</em></span>{/if}
          {/each}
          {#if faulted && img}<span class="job failed" title={diedInWork ? 'The server died during this job' : 'The server died between jobs'}><i></i><em>fault</em></span>{/if}
        </div>
      {:else}
        <div class="reqs">
          {#each reqSlots as r, i (i)}
            {#if r}
              {@const tot = r.prefillS + r.decodeS}
              <span class="req" style="--o:{0.35 + (0.65 * (i + 1)) / reqN}" title={reqTitle(r)}>
                <span class="cap6"><i class="pre" style="width:{tot > 0 ? (r.prefillS / tot) * 100 : 0}%"></i><i class="dec"></i></span>
                <em>{fmtInt(r.generatedTokens)} tok</em>
              </span>
            {:else}<span class="req empty"><span class="cap6"></span><em>&nbsp;</em></span>{/if}
          {/each}
          {#if faulted && llm}<span class="req failed" title={diedInWork ? 'The server died during this request' : 'The server died with no request in flight'}><span class="cap6"></span><em>fault</em></span>{/if}
        </div>
      {/if}
    </section>

    <!-- Controls: the primary act keeps one width, so nothing beside it moves -->
    <footer class="ctrl">
      <button class="chip" onclick={() => actions.copyEndpoint(sel?.id)} disabled={!online} title={online && s ? `Copy http://${s.endpoint.host}:${s.endpoint.port}` : 'Endpoint offline'}>
        :{port ?? '—'}{#if !online}<em>offline</em>{/if}
      </button>
      <button class="chip" onclick={() => actions.copyApiKey()} disabled={!online || !s?.apiKeySet} title={s?.apiKeySet ? 'Copy the API key' : 'No API key set'}>
        {#if s?.apiKeySet}••••••<em>copy</em>{:else}<em class="nk">no key</em>{/if}
      </button>
      <span class="grow"></span>
      {#if sel?.external}
        <button class="act ext" disabled title={EXTERNAL_TITLE}>{EXTERNAL_NOTE}</button>
      {:else if !s && isPendingLaunch(sel)}
        <button class="act stop" onclick={() => actions.stop(sel?.id)} disabled={!canStop(sel)} title={sel?.reason ?? 'Cancel the launch'}>Cancel</button>
      {:else if !s}
        <button class="act go" onclick={() => doLaunch(actions, sel, ctl)} disabled={!ctl.enabled} title={ctl.enabled ? (ctl.stopOthers ? `${ctl.text}: ${sel?.reason ?? ''}` : `Launch ${sel?.label ?? ''}`) : `${sel?.label ?? ''}: ${ctl.blocked}`}>
          {ctl.stopOthers ? ctl.text : `Launch ${sel?.label ?? ''}`}
        </button>
      {:else if faulted}
        <button class="act hot" onclick={() => actions.restart(sel?.id)} disabled={!mine} title="Launch {sel?.label ?? 'the System'} again">Restart {sel?.label ?? ''}</button>
      {:else if phase === 'stopping'}
        <button class="act stop" disabled>Stopping</button>
      {:else}
        <button class="act stop" onclick={() => actions.stop(sel?.id)} disabled={!canStop(sel)} title={heldWhy || (loading ? 'Cancel the launch' : 'Stop the server')}>{loading ? 'Cancel' : `Stop ${sel?.label ?? ''}`}</button>
      {/if}
      {#if faulted}
        <button class="tbtn w1" onclick={() => actions.dismiss(sel?.id)} disabled={!mine} title="Back to the launcher">Dismiss</button>
      {:else}
        <button class="tbtn w1" onclick={() => actions.restart(sel?.id)} disabled={!s || busy || !mine} title={heldWhy || 'Stop and launch again with the current settings'}>Restart</button>
      {/if}
      <button class="tbtn w2" onclick={() => actions.openTune(sel?.id)} title={s && !faulted ? 'Change the settings; Restart to apply them' : 'Change what this System launches'}>Tune</button>
      <button class="tbtn w1" onclick={() => actions.openRecords()} title="Records: the best each model reached">Records</button>
      <button class="tbtn w3" onclick={() => actions.openEndpoint(sel?.id)} disabled={!online} title={kind === 'image' ? 'Open the sd-server web UI' : 'Open the endpoint'}>{kind === 'image' ? 'Web UI' : 'Endpoint'}</button>
      <button class="tbtn w3" onclick={() => actions.toggleConsole(faulted ? true : undefined)}>{faulted ? 'Full log' : 'Console'}</button>
    </footer>
    <button class="cline" class:hot={faulted} onclick={() => actions.toggleConsole(true)} title="Open the console">{lastLine || ' '}</button>
  </div>
</div>

<style>
  .full {
    --ink: #ede8f5;
    --muted: #9a90a8;
    --sec: rgba(237, 232, 245, 0.5);
    --faint: rgba(237, 232, 245, 0.1);
    --blue: #7ab0ff; /* UI accents; pink stays only in the background and the blue-to-pink gradient bars */
    --teal: #4fc3d9;
    --warn: #ffb347;
    --danger: #ff5470;
    --grad: linear-gradient(90deg, #4fc3d9, #ff4fd8);
    --disp: 'Segoe UI Variable Display', 'Segoe UI', -apple-system, system-ui, sans-serif;
    --text: 'Segoe UI Variable Text', 'Segoe UI', -apple-system, system-ui, sans-serif;
    --data: 'Iosevka', ui-monospace, monospace;
    --pad: calc(var(--u) * 36px);
    --fs-cap: max(10px, calc(var(--u) * 11px));
    --fs-cap2: max(10.5px, calc(var(--u) * 12px));
    --fs-d11: max(10px, calc(var(--u) * 11px));
    --fs-d12: max(10.5px, calc(var(--u) * 12px));
    --fs-d13: max(11px, calc(var(--u) * 13px));
    --fs-d16: max(13px, calc(var(--u) * 16px));
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: #000;
    color: var(--ink);
    font-family: var(--text);
    font-variant-numeric: tabular-nums;
    user-select: none;
  }
  .veil {
    position: absolute;
    left: 0;
    right: 0;
    pointer-events: none;
  }
  /* header, tabs and model line: the cloud grows up here under prefill pressure */
  .veil.top {
    top: 0;
    height: calc(var(--u) * 240px);
    background: linear-gradient(180deg, rgba(0, 0, 0, 0.82), rgba(0, 0, 0, 0.72) 55%, rgba(0, 0, 0, 0.3) 80%, transparent);
  }
  .veil.bottom {
    bottom: 0;
    height: calc(var(--u) * 420px);
    background: linear-gradient(180deg, transparent, rgba(0, 0, 0, 0.5) 22%, rgba(0, 0, 0, 0.8) 45%, rgba(0, 0, 0, 0.9));
  }
  .ui {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    padding: 0 var(--pad) calc(var(--u) * 10px);
  }
  .ui > :global(*) {
    flex: none;
    min-width: 0;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    margin: 0;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 1px solid var(--teal);
    outline-offset: 3px;
  }
  .grow {
    flex: 1 1 auto;
    align-self: stretch;
  }
  .red {
    color: var(--danger);
  }
  @keyframes pulse {
    50% {
      opacity: 0.25;
    }
  }

  /* ---- header ---- */
  .hdr {
    height: calc(var(--u) * 48px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 16px);
    margin: 0 calc(var(--pad) * -1);
    padding: 0 var(--pad);
    white-space: nowrap;
  }
  .hdr.frameless {
    padding-right: 0;
  }
  .wm {
    font: 600 max(13px, calc(var(--u) * 15px)) / 1 var(--disp);
    letter-spacing: 0.32em;
    color: var(--ink);
  }
  .sk {
    font: 300 var(--fs-cap) / 1 var(--text);
    letter-spacing: 0.32em;
    color: var(--sec);
    padding-left: calc(var(--u) * 16px);
    border-left: 1px solid var(--faint);
    line-height: calc(var(--u) * 16px);
  }
  .st {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 8px);
    margin-left: calc(var(--u) * 8px);
    font: 400 var(--fs-cap) / 1 var(--text);
    letter-spacing: 0.2em;
    color: var(--sec);
  }
  .st i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
  }
  .st.live {
    color: var(--ink);
  }
  .st.live i {
    background: var(--blue);
    box-shadow: 0 0 8px var(--blue), 0 0 2px var(--blue);
  }
  .st.sleep i {
    background: rgba(154, 144, 168, 0.4);
  }
  .st.red {
    color: var(--danger);
  }
  .st.red i {
    background: var(--danger);
    box-shadow: 0 0 8px rgba(255, 84, 112, 0.7);
  }
  .st.pulse i {
    animation: pulse 1.4s ease-in-out infinite;
  }
  .clk {
    font: 300 var(--fs-d13) / 1 var(--data);
    color: var(--ink);
    letter-spacing: 0.04em;
  }
  .clk.ver {
    color: var(--sec);
  }
  .tbtn {
    position: relative;
    height: calc(var(--u) * 28px);
    font: 400 var(--fs-cap) / 1 var(--text);
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--sec);
    white-space: nowrap;
  }
  .tbtn::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0.2em;
    bottom: calc(var(--u) * 3px);
    height: 1px;
    background: var(--grad);
    transform: scaleX(0);
    transform-origin: left;
    transition: transform 0.18s ease;
  }
  .tbtn:hover:not(:disabled) {
    color: var(--ink);
  }
  .tbtn:hover:not(:disabled)::after {
    transform: scaleX(1);
  }
  .tbtn:disabled {
    opacity: 0.32;
    cursor: default;
  }
  .pbtn.on {
    color: var(--ink);
  }
  .pbtn.on::after {
    transform: scaleX(1);
  }
  .hdr.frameless .pbtn {
    margin-right: calc(var(--u) * 6px);
  }
  .hair {
    height: 1px;
    margin: 0 calc(var(--pad) * -1);
    background: var(--grad);
    opacity: 0.25;
  }

  /* ---- tier tabs ---- */
  .sysbar {
    margin-top: calc(var(--u) * 20px);
    height: calc(var(--u) * 50px);
    display: flex;
    column-gap: calc(var(--u) * 28px);
    border-bottom: 1px solid var(--faint);
    min-width: 0;
  }
  .tiers {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    column-gap: calc(var(--u) * 28px);
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tiers::-webkit-scrollbar {
    display: none;
  }
  .tiers .tab {
    flex: 1 0 calc(var(--u) * 150px);
  }
  .tab.add {
    flex: none;
    width: calc(var(--u) * 30px);
    align-items: center;
    font: 300 calc(var(--u) * 22px) / 1 var(--text);
    color: var(--sec);
  }
  .tab.add:hover {
    color: var(--ink);
  }
  .tab {
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: flex-start;
    gap: calc(var(--u) * 7px);
    padding-top: calc(var(--u) * 4px);
    text-align: left;
    min-width: 0;
  }
  .tab .tl {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 9px);
    font: 400 var(--fs-cap2) / 1 var(--text);
    letter-spacing: 0.2em;
    color: var(--sec);
    white-space: nowrap;
  }
  .tab .tm {
    font: 300 var(--fs-d11) / 1.2 var(--data);
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tab::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: -1px;
    height: 2px;
    background: var(--grad);
    opacity: 0;
  }
  .tab.sel .tl {
    color: var(--ink);
  }
  .tab.sel .tm {
    color: rgba(237, 232, 245, 0.7);
  }
  .tab.sel::after {
    opacity: 1;
  }
  .tab:not(.sel):hover .tl {
    color: var(--ink);
  }
  .tab.na {
    opacity: 0.4;
  }

  /* ---- model line ---- */
  .mline {
    margin-top: calc(var(--u) * 14px);
    height: calc(var(--u) * 18px);
    font: 300 var(--fs-d13) / calc(var(--u) * 18px) var(--data);
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ---- stage: the hero column at a fixed size, centred in whatever height the window leaves ---- */
  .stage {
    position: relative;
    flex: 1 1 0 !important;
    min-height: calc(var(--u) * 470px);
    margin-top: calc(var(--u) * 8px);
  }
  .col {
    position: absolute;
    right: 0;
    top: 50%;
    width: calc(var(--u) * 460px);
    height: calc(var(--u) * 412px);
    transform: translateY(-50%);
    display: grid;
    grid-template-rows:
      calc(var(--u) * 18px) calc(var(--u) * 136px) calc(var(--u) * 40px)
      calc(var(--u) * 74px) minmax(0, 1fr);
    row-gap: calc(var(--u) * 8px);
    text-align: right;
    isolation: isolate;
  }
  /* the scrim: soft darkness behind the hero so text never sits on bright plasma */
  .col::before {
    content: '';
    position: absolute;
    z-index: -1;
    inset: calc(var(--u) * -100px) calc(var(--pad) * -1 - 10px) calc(var(--u) * -80px) calc(var(--u) * -200px);
    background: radial-gradient(closest-side, rgba(0, 0, 0, 0.74), rgba(0, 0, 0, 0.6) 50%, rgba(0, 0, 0, 0.3) 78%, rgba(0, 0, 0, 0) 100%);
    pointer-events: none;
  }
  /* small text over the plasma keeps a soft dark halo (the hero figure keeps its own glow) */
  .hl,
  .hs,
  .hx,
  .facts,
  .tiers,
  .mline {
    text-shadow:
      0 0 10px rgba(0, 0, 0, 0.9),
      0 0 3px rgba(0, 0, 0, 0.8);
  }
  .hl {
    align-self: end;
    font: 400 var(--fs-cap2) / 1 var(--text);
    letter-spacing: 0.28em;
    color: var(--teal);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hv {
    display: flex;
    justify-content: flex-end;
    align-items: baseline;
    gap: calc(var(--u) * 12px);
    white-space: nowrap;
    align-self: center;
  }
  .hv b {
    font: 200 calc(var(--u) * 132px) / 1 var(--disp);
    letter-spacing: -0.02em;
    color: var(--ink);
    text-shadow: 0 0 40px rgba(122, 176, 255, 0.22);
    font-variant-numeric: tabular-nums;
  }
  .hu {
    font: 300 max(13px, calc(var(--u) * 18px)) / 1 var(--data);
    color: var(--muted);
  }
  .hs {
    font: 300 var(--fs-d13) / 1.45 var(--data);
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
  .hx {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: calc(var(--u) * 12px);
    min-height: 0;
    overflow: hidden;
  }
  .prog {
    position: relative;
    width: 100%;
    height: 1px;
    margin-top: calc(var(--u) * 4px);
    background: var(--faint);
    flex: none;
  }
  .prog i {
    position: absolute;
    left: 0;
    top: -0.5px;
    height: 2px;
    background: var(--grad);
    box-shadow: 0 0 8px rgba(255, 79, 216, 0.45);
    transition: width 0.4s ease-out;
  }
  .steps {
    display: flex;
    gap: calc(var(--u) * 11px);
  }
  .steps i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px rgba(237, 232, 245, 0.3);
  }
  .steps i.done {
    background: var(--ink);
    box-shadow: none;
  }
  .steps i.active {
    background: var(--blue);
    box-shadow: 0 0 8px var(--blue);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .steps i.failed {
    background: var(--danger);
    box-shadow: none;
  }
  .log {
    width: 100%;
    display: grid;
    row-gap: calc(var(--u) * 3px);
    font: 300 max(10px, calc(var(--u) * 11.5px)) / 1.35 var(--data);
    color: rgba(255, 84, 112, 0.66);
  }
  .log div {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .facts {
    align-self: start;
    border-top: 1px solid var(--faint);
  }
  .fact {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: calc(var(--u) * 18px);
    height: calc(var(--u) * 38px);
    line-height: calc(var(--u) * 38px);
    border-bottom: 1px solid var(--faint);
  }
  .fact .k {
    flex: none;
    font-family: var(--text);
    font-weight: 400;
    font-size: var(--fs-cap);
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--sec);
  }
  .fact .v {
    min-width: 0;
    font-family: var(--data);
    font-weight: 300;
    font-size: var(--fs-d13);
    color: var(--ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .facts.dim .v {
    color: rgba(237, 232, 245, 0.62);
  }
  .col.dim .hv b {
    opacity: 0.42;
    text-shadow: none;
  }
  .col.red .hl {
    color: var(--danger);
  }
  .col.red .hv b {
    color: var(--danger);
    text-shadow: 0 0 40px rgba(255, 84, 112, 0.3);
  }
  .col.red .hs {
    color: rgba(255, 190, 200, 0.85);
  }

  /* ---- VRAM ---- */
  .vram {
    margin-top: calc(var(--u) * 10px);
    height: calc(var(--u) * 24px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 18px);
  }
  .vram .k {
    flex: none;
    font: 400 var(--fs-cap) / 1 var(--text);
    letter-spacing: 0.22em;
    color: var(--sec);
  }
  .track {
    position: relative;
    flex: 1 1 auto;
    height: 1px;
    background: var(--faint);
  }
  .track .fill {
    position: absolute;
    left: 0;
    top: -1px;
    height: 3px;
    border-radius: 2px;
    background: var(--grad);
    box-shadow: 0 0 10px rgba(255, 79, 216, 0.45);
    transition: width 0.4s ease-out;
  }
  .track .fill.warn {
    background: var(--warn);
    box-shadow: 0 0 10px rgba(255, 179, 71, 0.4);
  }
  .track .fill.red {
    background: var(--danger);
    box-shadow: 0 0 10px rgba(255, 84, 112, 0.5);
  }
  .track .fill.sleep {
    background: var(--grad);
    opacity: 0.35;
    box-shadow: none;
  }
  .track .paged {
    position: absolute;
    top: -2px;
    height: 5px;
    background: repeating-linear-gradient(-45deg, rgba(237, 232, 245, 0.32) 0 1px, transparent 1px 4px);
  }
  .track .ghost {
    position: absolute;
    top: -3px;
    height: 7px;
    border: 1px dashed rgba(237, 232, 245, 0.45);
    border-radius: 3px;
  }
  .track .ghost.over {
    border-color: var(--danger);
  }
  .vram .v {
    flex: none;
    font: 300 var(--fs-d13) / 1 var(--data);
    color: var(--muted);
    white-space: nowrap;
  }
  .vram .v b {
    font-weight: 300;
    color: var(--ink);
  }
  .vram .v em {
    font-style: normal;
  }
  .vram .v em.red {
    color: var(--danger);
  }
  .vram .v em.warn {
    color: var(--warn);
  }

  /* ---- detail strip ---- */
  .strip {
    margin-top: calc(var(--u) * 16px);
    height: calc(var(--u) * 62px);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border-top: 1px solid var(--faint);
  }
  .cell {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 7px);
    padding: 0 calc(var(--u) * 18px);
    border-left: 1px solid var(--faint);
    min-width: 0;
  }
  .cell:first-child {
    padding-left: 0;
    border-left: 0;
  }
  .cell .k {
    font: 400 var(--fs-cap) / 1 var(--text);
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--sec);
    white-space: nowrap;
  }
  .cell .v {
    font: 300 var(--fs-d16) / 1.1 var(--data);
    color: var(--ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .strip.dim .v {
    color: rgba(237, 232, 245, 0.6);
  }

  /* ---- timeline ---- */
  .tline {
    margin-top: calc(var(--u) * 6px);
    height: calc(var(--u) * 88px);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    row-gap: calc(var(--u) * 10px);
    padding-top: calc(var(--u) * 12px);
    border-top: 1px solid var(--faint);
  }
  .cap {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 14px);
    white-space: nowrap;
    overflow: hidden;
  }
  .cap .k {
    font: 400 var(--fs-cap) / 1 var(--text);
    letter-spacing: 0.2em;
    color: var(--sec);
  }
  .cap .n {
    font: 300 var(--fs-d11) / 1 var(--data);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .reqs,
  .jobs {
    display: flex;
    gap: calc(var(--u) * 14px);
    min-height: 0;
  }
  .req {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 7px);
    opacity: var(--o, 1);
  }
  .req.failed {
    flex: 0 0 calc(var(--u) * 52px);
  }
  .cap6 {
    display: flex;
    height: 6px;
    border-radius: 3px;
    overflow: hidden;
  }
  .cap6 .pre {
    background: var(--teal);
    box-shadow: 0 0 8px rgba(79, 195, 217, 0.5);
  }
  .cap6 .dec {
    flex: 1;
    background: var(--blue);
    box-shadow: 0 0 8px rgba(122, 176, 255, 0.5);
  }
  .req.empty .cap6 {
    box-shadow: inset 0 0 0 1px var(--faint);
  }
  .req.failed .cap6 {
    border: 1px dashed var(--danger);
  }
  .req em,
  .job em {
    font: 300 max(9.5px, calc(var(--u) * 10px)) / 1 var(--data);
    font-style: normal;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .req.failed em,
  .job.failed em {
    color: var(--danger);
  }
  .job {
    flex: 1 1 0;
    min-width: 0;
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    justify-items: center;
    row-gap: calc(var(--u) * 4px);
    opacity: var(--o, 1);
  }
  .job i {
    align-self: end;
    width: 46%;
    min-height: 2px;
    border-radius: 2px 2px 0 0;
    background: var(--blue);
    box-shadow: 0 0 8px rgba(122, 176, 255, 0.35);
  }
  .job i.edit {
    background: repeating-linear-gradient(-45deg, var(--blue) 0 2px, rgba(122, 176, 255, 0.18) 2px 5px);
    box-shadow: inset 0 0 0 1px var(--blue);
  }
  .job.empty i {
    height: 1px;
    min-height: 1px;
    background: var(--faint);
    box-shadow: none;
  }
  .job.failed i {
    height: 100%;
    background: repeating-linear-gradient(-45deg, rgba(255, 84, 112, 0.55) 0 1px, transparent 1px 5px);
    box-shadow: inset 0 0 0 1px var(--danger);
  }

  /* ---- controls ---- */
  .ctrl {
    height: calc(var(--u) * 50px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 22px);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 8px);
    height: calc(var(--u) * 26px);
    padding: 0 calc(var(--u) * 13px);
    border: 1px solid rgba(237, 232, 245, 0.18);
    border-radius: 999px;
    font: 300 var(--fs-d12) / 1 var(--data);
    color: var(--ink);
    white-space: nowrap;
    flex: none;
  }
  .chip + .chip {
    margin-left: calc(var(--u) * -12px);
  }
  .chip em {
    font-style: normal;
    color: var(--muted);
  }
  .chip:hover:not(:disabled) {
    border-color: rgba(237, 232, 245, 0.42);
  }
  .chip:disabled {
    cursor: default;
    color: var(--muted);
  }
  /* the primary act: the gradient hairline as its border, one width for every phase */
  .act {
    position: relative;
    flex: none;
    width: calc(var(--u) * 220px);
    height: calc(var(--u) * 38px);
    border-radius: 999px;
    font: 500 var(--fs-cap2) / 1 var(--text);
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    background: rgba(122, 176, 255, 0.08);
  }
  .act::before {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    padding: 1px;
    background: var(--grad);
    -webkit-mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    pointer-events: none;
  }
  .act:hover:not(:disabled) {
    background: rgba(122, 176, 255, 0.16);
    box-shadow: 0 0 22px rgba(122, 176, 255, 0.18);
  }
  .act.stop {
    background: rgba(0, 0, 0, 0.35);
  }
  .act.stop:hover:not(:disabled) {
    background: rgba(237, 232, 245, 0.06);
  }
  .act.hot {
    background: rgba(255, 84, 112, 0.14);
  }
  .act.hot:hover:not(:disabled) {
    background: rgba(255, 84, 112, 0.24);
  }
  .act:disabled {
    cursor: default;
    opacity: 0.4;
  }
  .act.ext,
  .act.ext:disabled {
    opacity: 1;
    background: transparent;
    color: var(--muted);
  }
  .act.ext::before {
    background: var(--faint);
  }
  .ctrl .tbtn {
    text-align: center;
  }
  .ctrl .tbtn.w1 {
    width: calc(var(--u) * 78px);
  }
  .ctrl .tbtn.w2 {
    width: calc(var(--u) * 48px);
  }
  .ctrl .tbtn.w3 {
    width: calc(var(--u) * 86px);
  }
  .cline {
    height: calc(var(--u) * 22px);
    margin-top: calc(var(--u) * 2px);
    font: 300 var(--fs-d12) / calc(var(--u) * 22px) var(--data);
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
    opacity: 0.8;
  }
  .cline:hover {
    color: var(--ink);
    opacity: 1;
  }
  .cline.hot {
    color: rgba(255, 84, 112, 0.75);
  }

  @media (prefers-reduced-motion: reduce) {
    .st.pulse i,
    .steps i.active {
      animation: none;
    }
  }
</style>
