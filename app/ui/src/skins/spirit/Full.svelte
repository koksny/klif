<script lang="ts">
  // Spirit, full window: the smoke is the subject being filmed, KLIF's data is the camera's OSD. The fixed
  // KLIF skeleton, scaled by --u (one design px at 1024x1152), the same regions in every phase (only the
  // contents change): header, tier tabs, model line, the viewfinder (absorbs the remaining height), detail
  // strip, request timeline / recent jobs, control bar, console line.
  //   idle      - STBY, timecode dashes, STANDBY hero with the last session, the configured settings line
  //   loading   - ◌ LOAD, the AF box closes in with the load fraction, the active load step under it
  //   live-llm  - ● REC, timecode running, decode tok/s (prefill: % of the prompt), meter = context fill
  //   live-img  - ● REC, step/steps, s/it and size on the settings line, meter = step progress
  //   dormant   - ‖ PAUSE, hero dimmed with the paged-out GiB, battery segments paged out
  //   fault     - ■ ERR, the corner brackets turn red, ERR + fault title, the log tail bottom left
  // Controls keep their places: the primary act (Launch / Cancel / Stop / Restart after a fault), Restart
  // (Dismiss after a fault), Tune (always), Endpoint / Web UI, Console (Full log after a fault).
  import type { Actions, RequestRecord, ViewModel } from '../../lib/model/types';
  import { fmtInt, fmtSeconds } from '../../lib/model/format';
  import Battery from './Battery.svelte';
  import Ic from './Ic.svelte';
  import Meter from './Meter.svelte';
  import Smoke from './Smoke.svelte';
  import Timecode from './Timecode.svelte';
  import WinCtl from './WinCtl.svelte';
  import { useOsd } from './osd.svelte';
  import { availabilityText, dateStamp, modelLine, short, sizeText } from './text';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  let w = $state(1024);
  let h = $state(1152);
  const u = $derived(Math.max(0.72, Math.min(1.2, Math.min(w / 1024, h / 1152))));

  const o = useOsd(
    () => vm,
    (id) => actions.launch(id),
    () => actions.stop(),
    () => actions.restart(),
  );
  const s = $derived(o.s);
  const faulted = $derived(o.faulted);
  const kind = $derived(o.kind);
  const frameless = $derived(!!vm.host?.frameless);
  const panel = $derived(vm.host?.panel?.available ? vm.host.panel : null);
  const panelTip = $derived(`Panel mode: show on the small screen${panel?.target ? ` (${panel.target})` : ''}`);
  let ready = $state(false);

  const PARTICLES = fmtInt(1024 * 1024);
  const logTail = $derived(o.fault ? o.fault.logTail.slice(-5) : []);

  // ---- detail strip: four fixed facts per kind, the same labels in every phase --------------------------------
  const rows = $derived.by<[string, string][]>(() => {
    const llm = o.llm;
    const img = o.img;
    const model = o.model;
    if (kind === 'image') {
      const j = img?.recent[img.recent.length - 1];
      return [
        ['LAST IMAGE', j ? `${fmtSeconds(j.seconds)} · ${j.width}×${j.height}${j.edit ? ' · edit' : ''}` : img ? 'none yet' : '—'],
        ['IMAGES', img ? `${fmtInt(img.imagesThisSession)} this session` : '—'],
        ['SIZE', sizeText(model?.imageSize) ?? '—'],
        ['MODE', model?.mode ?? '—'],
      ];
    }
    const pf = llm?.prefill;
    const total = llm?.context.totalTokens || model?.ctxTokens || 0;
    return [
      ['PREFILL', pf ? (o.prefilling ? `${pf.cachedTokens ? `${fmtInt(pf.cachedTokens)} cached · ` : ''}running ${fmtSeconds(pf.elapsedS)}` : `${fmtInt(pf.tokens)} tok · ${fmtInt(pf.tps)} tok/s`) : llm ? 'no request yet' : '—'],
      ['DECODE', llm ? (o.prefilling ? 'waiting for prefill' : `${fmtInt(llm.generatedTokens)} tok generated`) : '—'],
      ['CONTEXT', llm ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(total)} · ${Math.round((llm.context.usedTokens / Math.max(1, total)) * 100)}%` : total ? `— / ${fmtInt(total)}` : '—'],
      ['SPECULATIVE', llm?.spec ? `${Math.round(llm.spec.acceptancePct)}% · ${llm.spec.mode}` : llm ? 'off' : (model?.specMode ?? 'off')],
    ];
  });

  // ---- request timeline (8 places; a fault marks the last) / recent jobs (12 places) ---------------------------
  const reqN = $derived(faulted && o.llm ? 7 : 8);
  const requests = $derived(o.llm ? o.llm.requests.slice(-reqN) : []);
  const reqSlots = $derived(Array.from({ length: reqN }, (_, i) => requests[i - (reqN - requests.length)] ?? null));
  const jobN = $derived(faulted && o.img ? 11 : 12);
  const jobs = $derived(o.img ? o.img.recent.slice(-jobN) : []);
  const jobSlots = $derived(Array.from({ length: jobN }, (_, i) => jobs[i - (jobN - jobs.length)] ?? null));
  const maxJob = $derived(Math.max(1, ...jobs.map((j) => j.seconds)));
  const diedInWork = $derived(faulted && ((!!o.llm && o.llm.activity !== 'idle') || (!!o.img && o.img.activity === 'generating')));
  const tlCaption = $derived.by(() => {
    if (!s) return ' · NOT RUNNING';
    if (o.loading) return kind === 'image' ? ' · NO JOBS YET' : ' · NO REQUESTS YET';
    const n = kind === 'image' ? jobs.length : requests.length;
    const what = kind === 'image' ? 'JOBS' : 'REQUESTS';
    if (faulted) {
      if (!o.llm && !o.img) return ` · NO ${what}: FAILED DURING STARTUP`;
      return ` · ${n ? `LAST ${n}` : `NO FINISHED ${what}`}, THEN ${diedInWork ? `THE ${kind === 'image' ? 'JOB' : 'REQUEST'} THAT DIED` : 'THE FAULT'}`;
    }
    if (n === 0) return ` · NO FINISHED ${what} YET`;
    return kind === 'image' ? ` · BAR HEIGHT = TIME · LAST ${n} OF ${fmtInt(o.img?.imagesThisSession ?? n)}` : ` · LAST ${n} · PREFILL CYAN · DECODE INK`;
  });
  const reqTitle = (r: RequestRecord) =>
    `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${fmtSeconds(r.prefillS)} · ${fmtInt(r.generatedTokens)} tok in ${fmtSeconds(r.decodeS)}`;

  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const port = $derived(s?.endpoint.port ?? o.selSlot?.recipe?.port);
</script>

<div class="full" bind:clientWidth={w} bind:clientHeight={h} style="--u:{u}">
  <!-- Header (window drag region; draws its own window controls when the host is frameless) -->
  <header class="hdr" class:frameless data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region title="KLIF · Koksny.com LOCAL INFERENCE FORNICATOR{vm.host?.appVersion ? ` · v${vm.host.appVersion}` : ''}">
      <span class="wm">KLIF</span><span class="vbar"></span><span class="sk">SPIRIT</span>
    </div>
    <span class="grow" data-tauri-drag-region></span>
    <div class="st {o.status.tone}" data-tauri-drag-region><i></i>{o.status.text}</div>
    <span class="clk" data-tauri-drag-region>{dateStamp(vm.now)}</span>
    {#if panel}
      <button class="br pbtn" class:on={panel.active} onclick={() => actions.togglePanel()} title={panelTip} aria-label={panelTip} aria-pressed={panel.active}>PANEL</button>
    {/if}
    {#if frameless}<WinCtl {actions} maximized={!!vm.host?.maximized} />{/if}
  </header>

  <!-- Tier tabs: select while nothing runs (or after a fault); double-click launches -->
  <div class="tiers" role="tablist" aria-label="Tier">
    {#each vm.slots as t (t.id)}
      {@const selected = t.id === vm.selected}
      {@const na = t.availability !== 'ready'}
      {@const running = !!s && s.slot === t.id && !faulted}
      {@const locked = !!s && !faulted}
      <button
        class="tab"
        class:sel={selected}
        class:na
        class:flt={faulted && s?.slot === t.id}
        role="tab"
        aria-selected={selected}
        aria-disabled={locked}
        title={locked ? `${t.label}: ${running ? 'running' : `stop ${s?.model.name ?? 'the session'} to change the tier`}` : na ? `${t.label}: ${t.reason ?? availabilityText(t.availability)}` : `${t.label}: ${t.model.name} (double-click to launch)`}
        onclick={() => {
          if (!locked) actions.select(t.id);
        }}
        ondblclick={() => {
          if ((!s || faulted) && !na) actions.launch(t.id);
        }}
      >
        <span class="tl">{t.label}{#if running}<span class="rn" class:hold={o.loading || !!o.dz}><Ic kind="dot" /></span>{/if}</span>
        <span class="tm">{na ? `${t.model.name} · ${availabilityText(t.availability)}` : `${t.model.name} · ${t.model.quant}`}</span>
      </button>
    {/each}
  </div>

  <div class="mline" class:dim={!s}><span class="mk">{short(o.slot?.label ?? '')}</span><span class="mt">{modelLine(o.model, kind)}</span></div>

  <!-- The viewfinder: the smoke, and the camera OSD over it -->
  <section class="vf" class:err={faulted} aria-label="Viewfinder">
    <Smoke {vm} bind:ready />
    <div class="osd">
      <i class="cn tl"></i><i class="cn tr"></i><i class="cn bl"></i><i class="cn br2"></i>
      {#if !o.loading}<i class="cross"></i>{/if}

      <div class="recst {o.rec.kind}" role="status">{#if o.rec.glyph}<span class="g"><Ic kind={o.rec.glyph} /></span>{/if}{o.rec.text}</div>
      <div class="shape">{o.shape}</div>

      <div class="tc" class:idle={!s} class:red={faulted}><Timecode t={o.uptime} running={o.rec.kind === 'rec'} /></div>
      <div class="hero {o.hero.tone}">
        <div class="hl {o.hero.labelTone}">{o.hero.label}</div>
        <div class="hv"><b>{o.hero.value}</b>{#if o.hero.unit}<span class="u">{o.hero.unit}</span>{/if}</div>
        <div class="hs {o.hero.subTone}">{o.hero.sub}</div>
        {#if faulted && o.exitText}<div class="hx">{o.exitText}</div>{/if}
      </div>

      {#if o.loading}
        {@const k = s?.loading?.fraction ?? 0}
        <div class="af" class:lock={k >= 0.999} style="--k:{k}">
          <i class="c a"></i><i class="c b"></i><i class="c c2"></i><i class="c d"></i>
          <div class="afl">{o.afLine[0]}{#if o.afLine[1]}<span>{o.afLine[1]}</span>{/if}</div>
        </div>
      {/if}

      {#if logTail.length}
        <div class="log" role="log">{#each logTail as line, i (i)}<div title={line}>{line}</div>{/each}</div>
      {/if}

      <div class="set" class:dim={!s || !!o.dz}>
        {#each o.settings as it, i (i)}
          <span class="it" class:mdl={it.model}>{#if it.k}<em>{it.k}</em>{/if}{it.v}{#if it.u}<em class="un">{it.u}</em>{/if}</span>
        {/each}
      </div>
      <div class="mtr"><Meter m={o.meter} /></div>
      <div class="ptag" class:init={!ready}>{#if !ready}<Ic kind="ring" />{/if}{PARTICLES} PARTICLES</div>
      <div class="bat"><Battery b={o.battery} /></div>
    </div>
  </section>

  <section class="rows" class:dim={!s}>
    {#each rows as [k, v] (k)}<div class="kv"><span class="k">{k}</span><span class="v" title={v}>{v}</span></div>{/each}
  </section>

  <section class="tline">
    <div class="cap">{kind === 'image' ? 'RECENT JOBS' : 'REQUESTS'}<span class="dim">{tlCaption}</span></div>
    {#if kind === 'image'}
      <div class="jobs">
        {#each jobSlots as j, i (i)}
          <span class="place">
            {#if j}
              <span class="job" class:edit={j.edit} style="--h:{(j.seconds / maxJob) * 100}%" title="{fmtSeconds(j.seconds)} · {j.width}×{j.height}{j.edit ? ' · edit' : ''}"></span>
              <em>{j.seconds.toFixed(0)}S</em>
            {:else}<span class="job empty"></span><em></em>{/if}
          </span>
        {/each}
        {#if faulted && o.img}<span class="place"><span class="job failed" title={diedInWork ? 'The server died during this job' : 'The server died between jobs'}></span><em class="red">ERR</em></span>{/if}
      </div>
    {:else}
      <div class="reqs">
        {#each reqSlots as r, i (i)}
          <span class="place">
            {#if r}
              {@const tot = r.prefillS + r.decodeS}
              <span class="req" title={reqTitle(r)}><span class="pre" style="width:{tot > 0 ? (r.prefillS / tot) * 100 : 0}%"></span><span class="dec"></span></span>
              <em>{tot.toFixed(1)}S · {fmtInt(r.generatedTokens)} TOK</em>
            {:else}<span class="req empty"></span><em></em>{/if}
          </span>
        {/each}
        {#if faulted && o.llm}<span class="place"><span class="req failed" title={diedInWork ? 'The server died during this request' : 'The server died with no request in flight'}></span><em class="red">ERR</em></span>{/if}
      </div>
    {/if}
  </section>

  <footer class="ctrl">
    <button class="br chip" onclick={() => actions.copyEndpoint()} disabled={!o.online} title={o.online && s ? `Copy http://${s.endpoint.host}:${s.endpoint.port}` : 'Endpoint offline'}>
      :{port ?? '—'}{#if !o.online}<em>OFF</em>{/if}
    </button>
    <button class="br chip" onclick={() => actions.copyApiKey()} disabled={!o.online || !s?.apiKeySet} title={s?.apiKeySet ? 'Copy API key' : 'No API key set'}>
      {#if s?.apiKeySet}KEY<em>•••••</em>{:else}NO KEY{/if}
    </button>
    <span class="grow"></span>
    <button class="act {o.act.kind}" onclick={o.act.run} disabled={o.act.disabled} title={o.act.title}><span class="g" class:red={o.act.red}><Ic kind={o.act.glyph} /></span>{o.act.text}</button>
    {#if faulted}
      <button class="br w1" onclick={() => actions.dismiss()} title="Back to the launcher">DISMISS</button>
    {:else}
      <button class="br w1" onclick={() => actions.restart()} disabled={!s || o.busy} title="Stop and launch again with the current settings">RESTART</button>
    {/if}
    <button class="br w2" onclick={() => actions.openTune(vm.selected)} title={s && s.slot === vm.selected && !faulted ? 'Change the settings; Restart to apply them' : 'Change what this tier launches'}>TUNE</button>
    <button class="br w3" onclick={() => actions.openEndpoint()} disabled={!o.online} title={kind === 'image' ? 'Open the sd-server web UI' : 'Open the endpoint'}>{kind === 'image' ? 'WEB UI' : 'ENDPOINT'}</button>
    <button class="br w3" class:hot={faulted} onclick={() => actions.toggleConsole(faulted ? true : undefined)}>{faulted ? 'FULL LOG' : 'CONSOLE'}</button>
  </footer>
  <button class="cline" class:hot={faulted} onclick={() => actions.toggleConsole(true)} title="Open the console"><span class="pr">&gt;</span><span class="lt">{lastLine}</span></button>
</div>

<style>
  .full {
    --bg: #080808;
    --ink: #d9f3ff;
    --muted: #6f8c99;
    --osd: rgba(217, 243, 255, 0.7);
    --cyan: #2bc8ff;
    --warn: #ffb347;
    --red: #ff3b3b;
    --rule: rgba(217, 243, 255, 0.14);
    --rule2: rgba(217, 243, 255, 0.32);
    --fs-xs: max(10px, calc(var(--u) * 10.5px));
    --fs-s: max(10.5px, calc(var(--u) * 11.5px));
    --fs-m: max(11px, calc(var(--u) * 12.5px));
    --fs-l: max(12px, calc(var(--u) * 14px));
    --gap: calc(var(--u) * 9px);
    --shadow: 0 0 2px rgba(0, 0, 0, 0.95), 0 1px 5px rgba(0, 0, 0, 0.75);
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    padding: 0 calc(var(--u) * 16px) calc(var(--u) * 12px);
    font-family: 'IBM Plex Mono', Consolas, monospace;
    font-size: var(--fs-m);
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.08em;
    color: var(--ink);
    background: var(--bg);
    overflow: hidden;
    user-select: none;
  }
  .full > :global(*) {
    flex: none;
    min-width: 0;
  }
  button {
    font: inherit;
    letter-spacing: inherit;
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
  .grow {
    flex: 1 1 auto;
    align-self: stretch;
  }
  i {
    font-style: normal;
  }
  em {
    font-style: normal;
  }
  .red {
    color: var(--red);
  }

  /* bracketed OSD buttons: [ TUNE ] drawn as two thin brackets, like the viewfinder corners */
  .br {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: calc(var(--u) * 7px);
    height: calc(var(--u) * 30px);
    padding: 0 calc(var(--u) * 14px);
    font-size: var(--fs-s);
    font-weight: 500;
    letter-spacing: 0.12em;
    white-space: nowrap;
    flex: none;
  }
  .br::before,
  .br::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    width: calc(var(--u) * 5px);
    border: 1px solid var(--rule2);
    transition: border-color 0.15s ease;
  }
  .br::before {
    left: 0;
    border-right: 0;
  }
  .br::after {
    right: 0;
    border-left: 0;
  }
  .br:hover:not(:disabled)::before,
  .br:hover:not(:disabled)::after {
    border-color: var(--cyan);
  }
  .br:hover:not(:disabled) {
    color: #fff;
  }
  .br:disabled {
    cursor: default;
    opacity: 0.38;
  }
  .br em {
    color: var(--muted);
    font-weight: 400;
  }

  /* ---- header ---- */
  .hdr {
    height: calc(var(--u) * 50px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 16px);
    border-bottom: 1px solid var(--rule);
    margin: 0 calc(var(--u) * -16px);
    padding: 0 calc(var(--u) * 16px);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 12px);
    white-space: nowrap;
  }
  .wm {
    font-size: calc(var(--u) * 19px);
    font-weight: 500;
    letter-spacing: 0.34em;
  }
  .vbar {
    width: 1px;
    height: calc(var(--u) * 16px);
    background: var(--rule2);
  }
  .sk {
    font-size: var(--fs-m);
    letter-spacing: 0.32em;
    color: var(--cyan);
  }
  .st {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8px);
    font-size: var(--fs-m);
    font-weight: 500;
    letter-spacing: 0.16em;
    color: var(--muted);
    white-space: nowrap;
  }
  .st i {
    width: calc(var(--u) * 6px);
    height: calc(var(--u) * 6px);
    min-width: 5px;
    min-height: 5px;
    border-radius: 50%;
    background: currentColor;
  }
  .st.live {
    color: var(--ink);
  }
  .st.live i {
    background: var(--red);
    animation: rec 2s ease-in-out infinite;
  }
  .st.warn {
    color: var(--warn);
  }
  .st.red {
    color: var(--red);
  }
  .clk {
    font-size: var(--fs-m);
    color: var(--muted);
    white-space: pre;
  }
  .pbtn.on {
    color: var(--cyan);
  }

  /* ---- tier tabs ---- */
  .tiers {
    height: calc(var(--u) * 54px);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--gap);
  }
  .tab {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 4px);
    padding: 0 calc(var(--u) * 12px);
    border: 1px solid transparent;
    border-top-color: var(--rule);
    text-align: left;
    min-width: 0;
  }
  .tab .tl {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8px);
    font-size: var(--fs-l);
    font-weight: 500;
    letter-spacing: 0.14em;
    white-space: nowrap;
  }
  .tab .rn {
    display: inline-flex;
    font-size: 0.62em;
    color: var(--cyan);
    text-shadow: 0 0 6px rgba(43, 200, 255, 0.7);
  }
  .tab .rn.hold {
    color: var(--muted);
    text-shadow: none;
  }
  .tab .tm {
    font-size: var(--fs-s);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tab:hover:not(.sel):not([aria-disabled='true']) {
    border-color: var(--rule2);
  }
  .tab[aria-disabled='true'] {
    cursor: default;
  }
  .tab.sel {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--bg);
  }
  .tab.sel .tm {
    color: #26414d;
  }
  .tab.sel .rn {
    color: #0077a8;
    text-shadow: none;
  }
  .tab.na {
    opacity: 0.4;
  }
  .tab.flt {
    border-color: var(--red);
  }
  .tab.sel.flt {
    background: var(--red);
    color: #140202;
  }
  .tab.sel.flt .tm {
    color: #3a0808;
  }

  /* ---- model line ---- */
  .mline {
    height: calc(var(--u) * 26px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 12px);
    padding: 0 calc(var(--u) * 2px);
    font-size: var(--fs-m);
    letter-spacing: 0.07em;
    white-space: nowrap;
    overflow: hidden;
  }
  .mk {
    flex: none;
    color: var(--cyan);
    font-weight: 500;
  }
  .mt {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--ink);
  }
  .mline.dim .mt {
    color: var(--osd);
  }

  /* ---- viewfinder ---- */
  .vf {
    position: relative;
    flex: 1 1 0 !important;
    min-height: calc(var(--u) * 260px);
    overflow: hidden;
    background: var(--bg);
    --t: calc(var(--u) * 34px);
  }
  .osd {
    position: absolute;
    inset: 0;
    pointer-events: none;
    text-shadow: var(--shadow);
  }
  .cn {
    position: absolute;
    width: calc(var(--u) * 28px);
    height: calc(var(--u) * 28px);
    border: 0 solid var(--osd);
    transition: border-color 0.4s ease;
  }
  .cn.tl {
    left: calc(var(--u) * 14px);
    top: calc(var(--u) * 14px);
    border-left-width: 1.5px;
    border-top-width: 1.5px;
  }
  .cn.tr {
    right: calc(var(--u) * 14px);
    top: calc(var(--u) * 14px);
    border-right-width: 1.5px;
    border-top-width: 1.5px;
  }
  .cn.bl {
    left: calc(var(--u) * 14px);
    bottom: calc(var(--u) * 14px);
    border-left-width: 1.5px;
    border-bottom-width: 1.5px;
  }
  .cn.br2 {
    right: calc(var(--u) * 14px);
    bottom: calc(var(--u) * 14px);
    border-right-width: 1.5px;
    border-bottom-width: 1.5px;
  }
  .vf.err .cn {
    border-color: var(--red);
  }
  .cross {
    position: absolute;
    left: 50%;
    top: 50%;
    width: calc(var(--u) * 14px);
    height: calc(var(--u) * 14px);
    transform: translate(-50%, -50%);
    opacity: 0.4;
    background:
      linear-gradient(var(--ink), var(--ink)) center / 100% 1px no-repeat,
      linear-gradient(var(--ink), var(--ink)) center / 1px 100% no-repeat;
  }

  .recst {
    position: absolute;
    left: var(--t);
    top: var(--t);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 9px);
    font-size: var(--fs-l);
    font-weight: 500;
    letter-spacing: 0.14em;
    line-height: 1;
    color: var(--ink);
  }
  .recst .g {
    display: inline-flex;
    font-size: 0.95em;
    filter: drop-shadow(0 0 2px rgba(0, 0, 0, 0.9));
  }
  .recst.stby,
  .recst.pause,
  .recst.stop {
    color: var(--muted);
  }
  .recst.rec .g {
    color: var(--red);
    filter: drop-shadow(0 0 5px rgba(255, 59, 59, 0.55));
    animation: rec 2s ease-in-out infinite;
  }
  .recst.load .g,
  .recst.wake .g {
    color: var(--cyan);
  }
  .recst.err {
    color: var(--red);
  }
  .shape {
    position: absolute;
    left: var(--t);
    top: calc(var(--t) + var(--u) * 24px);
    font-size: var(--fs-s);
    letter-spacing: 0.1em;
    color: var(--osd);
    white-space: nowrap;
  }
  @keyframes rec {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.18;
    }
  }

  .tc {
    position: absolute;
    right: var(--t);
    top: calc(var(--t) - var(--u) * 3px);
    font-size: calc(var(--u) * 19px);
    font-weight: 400;
    letter-spacing: 0.06em;
    line-height: 1;
    color: var(--ink);
  }
  .tc.idle {
    color: var(--muted);
  }
  .tc.red {
    color: var(--red);
  }

  .hero {
    position: absolute;
    right: var(--t);
    top: calc(var(--t) + var(--u) * 36px);
    max-width: 60%;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    text-align: right;
  }
  /* Readability over bright smoke: a soft local darkening behind the readout (camera OSDs do the same). */
  .hero {
    isolation: isolate;
  }
  .hero::before {
    content: '';
    position: absolute;
    inset: -36px -28px -28px -90px;
    z-index: -1;
    background: radial-gradient(ellipse at 68% 50%, rgba(0, 0, 0, 0.6), rgba(0, 0, 0, 0.32) 45%, transparent 72%);
    pointer-events: none;
  }
  .hl {
    font-size: var(--fs-m);
    font-weight: 500;
    letter-spacing: 0.22em;
    color: var(--cyan);
    white-space: nowrap;
  }
  .hl.dim {
    color: var(--muted);
  }
  .hl.warn {
    color: var(--warn);
  }
  .hl.red {
    color: var(--red);
  }
  .hv {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 10px);
    margin-top: calc(var(--u) * 4px);
    white-space: nowrap;
  }
  .hv b {
    font-size: calc(var(--u) * 96px);
    font-weight: 200;
    line-height: 0.95;
    letter-spacing: -0.02em;
    color: var(--ink);
  }
  .hv .u {
    font-size: calc(var(--u) * 18px);
    font-weight: 300;
    color: var(--osd);
  }
  .hs {
    margin-top: calc(var(--u) * 6px);
    font-size: var(--fs-m);
    line-height: 1.4;
    color: var(--osd);
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .hs.warn {
    color: var(--warn);
  }
  .hs.red {
    color: #ff8a8a;
  }
  .hx {
    margin-top: calc(var(--u) * 4px);
    font-size: var(--fs-s);
    color: var(--red);
  }
  .hero.dim .hv b {
    opacity: 0.5;
  }
  .hero.red .hv b {
    color: var(--red);
  }

  /* AF box: the corners close in as the load fraction rises */
  .af {
    position: absolute;
    left: 50%;
    top: 50%;
    width: calc(var(--u) * 150px);
    height: calc(var(--u) * 96px);
    transform: translate(-50%, -50%);
    --sx: calc((1 - var(--k)) * var(--u) * 70px);
    --sy: calc((1 - var(--k)) * var(--u) * 44px);
  }
  .af .c {
    position: absolute;
    width: calc(var(--u) * 18px);
    height: calc(var(--u) * 18px);
    border: 0 solid var(--ink);
    transition:
      transform 0.9s cubic-bezier(0.25, 0.8, 0.3, 1),
      border-color 0.3s ease;
  }
  .af .a {
    left: 0;
    top: 0;
    border-left-width: 1.5px;
    border-top-width: 1.5px;
    transform: translate(calc(-1 * var(--sx)), calc(-1 * var(--sy)));
  }
  .af .b {
    right: 0;
    top: 0;
    border-right-width: 1.5px;
    border-top-width: 1.5px;
    transform: translate(var(--sx), calc(-1 * var(--sy)));
  }
  .af .c2 {
    left: 0;
    bottom: 0;
    border-left-width: 1.5px;
    border-bottom-width: 1.5px;
    transform: translate(calc(-1 * var(--sx)), var(--sy));
  }
  .af .d {
    right: 0;
    bottom: 0;
    border-right-width: 1.5px;
    border-bottom-width: 1.5px;
    transform: translate(var(--sx), var(--sy));
  }
  .af.lock .c {
    border-color: var(--cyan);
  }
  .afl {
    position: absolute;
    left: 50%;
    top: calc(100% + var(--u) * 52px);
    transform: translateX(-50%);
    display: flex;
    gap: calc(var(--u) * 10px);
    font-size: var(--fs-m);
    font-weight: 500;
    letter-spacing: 0.14em;
    white-space: nowrap;
  }
  .afl span {
    color: var(--osd);
    font-weight: 400;
  }

  .log {
    position: absolute;
    left: var(--t);
    bottom: calc(var(--u) * 104px);
    max-width: 50%;
    display: grid;
    row-gap: calc(var(--u) * 3px);
    padding-left: calc(var(--u) * 10px);
    border-left: 1px solid rgba(255, 59, 59, 0.55);
    font-size: var(--fs-xs);
    letter-spacing: 0.02em;
    color: rgba(255, 120, 120, 0.82);
  }
  .log div {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .set {
    position: absolute;
    left: var(--t);
    right: var(--t);
    bottom: calc(var(--u) * 66px);
    display: flex;
    justify-content: center;
    gap: calc(var(--u) * 28px);
    font-size: var(--fs-l);
    white-space: nowrap;
    overflow: hidden;
  }
  .set .it em {
    color: var(--osd);
    margin-right: 0.55em;
  }
  .set .it em.un {
    margin: 0 0 0 0.45em;
  }
  .set .mdl {
    color: var(--osd);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .set.dim .it {
    color: var(--osd);
  }
  .mtr {
    position: absolute;
    left: 50%;
    bottom: calc(var(--u) * 26px);
    width: calc(var(--u) * 380px);
    transform: translateX(-50%);
    font-size: var(--fs-xs);
  }
  .ptag {
    position: absolute;
    left: var(--t);
    bottom: calc(var(--u) * 31px);
    font-size: var(--fs-xs);
    letter-spacing: 0.12em;
    color: var(--muted);
    white-space: nowrap;
  }
  .ptag {
    display: flex;
    align-items: center;
    gap: 0.6em;
  }
  .ptag.init {
    animation: rec 2s ease-in-out infinite;
  }
  .bat {
    position: absolute;
    right: var(--t);
    bottom: calc(var(--u) * 28px);
    font-size: var(--fs-m);
  }

  /* ---- detail strip ---- */
  .rows {
    height: calc(var(--u) * 56px);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    border-top: 1px solid var(--rule);
    border-bottom: 1px solid var(--rule);
  }
  .kv {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 4px);
    padding: 0 calc(var(--u) * 12px);
    border-left: 1px solid var(--rule);
    min-width: 0;
  }
  .kv:first-child {
    border-left: 0;
    padding-left: calc(var(--u) * 2px);
  }
  .kv .k {
    font-size: var(--fs-xs);
    font-weight: 500;
    letter-spacing: 0.18em;
    color: var(--cyan);
  }
  .kv .v {
    font-size: var(--fs-l);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v {
    color: var(--osd);
  }

  /* ---- timeline ---- */
  .tline {
    height: calc(var(--u) * 82px);
    display: flex;
    flex-direction: column;
    gap: calc(var(--u) * 8px);
    padding: calc(var(--u) * 2px) calc(var(--u) * 2px) 0;
  }
  .cap {
    font-size: var(--fs-xs);
    font-weight: 500;
    letter-spacing: 0.18em;
    color: var(--cyan);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cap .dim {
    color: var(--muted);
    font-weight: 400;
    letter-spacing: 0.08em;
  }
  .reqs,
  .jobs {
    flex: 1;
    min-height: 0;
    display: grid;
    column-gap: calc(var(--u) * 14px);
  }
  .reqs {
    grid-template-columns: repeat(8, minmax(0, 1fr));
    align-items: center;
  }
  .jobs {
    grid-template-columns: repeat(12, minmax(0, 1fr));
  }
  .place {
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 7px);
    min-width: 0;
    height: 100%;
  }
  .place em {
    font-size: var(--fs-xs);
    letter-spacing: 0.04em;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-height: 1.2em;
  }
  .req {
    display: flex;
    height: calc(var(--u) * 3px);
    min-height: 2px;
  }
  .req .pre {
    background: rgba(43, 200, 255, 0.6);
  }
  .req .dec {
    flex: 1;
    background: var(--ink);
  }
  .req.empty {
    height: 0;
    border-top: 1px dashed var(--rule2);
  }
  .req.failed {
    height: 0;
    border-top: 2px dashed var(--red);
  }
  .jobs .place {
    justify-content: flex-end;
    align-items: center;
    gap: calc(var(--u) * 4px);
  }
  .jobs .place em {
    min-height: 1.15em;
  }
  .job {
    width: calc(var(--u) * 6px);
    height: calc(var(--h) * 0.72);
    min-height: 2px;
    background: var(--ink);
  }
  .job.edit {
    background: none;
    border: 1px solid var(--cyan);
  }
  .job.empty {
    width: 60%;
    height: 0;
    min-height: 0;
    background: none;
    border-top: 1px dashed var(--rule2);
  }
  .job.failed {
    height: 72%;
    background: none;
    border: 1px dashed var(--red);
  }

  /* ---- control bar ---- */
  .ctrl {
    height: calc(var(--u) * 48px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 10px);
    border-top: 1px solid var(--rule);
    padding-top: calc(var(--u) * 2px);
  }
  .chip {
    font-weight: 400;
    letter-spacing: 0.06em;
  }
  .chip em {
    margin-left: calc(var(--u) * 2px);
  }
  .w1 {
    width: calc(var(--u) * 106px);
  }
  .w2 {
    width: calc(var(--u) * 78px);
  }
  .w3 {
    width: calc(var(--u) * 116px);
  }
  .br.hot {
    color: #ff8a8a;
  }
  /* the primary place: one width for launch / cancel / stop / restart, so nothing beside it moves */
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: calc(var(--u) * 9px);
    width: calc(var(--u) * 220px);
    height: calc(var(--u) * 32px);
    flex: none;
    border: 1px solid var(--ink);
    font-size: var(--fs-m);
    font-weight: 500;
    letter-spacing: 0.16em;
    white-space: nowrap;
    overflow: hidden;
    transition:
      background 0.15s ease,
      color 0.15s ease;
  }
  .act .g {
    display: inline-flex;
    font-size: 0.95em;
    color: var(--osd);
  }
  .act .g.red {
    color: var(--red);
    filter: drop-shadow(0 0 4px rgba(255, 59, 59, 0.7));
  }
  .act:hover:not(:disabled) {
    background: var(--ink);
    color: var(--bg);
  }
  .act.go:hover:not(:disabled) .g {
    color: var(--bg);
  }
  .act:disabled {
    cursor: default;
    border-color: var(--rule2);
    color: var(--muted);
  }
  .act.hot {
    border-color: var(--red);
    color: var(--red);
    background: rgba(255, 59, 59, 0.1);
  }
  .act.hot .g {
    color: var(--red);
  }
  .act.hot:hover {
    background: var(--red);
    color: #140202;
  }
  .act.hot:hover .g {
    color: #140202;
  }

  .cline {
    height: calc(var(--u) * 20px);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8px);
    font-size: var(--fs-xs);
    letter-spacing: 0.02em;
    color: var(--muted);
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
    color: var(--ink);
  }
  .cline.hot .lt {
    color: #ff8a8a;
  }

  @media (prefers-reduced-motion: reduce) {
    .recst.rec .g,
    .st.live i,
    .ptag {
    display: flex;
    align-items: center;
    gap: 0.6em;
  }
  .ptag.init {
      animation: none;
    }
  }
</style>
