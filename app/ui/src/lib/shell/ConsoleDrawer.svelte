<script lang="ts">
  // Bottom-sheet console: the last 200 server lines, monospace, following the tail only while the
  // reader is parked at the bottom. Themed purely by the --k-* variables.
  import { tick } from 'svelte';
  import { fly } from 'svelte/transition';
  import { player } from '../state/player.svelte';
  import { selectedSystem } from '../model/systems';
  import { ui } from '../state/ui.svelte';

  const lines = $derived(player.vm.console);
  /** The console is the selected System's. */
  const sys = $derived(selectedSystem(player.vm));
  let box = $state<HTMLDivElement | undefined>();
  let stick = $state(true);

  function atBottom(el: HTMLDivElement) {
    return el.scrollHeight - el.scrollTop - el.clientHeight < 6;
  }

  function onScroll() {
    if (box) stick = atBottom(box);
  }

  function toBottom() {
    if (!box) return;
    box.scrollTop = box.scrollHeight;
    stick = true;
  }

  $effect(() => {
    void lines;
    if (stick && box) void tick().then(() => box && (box.scrollTop = box.scrollHeight));
  });

  // Fresh open: start on the tail.
  $effect(() => {
    if (box) void tick().then(toBottom);
  });

  function level(line: string): 'err' | 'warn' | '' {
    if (/^\S+ E /.test(line) || /^\[ERROR\]/.test(line) || /\bROCm error\b|NativeCommandError|exited with code/.test(line)) return 'err';
    if (/^\S+ W /.test(line) || /^\[WARN/.test(line)) return 'warn';
    return '';
  }

  async function copyAll() {
    try {
      await navigator.clipboard.writeText(lines.join('\n'));
      ui.toast('Console copied');
    } catch {
      ui.toast('Clipboard is not available here.');
    }
  }
</script>

<div class="sheet" role="dialog" aria-label="Console" transition:fly|global={{ y: 340, duration: 220, opacity: 1 }}>
  <header>
    <h2>Console{#if sys}<span class="of"> · {sys.label}</span>{/if}</h2>
    <span class="count">{lines.length} / 200 lines</span>
    <span class="grow"></span>
    <button type="button" class="ghost" onclick={copyAll}>Copy</button>
    <button type="button" class="ghost" aria-label="Close console" onclick={() => ui.toggleConsole(false)}>Close</button>
  </header>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable log must be keyboard focusable) -->
  <div class="body" bind:this={box} onscroll={onScroll} tabindex="0" aria-label="Server output">
    {#if lines.length === 0}
      <p class="empty">No output yet. Launch a slot to see its log.</p>
    {/if}
    {#each lines as line, i (i)}
      <div class="line {level(line)}">{line}</div>
    {/each}
  </div>
  {#if !stick}
    <button type="button" class="jump" onclick={toBottom}>Latest</button>
  {/if}
</div>

<style>
  .sheet {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: min(46%, 440px);
    min-height: 180px;
    display: flex;
    flex-direction: column;
    background: var(--k-surface-raised, #1a1a1a);
    color: var(--k-ink, #e6e6e6);
    border-top: 1px solid var(--k-line, #2e2e2e);
    box-shadow: 0 -12px 36px rgba(0, 0, 0, 0.4);
    z-index: 50;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--k-line, #2e2e2e);
    font-family: var(--k-font-ui, system-ui, sans-serif);
  }
  h2 {
    margin: 0;
    font: 600 12px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }
  .count {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1 var(--k-font-data, ui-monospace, monospace);
  }
  .grow {
    flex: 1;
  }
  button {
    font: 500 12px/1 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
    background: transparent;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 7px 12px;
    cursor: pointer;
  }
  button:hover {
    border-color: var(--k-accent, #5ab6eb);
    color: var(--k-accent, #5ab6eb);
  }
  button:focus-visible,
  .body:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
    outline-offset: 1px;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 10px 16px 14px;
    font: 400 12.5px/1.5 var(--k-font-data, ui-monospace, monospace);
    background: var(--k-surface, #141414);
    overscroll-behavior: contain;
  }
  .line {
    white-space: pre;
    color: var(--k-ink, #e6e6e6);
    opacity: 0.88;
  }
  .line.warn {
    color: var(--k-warn, #f2a33a);
    opacity: 1;
  }
  .line.err {
    color: var(--k-danger, #e05a5a);
    opacity: 1;
  }
  .empty {
    margin: 0;
    color: var(--k-muted, #8a8a8a);
    font-family: var(--k-font-ui, system-ui, sans-serif);
  }
  .jump {
    position: absolute;
    right: 20px;
    bottom: 14px;
    background: var(--k-accent, #5ab6eb);
    color: var(--k-accent-ink, #0d1117);
    border-color: transparent;
    font-weight: 600;
  }
  .jump:hover {
    color: var(--k-accent-ink, #0d1117);
    filter: brightness(1.08);
  }
  .of {
    color: var(--k-muted);
    font-weight: 400;
  }
</style>
