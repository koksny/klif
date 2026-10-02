<script lang="ts">
  // Full window: one hardware front panel. Header, job selector + backend toggle, model line and the
  // bottom control strip are shared by every state; the body in between branches on the session:
  //   idle            -> Idle (fit preview, launch)
  //   live LLM        -> LiveLlm (approved live panel; prefill takes the hero while a prompt is processed)
  //   everything else -> Session (starting / loading, live image, fault)
  import { onMount, tick } from 'svelte';
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtClock } from '../../../lib/model/format';
  import { PHASE_LABEL, modelLine, phaseOf, pointerSlot, slotById } from '../theme';
  import Mark from '../parts/Mark.svelte';
  import Led from '../parts/Led.svelte';
  import Selector from '../parts/Selector.svelte';
  import BackendToggle from '../parts/BackendToggle.svelte';
  import Icon from '../parts/Icon.svelte';
  import WinCtl from '../parts/WinCtl.svelte';
  import LiveLlm from './LiveLlm.svelte';
  import Idle from './Idle.svelte';
  import Session from './Session.svelte';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  const s = $derived(vm.session);
  const phase = $derived(phaseOf(vm));
  const ptr = $derived(pointerSlot(vm));
  const ptrSlot = $derived(slotById(vm, ptr));
  const backend = $derived(s?.model.backend ?? ptrSlot?.model.backend ?? null);
  const isLiveLlm = $derived(!!s && (s.phase === 'live' || s.phase === 'stopping') && !!s.llm);
  const isFault = $derived(s?.phase === 'fault');
  const isLoading = $derived(s?.phase === 'starting' || s?.phase === 'loading');
  const isImage = $derived(ptrSlot ? ptrSlot.kind === 'image' : !!s?.image);
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const frameless = $derived(vm.host?.frameless ?? false);
  const runLabel = $derived(ptrSlot?.label ?? s?.slot.toUpperCase() ?? '');
  // Panel mode (read-only mini layout on the small status screen): always offered when a target exists.
  const panelAvailable = $derived(vm.host?.panel?.available ?? false);
  const panelActive = $derived(vm.host?.panel?.active ?? false);
  const panelTip = $derived(
    panelActive
      ? 'Leave panel mode'
      : `Panel mode: show on the small screen${vm.host?.panel?.target ? ` (${vm.host.panel.target})` : ''}`,
  );
  // The button carries a PANEL label while the header has room for it. When the brand tagline would be
  // clipped (frameless window controls, long phase names, narrow windows) it drops to the bare glyph;
  // title and aria-label stay. Re-measured whenever the header width, phase or fonts change.
  let hdrW = $state(0);
  let fontsTick = $state(0);
  let tagEl = $state<HTMLElement>();
  let compactPm = $state(false);
  onMount(() => {
    const bump = () => fontsTick++;
    document.fonts?.addEventListener('loadingdone', bump);
    void document.fonts?.ready.then(bump);
    return () => document.fonts?.removeEventListener('loadingdone', bump);
  });
  $effect(() => {
    void hdrW;
    void fontsTick;
    void phase;
    void frameless;
    void panelAvailable;
    void panelActive;
    compactPm = false;
    void tick().then(() => {
      if (tagEl && tagEl.scrollWidth > tagEl.clientWidth + 1) compactPm = true;
    });
  });

  function back() {
    if (typeof actions.dismiss === 'function') actions.dismiss();
    else actions.stop();
  }
</script>

{#snippet panelBtn()}
  <button class="pm" class:bare={compactPm} type="button" onclick={() => actions.togglePanel?.()} title={panelTip} aria-label={panelTip}>
    <Icon name="screen" size="calc(24 * var(--u))" />
    {#if !compactPm}<span class="pl">{panelActive ? 'WINDOW' : 'PANEL'}</span>{/if}
  </button>
{/snippet}

<div class="full">
  <header class="hdr" data-tauri-drag-region bind:clientWidth={hdrW}>
    <div class="brand" data-tauri-drag-region>
      <span class="mk"><Mark /></span>
      <span class="word">KLIF</span>
      <span class="tag" bind:this={tagEl}>Koksny.com LOCAL INFERENCE FORNICATOR</span>
    </div>
    <div class="status" data-tauri-drag-region>
      <Led on={phase !== 'idle'} tone={phase === 'fault' ? 'orange' : 'cyan'} size="calc(15 * var(--u))" />
      <span class="ph" class:fault={phase === 'fault'}>{PHASE_LABEL[phase]}</span>
      {#if s}
        <span class="vsep"></span>
        <span class="up">{isLoading ? 'elapsed' : 'uptime'} <b>{fmtClock(s.uptimeS)}</b></span>
      {/if}
      {#if frameless}
        <span class="vsep wsep"></span>
        <div class="grp">
          {#if panelAvailable}{@render panelBtn()}{/if}
          <WinCtl
            maximized={vm.host?.maximized ?? false}
            onmin={() => actions.minimize?.()}
            onmax={() => actions.toggleMaximize?.()}
            onclose={() => actions.closeWindow?.()}
          />
        </div>
      {:else if panelAvailable}
        <span class="vsep"></span>
        {@render panelBtn()}
      {/if}
    </div>
  </header>

  <div class="row mode" class:tall={!s}>
    <section class="panel sel">
      <span class="lbl corner">MODE</span>
      <div class="selbox">
        <Selector
          slots={vm.slots}
          pointer={ptr}
          selected={vm.selected}
          running={s?.slot ?? null}
          layout={s ? 'inline' : 'stack'}
          locked={isLoading}
          tone={isFault ? 'orange' : 'cyan'}
          onselect={(id) => actions.select(id)}
          onlaunch={(id) => actions.launch(id)}
        />
      </div>
    </section>
    <section class="panel be">
      <span class="lbl corner">BACKEND</span>
      <div class="bebox">
        <BackendToggle {backend} onclick={() => actions.openTune(ptr)} />
      </div>
    </section>
  </div>

  <section class="panel model">
    <span class="lbl">{s ? 'ACTIVE MODEL' : 'SELECTED MODEL'}</span>
    <span class="mline" title={modelLine(s, ptrSlot)}>{modelLine(s, ptrSlot)}</span>
  </section>

  <div class="body">
    {#if !s}
      <Idle {vm} {actions} />
    {:else if isLiveLlm && s.llm}
      <LiveLlm {vm} llm={s.llm} />
    {:else}
      <Session {vm} {s} {actions} />
    {/if}
  </div>

  <section class="panel bar" class:s={!!s}>
    {#if s}
      <span class="lbl">ENDPOINT</span>
      {#if isFault}
        <span class="chip ep off" title="The server is not running">offline</span>
      {:else}
        <button class="chip ep" title="Copy {s.endpoint.host}:{s.endpoint.port}" onclick={() => actions.copyEndpoint()}>
          <span class="port">:{s.endpoint.port}</span>
        </button>
      {/if}
      <span class="vsep"></span>
      <span class="lbl">API KEY</span>
      <button class="chip key" disabled={!s.apiKeySet} onclick={() => actions.copyApiKey()} aria-label="Copy API key">
        <span class="dots">{s.apiKeySet ? '•••••••••' : 'none'}</span>
        {#if s.apiKeySet}<span class="copy">copy</span>{/if}
      </button>
      <span class="vsep"></span>
      {#if isFault}
        <button class="btn primary hot" onclick={() => actions.restart()}>
          <Icon name="restart" size="calc(24 * var(--u))" />Restart {runLabel}
        </button>
        <button class="btn" onclick={() => actions.toggleConsole(true)}><Icon name="log" size="calc(24 * var(--u))" />Show full log</button>
        <button class="btn" onclick={back}><Icon name="back" size="calc(24 * var(--u))" />Back to launcher</button>
      {:else}
        <button class="btn primary stop" onclick={() => actions.stop()} disabled={s.phase === 'stopping'}>
          <span class="sq"></span>{isLoading ? 'Cancel' : 'Stop'}
        </button>
        <button class="btn" onclick={() => actions.restart()}><Icon name="restart" size="calc(24 * var(--u))" />Restart</button>
        <button class="btn" onclick={() => actions.openEndpoint()} disabled={s.phase !== 'live'}>
          <Icon name="open" size="calc(22 * var(--u))" />{isImage ? 'Open web UI' : 'Open endpoint'}
        </button>
        <button class="btn" onclick={() => actions.toggleConsole()}><Icon name="console" size="calc(24 * var(--u))" />Console</button>
      {/if}
    {:else}
      <span class="lbl">ENDPOINT</span>
      <span class="chip ep off" title="No session is running">offline</span>
      <span class="grow"></span>
      <button class="btn" onclick={() => actions.toggleConsole()}><Icon name="console" size="calc(24 * var(--u))" />Console</button>
    {/if}
  </section>

  <button class="console" onclick={() => actions.toggleConsole()} title="Open console">
    <Icon name="chev" size="calc(18 * var(--u))" />
    <span class="cl">{lastLine || 'no output yet'}</span>
  </button>
</div>

<style>
  .full {
    display: flex;
    flex-direction: column;
    gap: calc(4 * var(--u));
    height: 100%;
    padding: 0 calc(4 * var(--u)) calc(8 * var(--u));
  }
  .full > * {
    flex: 0 0 auto;
  }

  /* header */
  .hdr {
    height: calc(82 * var(--u));
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 calc(26 * var(--u)) 0 calc(24 * var(--u));
    border-bottom: 1px solid #0a0b0c;
    box-shadow: 0 1px 0 rgba(255, 255, 255, 0.03);
    margin: 0 calc(-4 * var(--u));
  }
  .brand {
    display: flex;
    align-items: center;
    gap: calc(18 * var(--u));
    min-width: 0;
  }
  .mk {
    display: block;
    width: calc(60 * var(--u));
    height: calc(60 * var(--u));
  }
  .word {
    font-weight: 700;
    font-size: calc(74 * var(--u));
    line-height: 1;
    letter-spacing: 0.01em;
    color: var(--cream);
    margin-top: calc(2 * var(--u));
  }
  .tag {
    font-weight: 500;
    font-size: calc(15 * var(--u));
    letter-spacing: 0.09em;
    color: rgba(237, 230, 214, 0.72);
    margin-top: calc(14 * var(--u));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    display: flex;
    align-items: center;
    gap: calc(14 * var(--u));
    height: calc(44 * var(--u));
    flex: 0 0 auto;
  }
  .wsep {
    margin-left: calc(4 * var(--u));
  }
  /* frameless: the panel button joins the window controls as one cluster */
  .grp {
    display: flex;
    align-items: center;
    gap: calc(4 * var(--u));
  }
  /* panel-mode button: a window-control-style key with a printed label */
  .pm {
    display: inline-flex;
    align-items: center;
    gap: calc(8 * var(--u));
    height: calc(40 * var(--u));
    padding: 0 calc(11 * var(--u)) 0 calc(9 * var(--u));
    border: 1px solid rgba(237, 230, 214, 0.2);
    border-radius: calc(4 * var(--u));
    background: transparent;
    color: rgba(237, 230, 214, 0.86);
    cursor: pointer;
    flex: 0 0 auto;
    white-space: nowrap;
  }
  .pm.bare {
    width: calc(44 * var(--u));
    padding: 0;
    justify-content: center;
  }
  .pm :global(.ic) {
    stroke-width: 1.5;
  }
  .pm:hover {
    background: rgba(237, 230, 214, 0.08);
    border-color: rgba(237, 230, 214, 0.34);
    color: var(--cream);
  }
  .pm:active {
    background: rgba(0, 0, 0, 0.25);
  }
  .pl {
    font-weight: 500;
    font-size: calc(15 * var(--u));
    letter-spacing: 0.08em;
    line-height: 1;
  }
  .ph {
    font-weight: 400;
    font-size: calc(20 * var(--u));
    letter-spacing: 0.06em;
  }
  .ph.fault {
    color: var(--orange);
  }
  .up {
    font-size: calc(17 * var(--u));
    color: var(--cream-2);
    letter-spacing: 0.02em;
    margin-left: calc(4 * var(--u));
  }
  .up b {
    font-weight: 400;
    font-size: calc(20 * var(--u));
    color: var(--cream);
    margin-left: calc(4 * var(--u));
  }

  /* panels */
  .corner {
    position: absolute;
    left: calc(26 * var(--u));
    top: calc(30 * var(--u));
  }
  .row {
    display: flex;
    gap: calc(4 * var(--u));
  }
  .mode {
    height: calc(122 * var(--u));
  }
  .mode.tall {
    height: calc(200 * var(--u));
  }
  .sel {
    flex: 1 1 auto;
  }
  .selbox {
    position: absolute;
    left: calc(100 * var(--u));
    top: calc(4 * var(--u));
    right: 0;
  }
  .be {
    flex: 0 0 calc(250 * var(--u));
  }
  .be .corner {
    left: calc(22 * var(--u));
  }
  .bebox {
    position: absolute;
    right: calc(30 * var(--u));
    top: calc(28 * var(--u));
  }
  .tall .bebox {
    top: calc(44 * var(--u));
  }
  .tall .corner {
    top: calc(46 * var(--u));
  }
  .model {
    height: calc(48 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(40 * var(--u));
    padding: 0 calc(26 * var(--u));
  }
  .mline {
    font-weight: 400;
    font-size: calc(21.5 * var(--u));
    letter-spacing: 0.05em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: calc(4 * var(--u));
  }

  /* bottom strip */
  .bar {
    height: calc(76 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(11 * var(--u));
    padding: 0 calc(20 * var(--u)) 0 calc(26 * var(--u));
  }
  .bar .btn {
    gap: calc(10 * var(--u));
    padding: 0 calc(14 * var(--u));
    font-size: calc(18 * var(--u));
  }
  .bar .vsep {
    height: calc(46 * var(--u));
    align-self: center;
    margin: 0 calc(3 * var(--u));
  }
  .bar .lbl {
    font-size: calc(14 * var(--u));
  }
  .chip {
    flex: 0 0 auto;
  }
  .ep {
    width: calc(88 * var(--u));
    padding: 0 calc(14 * var(--u));
  }
  .ep.off {
    width: auto;
    cursor: default;
    font-size: calc(19 * var(--u));
    letter-spacing: 0.04em;
    color: var(--cream-2);
  }
  .port {
    font-size: calc(22 * var(--u));
    letter-spacing: 0.03em;
  }
  .key .dots {
    padding: 0 calc(12 * var(--u));
    font-size: calc(16 * var(--u));
    letter-spacing: 0.08em;
  }
  .key .copy {
    align-self: stretch;
    display: flex;
    align-items: center;
    padding: 0 calc(11 * var(--u));
    border-left: 1px solid #0a0b0c;
    background: linear-gradient(180deg, #2a2b2f, #1e1f22);
    font-size: calc(16 * var(--u));
    color: var(--cream-2);
  }
  .bar .btn {
    flex: 0 0 auto;
  }
  .bar.s .btn {
    flex: 1 0 auto;
  }
  .stop {
    min-width: calc(104 * var(--u));
  }
  .sq {
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    border-radius: 2px;
    background: var(--cyan);
    box-shadow: 0 0 calc(8 * var(--u)) rgba(90, 182, 235, 0.7);
  }
  .bar .btn.primary.hot {
    border-color: var(--orange);
    color: #ff8a55;
    box-shadow:
      inset 0 0 0 1px rgba(255, 107, 44, 0.35),
      0 0 calc(10 * var(--u)) rgba(255, 107, 44, 0.3);
  }
  .grow {
    flex: 1 1 auto;
  }

  /* collapsed console */
  .console {
    height: calc(42 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(14 * var(--u));
    padding: 0 calc(16 * var(--u));
    margin: calc(2 * var(--u)) calc(14 * var(--u)) 0;
    border-radius: calc(5 * var(--u));
    border: 1px solid #0a0b0c;
    background: #18191b;
    box-shadow: inset 0 1px 3px rgba(0, 0, 0, 0.6);
    color: var(--cream-2);
    cursor: pointer;
    text-align: left;
    min-width: 0;
  }
  .console:hover {
    background: #1c1d20;
  }
  .cl {
    font-size: calc(18.5 * var(--u));
    letter-spacing: 0.04em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
</style>
