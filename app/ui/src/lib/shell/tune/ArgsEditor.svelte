<script lang="ts">
  // The argument list: one input per token, exactly one argv entry each (never split, merged or re-tokenized).
  // A flag and its value share a row for reading only; rows regroup when focus leaves the list or the list
  // changes shape. Rows move up/down and go away as a whole. Raw = one token per line.
  import { tick, untrack } from 'svelte';
  import { groupRows, isFlagToken, rowStarts, tokensFromLines } from './args';
  import { secretArgIndexes } from './secret';

  interface Props {
    /** The draft's token list (bound), edited in place. */
    args: string[];
    readonly?: boolean;
  }
  let { args = $bindable(), readonly = false }: Props = $props();

  let raw = $state(false);
  let rawText = $state('');
  let rows = $state<number[]>([]);
  let box = $state<HTMLDivElement | undefined>();

  // Another list arrived (another preset, Revert, Reload): group it afresh.
  $effect(() => {
    const a = args;
    untrack(() => {
      rows = groupRows(a);
      if (raw) rawText = a.join('\n');
    });
  });

  const sum = (r: number[]) => r.reduce((a, n) => a + n, 0);
  // Rows always partition the tokens, whatever happened meanwhile.
  const view = $derived(sum(rows) === args.length ? rows : groupRows(args));
  const starts = $derived(rowStarts(view));
  const secret = $derived(secretArgIndexes(args));

  function regroup() {
    const g = groupRows(args);
    if (g.join() !== view.join() || sum(rows) !== args.length) rows = g;
  }

  function onFocusOut(e: FocusEvent) {
    if (box && !box.contains(e.relatedTarget as Node | null)) regroup();
  }

  async function focusToken(i: number) {
    await tick();
    box?.querySelector<HTMLInputElement>(`input[data-i="${i}"]`)?.focus();
  }

  async function focusRowButton(r: number, which: 'up' | 'down') {
    await tick();
    box?.querySelector<HTMLButtonElement>(`[data-row="${r}"] .${which}`)?.focus();
  }

  function addToken(after?: number) {
    const cur = [...view];
    if (after === undefined) {
      args.push('');
      rows = [...cur, 1];
      void focusToken(args.length - 1);
      return;
    }
    // Insert after token `after`, as its own row right after the row holding it.
    let r = 0;
    while (r < cur.length && starts[r] + cur[r] <= after) r++;
    const at = starts[r] + cur[r];
    args.splice(at, 0, '');
    cur.splice(r + 1, 0, 1);
    rows = cur;
    void focusToken(at);
  }

  function removeRow(r: number) {
    const cur = [...view];
    args.splice(starts[r], cur[r]);
    cur.splice(r, 1);
    rows = cur;
  }

  function moveRow(r: number, dir: -1 | 1) {
    const o = r + dir;
    const cur = [...view];
    if (o < 0 || o >= cur.length) return;
    const [a, b] = dir < 0 ? [o, r] : [r, o];
    const first = args.slice(starts[a], starts[a] + cur[a]);
    const second = args.slice(starts[b], starts[b] + cur[b]);
    args.splice(starts[a], cur[a] + cur[b], ...second, ...first);
    [cur[a], cur[b]] = [cur[b], cur[a]];
    rows = cur;
    void focusRowButton(o, dir < 0 ? 'up' : 'down');
  }

  function onTokKey(e: KeyboardEvent, i: number) {
    if (e.key === 'Enter' && !readonly) {
      e.preventDefault();
      addToken(i);
    }
  }

  function setRaw(on: boolean) {
    if (on) rawText = args.join('\n');
    else rows = groupRows(args);
    raw = on;
  }

  function onRawInput(text: string) {
    rawText = text;
    args.splice(0, args.length, ...tokensFromLines(text));
  }
</script>

<div class="field args">
  <span id="tune-args-lbl">Args</span>
  <div class="argbox" bind:this={box} onfocusout={onFocusOut}>
    {#if raw}
      <textarea
        aria-labelledby="tune-args-lbl"
        aria-describedby="tune-args-rawhint"
        class="rawargs"
        rows={Math.min(18, Math.max(4, args.length + 1))}
        spellcheck="false"
        autocomplete="off"
        {readonly}
        value={rawText}
        oninput={(e) => onRawInput(e.currentTarget.value)}
      ></textarea>
      <small id="tune-args-rawhint" class="hint">One token per line, exactly as the program receives it.</small>
    {:else}
      {#if args.length === 0}<p class="hint none">No arguments.</p>{/if}
      <ol class="toks" aria-labelledby="tune-args-lbl">
        {#each view as len, r (r)}
          {@const s = starts[r]}
          <li class="row" data-row={r}>
            <span class="cells">
              {#each { length: len } as _, k (k)}
                {@const i = s + k}
                <input
                  class="tok"
                  class:flag={isFlagToken(args[i] ?? '')}
                  class:pair={len === 2}
                  data-i={i}
                  type={secret.has(i) ? 'password' : 'text'}
                  value={args[i] ?? ''}
                  spellcheck="false"
                  autocomplete="off"
                  aria-label="Argument {i + 1}"
                  {readonly}
                  oninput={(e) => (args[i] = e.currentTarget.value)}
                  onkeydown={(e) => onTokKey(e, i)}
                />
              {/each}
            </span>
            {#if !readonly}
              <span class="rowbtns">
                <button type="button" class="up mini" aria-label="Move up" title="Move up" disabled={r === 0} onclick={() => moveRow(r, -1)}>↑</button>
                <button type="button" class="down mini" aria-label="Move down" title="Move down" disabled={r === view.length - 1} onclick={() => moveRow(r, 1)}>↓</button>
                <button type="button" class="mini" aria-label="Remove" title="Remove" onclick={() => removeRow(r)}>×</button>
              </span>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}
    <div class="argfoot">
      {#if !readonly && !raw}<button type="button" class="mini wide" onclick={() => addToken()}>+ arg</button>{/if}
      <span class="grow"></span>
      <button type="button" class="switch small" role="switch" aria-checked={raw} class:on={raw} onclick={() => setRaw(!raw)}><i></i>Raw</button>
    </div>
  </div>
</div>

<style>
  .field.args {
    align-items: start;
  }
  .field.args > span {
    padding-top: 9px;
  }
  .argbox {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .toks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .cells {
    flex: 1;
    display: flex;
    gap: 4px;
    min-width: 0;
  }
  .tok {
    flex: 1 1 0;
    min-width: 0;
    font-family: var(--k-font-data, ui-monospace, monospace);
    font-size: 12px;
    padding: 6px 8px;
  }
  .tok.flag {
    font-weight: 600;
  }
  .tok.flag.pair {
    flex: 0 1 40%;
  }
  .rowbtns {
    display: flex;
    gap: 2px;
    flex: none;
  }
  .rawargs {
    width: 100%;
    font-family: var(--k-font-data, ui-monospace, monospace);
    font-size: 12px;
    line-height: 1.5;
    resize: vertical;
  }
  .argfoot {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .none {
    margin: 0;
  }
</style>
