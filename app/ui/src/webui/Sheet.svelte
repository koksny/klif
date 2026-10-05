<script lang="ts">
  // One System's controls, from the bottom of the screen: status, preset, params, the main button (it says when a
  // launch stops other Systems), restart, the console. Live: it follows the state poll.
  import { onDestroy } from 'svelte';
  import { client, CONSOLE_MS, type Act } from './client.svelte';
  import { RUNNING, gb, glyph, mainAction, metricText, statusColor, statusWord, uptime } from './format';
  import type { WebState } from './types';

  let { web, id, onclose }: { web: WebState; id: string; onclose: () => void } = $props();

  const s = $derived(web.systems.find((x) => x.id === id));
  const machine = $derived(s ? web.machines.find((m) => m.id === s.machine) : undefined);
  const main = $derived(s ? mainAction(s) : null);
  let busy = $state('');
  let err = $state('');
  let consoleOpen = $state(false);
  let lines = $state<string[]>([]);
  let consoleErr = $state('');
  let timer: ReturnType<typeof setTimeout> | null = null;

  async function send(a: Act, what: string) {
    busy = what;
    err = '';
    try {
      await client.act(a);
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = '';
    }
  }

  function onMain() {
    if (!s || !main) return;
    if (main.kind === 'stop') void send({ type: 'stop', system: s.id }, 'Stopping…');
    else void send({ type: 'launch', system: s.id, stopOthers: main.kind === 'warn' }, 'Launching…');
  }

  async function pollConsole() {
    if (!consoleOpen || !s) return;
    try {
      lines = await client.console(s.id);
      consoleErr = '';
    } catch (e) {
      consoleErr = e instanceof Error ? e.message : String(e);
    }
    if (consoleOpen) timer = setTimeout(() => void pollConsole(), CONSOLE_MS);
  }

  function toggleConsole() {
    consoleOpen = !consoleOpen;
    if (timer) clearTimeout(timer);
    if (consoleOpen) void pollConsole();
  }

  onDestroy(() => {
    consoleOpen = false;
    if (timer) clearTimeout(timer);
  });

  const locked = $derived(!!busy || !s?.controllable);
</script>

<button type="button" class="scrim" aria-label="Close" onclick={onclose}></button>
<div class="sheet" role="dialog" aria-modal="true" aria-labelledby="sh-t">
  <span class="grab"></span>
  {#if !s}
    <p class="st">This System is not in KLIF any more.</p>
    <div class="sub"><button type="button" class="btn" onclick={onclose}>Close</button></div>
  {:else}
    <div class="sh-h" style="--c:{statusColor(s)}">
      <h3 id="sh-t">{s.label}</h3>
      <span class="where">{machine?.name ?? ''}</span>
    </div>
    <p class="st" style="--c:{statusColor(s)}">
      <i class="g {glyph(s)}"></i><em>{statusWord(s)}</em>
      {#if s.metric}<span>· {metricText(s)}</span>{/if}
      {#if s.load}<span>· {s.load.pct}% {s.load.step.toLowerCase()}</span>{/if}
      {#if !RUNNING(s) && s.last}<span>· last {s.last}</span>{/if}
    </p>
    {#if s.model}
      <p class="info">
        {s.model}{s.quant ? ` ${s.quant}` : ''}{s.gpuNames.length ? ` · ${s.gpuNames.join(' + ')}` : ''}{s.vramGiB !== undefined
          ? ` · ${RUNNING(s) ? '' : 'needs ~'}${gb(s.vramGiB)} GB`
          : ''}{s.uptimeS !== undefined && RUNNING(s) ? ` · up ${uptime(s.uptimeS)}` : ''}
      </p>
    {/if}
    {#if s.reason}<p class="reason" class:bad={s.status === 'fault' || s.status === 'invalid'}>{s.reason}</p>{/if}
    {#if !s.controllable}<p class="hint">KLIF here may only watch {s.label}: its machine does not grant launch.</p>{/if}
    {#if s.external}<p class="hint">An external server: KLIF only watches it.</p>{/if}

    {#if s.presets.length > 1 || (s.presets.length === 1 && s.presets[0].id !== s.preset)}
      <label class="field">
        <span>Preset</span>
        <select
          value={s.preset ?? ''}
          disabled={locked}
          onchange={(e) => void send({ type: 'usePreset', system: s.id, preset: e.currentTarget.value }, 'Saving…')}
        >
          {#if !s.preset}<option value="" disabled>Choose a preset</option>{/if}
          {#each s.presets as p (p.id)}<option value={p.id}>{p.name}{p.ready ? '' : ' (not ready)'}</option>{/each}
        </select>
      </label>
    {/if}
    {#each s.params as pa (pa.name)}
      <div class="field">
        <span>{pa.label}</span>
        <div class="seg" role="radiogroup" aria-label={pa.label}>
          {#each pa.choices as c (c.value)}
            <button
              type="button"
              role="radio"
              aria-checked={c.value === pa.value}
              class:on={c.value === pa.value}
              disabled={locked}
              onclick={() => c.value !== pa.value && void send({ type: 'setParam', system: s.id, name: pa.name, value: c.value }, 'Saving…')}
              >{c.label}</button
            >
          {/each}
        </div>
      </div>
    {/each}
    {#if RUNNING(s) && (s.params.length || s.presets.length > 1)}<p class="hint">A new preset or param takes effect when {s.label} restarts.</p>{/if}

    {#if main?.kind === 'warn'}
      <p class="conflict">
        {s.label}{s.model ? ` (${s.model})` : ''} needs {s.gpuNames.length ? `the ${s.gpuNames.join(' + ')}` : 'the same GPU'}. {s.conflicts.join(' and ')}
        {s.conflicts.length > 1 ? 'are' : 'is'} using it now and will stop.
      </p>
    {/if}
    {#if main}
      <button type="button" class="btn main {main.kind}" disabled={locked} onclick={onMain}>{busy || main.label}</button>
    {/if}
    {#if err}<p class="err" role="alert">{err}</p>{/if}
    <div class="sub">
      {#if RUNNING(s) && !s.external}
        <button type="button" class="btn" disabled={locked} onclick={() => void send({ type: 'restart', system: s.id }, 'Restarting…')}>Restart</button>
      {/if}
      {#if s.status === 'fault'}
        <button type="button" class="btn" disabled={locked} onclick={() => void send({ type: 'dismiss', system: s.id }, 'Dismissing…')}>Dismiss</button>
      {/if}
      <button type="button" class="btn" aria-expanded={consoleOpen} onclick={toggleConsole}>{consoleOpen ? 'Hide console' : 'Console'}</button>
      <button type="button" class="btn" onclick={onclose}>Close</button>
    </div>
    {#if consoleOpen}
      <pre class="console">{lines.length ? lines.join('\n') : consoleErr || (s.machine === 'local' ? 'No console lines yet.' : 'The console of a System on another machine shows in KLIF there, or when that System is selected in KLIF here.')}</pre>
    {/if}
  {/if}
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 4;
    border: 0;
    background: color-mix(in srgb, var(--k-bg) 65%, transparent);
  }
  .sheet {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 5;
    max-width: 640px;
    max-height: 88vh;
    margin: 0 auto;
    overflow-y: auto;
    padding: 8px 16px calc(14px + env(safe-area-inset-bottom));
    border: 1px solid var(--k-line);
    border-bottom: 0;
    border-radius: calc(var(--k-radius) * 1.5) calc(var(--k-radius) * 1.5) 0 0;
    background: var(--k-surface-raised);
  }
  .grab {
    display: block;
    width: 40px;
    height: 4px;
    margin: 0 auto 12px;
    border-radius: 2px;
    background: var(--k-line);
  }
  .sh-h {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 10px;
  }
  h3 {
    margin: 0;
    font: 700 22px var(--k-font-display);
  }
  .where {
    font-size: 13px;
    color: var(--k-muted);
  }
  .st {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    margin: 8px 0 0;
    font-size: 15px;
    color: var(--k-muted);
  }
  .st em {
    font-style: normal;
    font-weight: 600;
    color: var(--c);
  }
  .info {
    margin: 6px 0 0;
    font: 13px/1.4 var(--k-font-data);
  }
  .reason,
  .hint {
    margin: 8px 0 0;
    font-size: 14px;
    line-height: 1.4;
    color: var(--k-muted);
  }
  .reason.bad {
    color: var(--k-danger);
  }
  .conflict {
    margin: 16px 0 0;
    padding: 10px 12px;
    border-left: 3px solid var(--k-warn);
    background: color-mix(in srgb, var(--k-warn) 10%, transparent);
    font-size: 14px;
    line-height: 1.45;
  }
  .sub {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 10px;
  }
  .sub .btn {
    flex: 1 1 auto;
  }
  .console {
    max-height: 40vh;
    overflow: auto;
    margin: 10px 0 0;
    padding: 10px;
    border: 1px solid var(--k-line);
    background: var(--k-bg);
    font: 12px/1.45 var(--k-font-data);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
