<script lang="ts">
  // Fault: covers the whole status block (same outer box, nothing below moves). What happened
  // (fault.title), the tier, model and exit code, how long ago; then the last real trace ending in a red
  // drop at the failure (decode tok/s for an LLM that was serving, otherwise the VRAM history), or the
  // load checklist when the process died while loading (fault.steps), beside the last log lines.
  // The ways out (Restart, Dismiss, Full log) are in the controls bar, in their usual places.
  import type { GpuMemory, Session, System } from '../../lib/model/types';
  import FaultTrace from './FaultTrace.svelte';
  import Steps from './Steps.svelte';
  import { fmtAgo } from './geom';

  let { session, system, vram, k }: { session: Session; system: System | undefined; vram: GpuMemory; k: number } = $props();

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

  const exitText = $derived.by(() => {
    if (!f || f.exitCode === undefined) return 'no exit code';
    return f.exitCodeHex ? `exit code ${f.exitCodeHex} (${f.exitCode})` : `exit code ${f.exitCode}`;
  });
  /** As many of the newest log lines as fit the box (never a clipped half line). */
  let linesH = $state(0);
  const lineH = $derived(Math.max(10.5, 11 * k) * 1.45);
  const fit = $derived(Math.max(1, Math.floor((linesH + 1) / lineH)));
  const lines = $derived((f?.logTail ?? []).slice(-fit));
</script>

<section class="panel grat fault" role="alert" aria-label="Fault">
  <div class="fhead">
    <svg class="ico" viewBox="0 0 48 44" aria-hidden="true"><path d="M24 3.5 L45 40.5 H3 Z" /><path d="M24 16v12.5" /><circle cx="24" cy="34.2" r="1.6" class="idot" /></svg>
    <div class="ft">
      <div class="ftitle" title={f?.title}>{f?.title ?? 'The server stopped unexpectedly.'}</div>
      <div class="fsub">{system?.label ?? ''} · {session.model.name} · <span class="code">{exitText}</span></div>
    </div>
    <div class="ago">{fmtAgo(since)}</div>
  </div>

  <div class="fbody">
    <div class="trace">
      {#if source === 'steps'}
        <div class="lbl sm">Load steps at the failure</div>
        <div class="stepbox"><Steps {steps} layout="compact" /></div>
      {:else if source === 'decode'}
        <FaultTrace values={session.llm?.decodeHistory ?? []} agoS={since} unit="decode tok/s · until the fault" />
      {:else}
        <FaultTrace values={vramBefore} agoS={since} unit="VRAM GiB · until the fault" stepped minSpan={2} />
      {/if}
    </div>
    <div class="logbox">
      <div class="lbl sm">Last log lines</div>
      <div class="lines" bind:clientHeight={linesH} style="--lh:{lineH.toFixed(2)}px">
        {#each lines as line, i (i)}<div class="ln mono" title={line}>{line}</div>{/each}
        {#if lines.length === 0}<div class="ln mut">no output captured</div>{/if}
      </div>
    </div>
  </div>
</section>

<style>
  .fault {
    height: 100%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(10px * var(--k)) calc(16px * var(--k)) calc(10px * var(--k));
    border-color: rgba(229, 97, 92, 0.85);
    box-shadow:
      0 0 14px rgba(229, 97, 92, 0.22),
      inset 0 0 24px rgba(229, 97, 92, 0.08);
    overflow: hidden;
  }
  .fhead {
    display: grid;
    grid-template-columns: calc(34px * var(--k)) minmax(0, 1fr) auto;
    column-gap: calc(12px * var(--k));
    align-items: center;
    padding-bottom: calc(8px * var(--k));
    border-bottom: 1px solid rgba(229, 97, 92, 0.45);
  }
  .ico {
    width: calc(32px * var(--k));
    height: calc(30px * var(--k));
    fill: none;
    stroke: var(--ph-danger);
    stroke-width: 2.6;
    stroke-linejoin: round;
    stroke-linecap: round;
    filter: drop-shadow(0 0 4px rgba(229, 97, 92, 0.6));
  }
  .ico .idot {
    fill: var(--ph-danger);
    stroke: none;
  }
  .ft {
    min-width: 0;
  }
  .ftitle {
    font-family: var(--ph-display);
    font-weight: 400;
    font-size: calc(19px * var(--k));
    line-height: 1.15;
    letter-spacing: 0.01em;
    color: var(--ph-danger);
    text-shadow:
      0 0 8px rgba(229, 97, 92, 0.5),
      0 0 22px rgba(229, 97, 92, 0.2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fsub {
    margin-top: calc(3px * var(--k));
    font-size: var(--ph-fs-m);
    color: var(--ph-ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .code {
    color: #ff8f88;
  }
  .ago {
    align-self: end;
    font-size: var(--ph-fs-m);
    color: var(--ph-danger);
    white-space: nowrap;
  }
  .fbody {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.15fr);
    column-gap: calc(16px * var(--k));
    min-height: 0;
    padding-top: calc(8px * var(--k));
  }
  .trace {
    position: relative;
    min-height: 0;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
  }
  .trace:has(.stepbox) {
    grid-template-rows: auto minmax(0, 1fr);
    gap: calc(6px * var(--k));
  }
  .stepbox {
    min-height: 0;
    overflow: hidden;
  }
  .lbl.sm {
    font-size: var(--ph-fs-xs);
  }
  .logbox {
    min-height: 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    gap: calc(5px * var(--k));
    padding-left: calc(14px * var(--k));
    border-left: 1px solid rgba(229, 97, 92, 0.3);
  }
  .lines {
    min-height: 0;
    overflow: hidden;
  }
  .ln {
    height: var(--lh);
    line-height: var(--lh);
    font-size: max(10.5px, calc(11px * var(--k)));
    color: #d8ecf2;
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
