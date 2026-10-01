<script lang="ts">
  // The KLIF shell: mock player, skin host, shared utility surfaces (console, tune, toast, dev bar),
  // tier driver and shortcuts. Skins only ever see SkinProps {vm, actions, size}.
  import { onMount } from 'svelte';
  import ConsoleDrawer from './lib/shell/ConsoleDrawer.svelte';
  import DevBar from './lib/shell/DevBar.svelte';
  import { installKeys } from './lib/shell/keys';
  import SkinHost from './lib/shell/SkinHost.svelte';
  import { installTierDriver, refreshTier } from './lib/shell/tier';
  import Toast from './lib/shell/Toast.svelte';
  import TuneDrawer from './lib/shell/TuneDrawer.svelte';
  import { player } from './lib/state/player.svelte';
  import { ui } from './lib/state/ui.svelte';

  // Start the player before the first render so the first frame already shows the right scenario.
  player.init(ui.params);

  onMount(() => {
    const offTier = installTierDriver();
    const offKeys = installKeys();
    const onResize = () => {
      ui.vw = innerWidth;
      ui.vh = innerHeight;
    };
    window.addEventListener('resize', onResize);
    onResize();
    return () => {
      offTier();
      offKeys();
      window.removeEventListener('resize', onResize);
      player.stopTimer();
    };
  });

  // Frameless host (?frameless=1 now, the Tauri window later): mirrored for CSS, the skin draws its own chrome.
  $effect(() => {
    const root = document.documentElement;
    if (player.vm.host.frameless) root.dataset.frameless = '1';
    else delete root.dataset.frameless;
  });

  // Pinning or unpinning a tier (dev bar, ?tier=) re-evaluates the driver.
  $effect(() => {
    void ui.tierPin;
    refreshTier();
  });

  // mini on a viewport that is not the panel: letterbox the 960x640 design size, scaled down to fit.
  const scale = $derived(Math.min(1, (ui.vw - 24) / 960, (ui.vh - 24) / 640));
</script>

<div class="stage" data-size={ui.size} class:framed={ui.miniFramed}>
  <div class="frame" style={ui.miniFramed ? `transform: translate(-50%, -50%) scale(${scale})` : ''}>
    <SkinHost skinId={ui.skinId} vm={player.vm} actions={player.actions} size={ui.size} />
    {#if ui.size === 'full'}
      {#if ui.consoleOpen}<ConsoleDrawer />{/if}
      {#if ui.tuneOpen}<TuneDrawer />{/if}
    {/if}
  </div>
  {#if ui.size === 'full'}<Toast />{/if}
  {#if ui.devbar && !ui.shot}<DevBar />{/if}
</div>

<style>
  .stage {
    position: fixed;
    inset: 0;
    overflow: hidden;
    background: var(--k-bg, #0d1117);
  }
  .frame {
    position: absolute;
    inset: 0;
    overflow: hidden;
  }
  .framed .frame {
    inset: auto;
    left: 50%;
    top: 50%;
    width: 960px;
    height: 640px;
    outline: 1px solid var(--k-line, #2e2e2e);
    box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.35);
  }
</style>
