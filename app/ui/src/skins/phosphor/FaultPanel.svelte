<script lang="ts">
  // Fault hero: what happened (fault.title), the exit code and how long ago, the last real trace
  // ending in a red drop at the failure (decode tok/s for an LLM that was serving, otherwise the VRAM
  // history), or the load checklist when the process died while loading (fault.steps), then the last
  // log lines and the three ways out.
  import type { Actions, GpuMemory, Session, Slot } from '../../lib/model/types';
  import FaultTrace from './FaultTrace.svelte';
  import Steps from './Steps.svelte';
  import { fmtAgo } from './geom';

  let {
    session,
    slot,
    vram,
    actions,
    k,
  }: { session: Session; slot: Slot | undefined; vram: GpuMemory; actions: Actions; k: number } = $props();

  const f = $derived(session.fault);
  const since = $derived(f?.sinceS ?? 0);
  const steps = $derived(f?.steps ?? []);

  const source = $derived.by(() => {
    if (steps.length) return 'steps' as const;
    const d = session.llm?.decodeHistory;
    if (d && d.some((v) => v > 0)) return 'decode' as const;
    return 'vram' as const;
  });

  /** VRAM samples up to the failure (the history keeps rolling after it, at the baseline). */
  const vramBefore = $derived.by(() => {
    const hst = vram.history;
    const cut = Math.max(1, hst.length - Math.round(since));
    return hst.slice(0, cut);
  });

  const exitText = $derived(f?.exitCodeHex ?? (f?.exitCode !== undefined ? String(f.exitCode) : null));
  /** As many of the newest log lines as fit the box (never a clipped half line). */
  let linesH = $state(0);
  const fit = $derived(Math.max(2, Math.floor((linesH - 12 * k - 2) / (16 * k * 1.42))));
  const lines = $derived((f?.logTail ?? []).slice(-fit));

  function back() {
    if (actions.dismiss) actions.dismiss();
    else actions.stop();
  }
</script>

<section class="panel grat fault" class:withsteps={source === 'steps'} aria-label="Fault">
  <div class="ftitle">{f?.title ?? 'The server stopped unexpectedly.'}</div>
  <div class="fmeta">
    {#if exitText}
      <span>exit code <span class="code" title={f?.exitCode !== undefined ? `exit code ${f.exitCode}` : undefined}>{exitText}</span></span>
      {#if f?.exitCodeHex && f?.exitCode !== undefined}<span class="dec">{f.exitCode}</span>{/if}
    {:else}
      <span class="mut">no exit code</span>
    {/if}
    <span class="bar" aria-hidden="true"></span>
    <span>{fmtAgo(since)}</span>
  </div>

  <div class="trace">
    {#if source === 'steps'}
      <div class="stepbox">
        <div class="lbl sm">Load steps at the failure</div>
        <Steps {steps} compact />
      </div>
    {:else if source === 'decode'}
      <FaultTrace values={session.llm?.decodeHistory ?? []} agoS={since} unit="decode tok/s · until the fault" />
    {:else}
      <FaultTrace values={vramBefore} agoS={since} unit="VRAM GiB · until the fault" stepped minSpan={2} />
    {/if}
  </div>

  <div class="logbox">
    <div class="lbl sm">Last log lines</div>
    <div class="lines" bind:clientHeight={linesH}>
      {#each lines as line, i (i)}<div class="ln">{line}</div>{/each}
      {#if lines.length === 0}<div class="ln mut">no output captured</div>{/if}
    </div>
  </div>

  <div class="acts">
    <button class="act go" onclick={() => actions.restart()}>Restart {slot?.label ?? ''}</button>
    <button class="act" onclick={() => actions.toggleConsole(true)}>Show full log</button>
    <button class="act" onclick={back}>Back to launcher</button>
  </div>
</section>

<style>
  .fault {
    flex: 2.2 1 0;
    min-height: calc(360px * var(--k));
    display: flex;
    flex-direction: column;
    gap: calc(10px * var(--k));
    padding: calc(16px * var(--k)) calc(22px * var(--k)) calc(16px * var(--k));
    overflow: hidden;
  }
  .ftitle {
    flex: none;
    font-family: var(--ph-display);
    font-stretch: 106%;
    font-weight: 400;
    font-size: calc(33px * var(--k));
    line-height: 1.12;
    letter-spacing: 0.02em;
    color: var(--ph-danger);
    text-shadow:
      0 0 8px rgba(229, 97, 92, 0.5),
      0 0 22px rgba(229, 97, 92, 0.2);
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
  .fmeta {
    flex: none;
    display: flex;
    align-items: baseline;
    gap: calc(16px * var(--k));
    font-size: calc(23px * var(--k));
    letter-spacing: 0.05em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow-soft);
    white-space: nowrap;
  }
  .code {
    color: var(--ph-ink);
  }
  .dec {
    font-size: 0.72em;
    color: var(--ph-muted);
    text-shadow: none;
  }
  .bar {
    width: 1px;
    height: 0.9em;
    align-self: center;
    background: var(--ph-rule);
    margin: 0 calc(4px * var(--k));
  }
  .trace {
    flex: 1.25 1 0;
    min-height: calc(72px * var(--k));
    margin: 0 calc(-8px * var(--k)) 0 calc(-6px * var(--k));
  }
  .withsteps .trace {
    flex: none;
    min-height: 0;
  }
  .stepbox {
    height: 100%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    gap: calc(6px * var(--k));
    padding: 0 calc(8px * var(--k));
    overflow: hidden;
  }
  .lbl.sm {
    font-size: calc(15px * var(--k));
  }
  .logbox {
    flex: 1 1 0;
    min-height: calc(110px * var(--k));
    display: flex;
    flex-direction: column;
    gap: calc(8px * var(--k));
    border: 1px solid var(--ph-rule);
    border-radius: 5px;
    background: rgba(3, 9, 12, 0.82);
    padding: calc(10px * var(--k)) calc(16px * var(--k)) calc(12px * var(--k));
    overflow: hidden;
  }
  .lines {
    flex: 1 1 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--ph-grat);
    border-radius: 3px;
    padding: calc(6px * var(--k)) calc(12px * var(--k));
  }
  .ln {
    flex: none;
    font-size: calc(16px * var(--k));
    line-height: 1.42;
    letter-spacing: 0.02em;
    color: #d8ecf2;
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .acts {
    flex: none;
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: calc(14px * var(--k));
    height: calc(48px * var(--k));
  }
  .act {
    border: 1px solid #2a7f93;
    border-radius: 5px;
    color: var(--ph-cyan);
    background: rgba(3, 9, 12, 0.7);
    font-size: calc(19px * var(--k));
    letter-spacing: 0.04em;
    text-shadow: var(--ph-glow-soft);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0 calc(12px * var(--k));
  }
  .act:hover {
    border-color: var(--ph-cyan);
    background: rgba(18, 48, 58, 0.5);
  }
  .act.go {
    color: #ff8f88;
    border: 1.5px solid var(--ph-danger);
    background: rgba(60, 14, 14, 0.32);
    box-shadow:
      0 0 12px rgba(229, 97, 92, 0.35),
      inset 0 0 10px rgba(229, 97, 92, 0.12);
    text-shadow: 0 0 6px rgba(229, 97, 92, 0.5);
  }
  .act.go:hover {
    border-color: #ff8a80;
    background: rgba(90, 20, 20, 0.42);
  }
</style>
