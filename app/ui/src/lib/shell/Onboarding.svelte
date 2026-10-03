<script lang="ts">
  // Shown instead of a skin while the view model has no Systems (a fresh install): one quiet card in the
  // active skin's --k-* tokens. "Add a System" opens Tune in add mode; the mini layout (no Tune) only explains.
  import type { SizeClass } from '../../skins/contract';
  import type { Actions, ViewModel } from '../model/types';

  interface Props {
    vm: ViewModel;
    actions: Actions;
    size: SizeClass;
  }
  let { vm, actions, size }: Props = $props();
  const mini = $derived(size === 'mini');
</script>

<div class="onb" class:mini data-tauri-drag-region>
  <div class="card" data-tauri-drag-region>
    <h1>No Systems yet.</h1>
    <p>A System is one server KLIF starts and watches: an LLM, image generation, speech, transcription or video.</p>
    {#if mini}
      <p class="hint">Add one in the KLIF window, or in klif.toml.</p>
    {:else}
      <button type="button" onclick={() => actions.openTune(undefined, { add: true })}>Add a System</button>
      <p class="hint">Or write a <code>[systems.s1]</code> table in klif.toml: KLIF reloads it as you save.</p>
    {/if}
  </div>
  {#if vm.host.frameless}
    <div class="win">
      <button type="button" aria-label="Minimize" onclick={() => actions.minimize()}>&minus;</button>
      <button type="button" aria-label="Close" onclick={() => actions.closeWindow()}>&times;</button>
    </div>
  {/if}
</div>

<style>
  .onb {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    background: var(--k-bg);
    color: var(--k-ink);
    font-family: var(--k-font-ui);
  }
  .card {
    max-width: 420px;
    text-align: center;
  }
  .mini .card {
    max-width: 640px;
  }
  h1 {
    margin: 0 0 8px;
    font: 500 20px/1.2 var(--k-font-display, var(--k-font-ui));
    letter-spacing: 0.02em;
  }
  .mini h1 {
    font-size: 40px;
  }
  p {
    margin: 0 0 14px;
    color: var(--k-muted);
    font-size: 14px;
    line-height: 1.45;
  }
  .mini p {
    font-size: 24px;
  }
  .hint {
    margin: 12px 0 0;
    font-size: 12px;
  }
  .mini .hint {
    font-size: 20px;
  }
  code {
    font-family: var(--k-font-data, monospace);
    color: var(--k-ink);
  }
  button {
    font: 500 13px/1 var(--k-font-ui);
    letter-spacing: 0.04em;
    color: var(--k-accent-ink, var(--k-bg));
    background: var(--k-accent);
    border: 1px solid var(--k-accent);
    border-radius: var(--k-radius, 4px);
    padding: 9px 16px;
    cursor: pointer;
  }
  button:hover {
    filter: brightness(1.08);
  }
  button:focus-visible {
    outline: 2px solid var(--k-accent);
    outline-offset: 2px;
  }
  .win {
    position: absolute;
    top: 8px;
    right: 8px;
    display: flex;
    gap: 4px;
  }
  .win button {
    padding: 4px 10px;
    font-size: 15px;
    color: var(--k-muted);
    background: transparent;
    border-color: var(--k-line);
  }
</style>
