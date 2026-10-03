<script lang="ts">
  // The ⋯ menu of a row: a small popover list. Arrow keys move, Escape closes it (and only it), a click
  // outside closes it.
  import { tick } from 'svelte';
  import type { MenuItem } from './types';

  interface Props {
    items: MenuItem[];
    /** Accessible name of the trigger, e.g. "System actions". */
    label: string;
    disabled?: boolean;
  }
  let { items, label, disabled = false }: Props = $props();

  let open = $state(false);
  let root = $state<HTMLDivElement | undefined>();
  let trigger = $state<HTMLButtonElement | undefined>();
  let list = $state<HTMLDivElement | undefined>();

  function buttons(): HTMLButtonElement[] {
    return list ? [...list.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')] : [];
  }

  async function toggle() {
    open = !open;
    if (open) {
      await tick();
      buttons()[0]?.focus();
    }
  }

  function close(refocus = true) {
    open = false;
    if (refocus) trigger?.focus();
  }

  function pick(it: MenuItem) {
    if (it.disabled) return;
    close();
    it.onselect();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const b = buttons();
      const i = b.indexOf(document.activeElement as HTMLButtonElement);
      const next = e.key === 'ArrowDown' ? (i + 1) % b.length : (i - 1 + b.length) % b.length;
      b[next]?.focus();
    }
    if (e.key === 'Tab') close(false);
  }

  function onOutside(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) close(false);
  }
</script>

<svelte:window onpointerdown={onOutside} />

<div class="mwrap" bind:this={root}>
  <button
    type="button"
    class="dots"
    bind:this={trigger}
    aria-haspopup="menu"
    aria-expanded={open}
    aria-label={label}
    title={label}
    {disabled}
    onclick={toggle}>⋯</button
  >
  {#if open}
    <div class="menu" role="menu" aria-label={label} tabindex="-1" bind:this={list} onkeydown={onKey}>
      {#each items as it (it.label)}
        <button
          type="button"
          role={it.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
          aria-checked={it.checked === undefined ? undefined : it.checked}
          class:danger={it.danger}
          disabled={it.disabled}
          title={it.hint ?? ''}
          onclick={() => pick(it)}
        >
          {#if it.checked !== undefined}<i class="check" class:on={it.checked} aria-hidden="true"></i>{/if}
          <span>{it.label}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .mwrap {
    position: relative;
    flex: none;
  }
  .dots {
    padding: 6px 10px;
    font-size: 15px;
    line-height: 1;
    letter-spacing: 0.05em;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 5;
    min-width: 190px;
    display: grid;
    padding: 4px;
    background: var(--k-surface-raised, #1a1a1a);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.4);
  }
  .menu button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    border-color: transparent;
    padding: 8px 10px;
    white-space: nowrap;
  }
  .menu button:hover:not(:disabled),
  .menu button:focus-visible {
    background: var(--k-surface, #141414);
    border-color: var(--k-line, #2e2e2e);
  }
  .menu button.danger {
    color: var(--k-danger, #e05a5a);
  }
  .check {
    width: 12px;
    height: 12px;
    border-radius: 3px;
    border: 1px solid var(--k-line, #2e2e2e);
    flex: none;
  }
  .check.on {
    background: var(--k-accent, #5ab6eb);
    border-color: var(--k-accent, #5ab6eb);
  }
</style>
