<script lang="ts">
  // The KLIF shell: player (native core in Tauri, mock in a browser), skin host, shared utility surfaces
  // (console, tune, toast, dev bar), tier driver and shortcuts. Skins only ever see SkinProps {vm, actions, size}.
  import { onMount } from 'svelte';
  import ConsoleDrawer from './lib/shell/ConsoleDrawer.svelte';
  import DevBar from './lib/shell/DevBar.svelte';
  import { installKeys } from './lib/shell/keys';
  import RecordsView from './lib/shell/records/RecordsView.svelte';
  import SkinHost from './lib/shell/SkinHost.svelte';
  import { installTierDriver, refreshTier } from './lib/shell/tier';
  import RecordMoment from './lib/shell/RecordMoment.svelte';
  import Toast from './lib/shell/Toast.svelte';
  import TuneDrawer from './lib/shell/TuneDrawer.svelte';
  import { player } from './lib/state/player.svelte';
  import { installSkinSync } from './lib/state/skinSync.svelte';
  import { ui } from './lib/state/ui.svelte';

  // Start the player before the first render so the first frame already shows the right scenario.
  player.init(ui.params);
  // The window's skin goes to klif.toml ([ui] skin) for klif-webui; a change made there is followed.
  installSkinSync();

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

  // Frameless host (?frameless=1 in a browser, the Tauri window): mirrored for CSS, the skin draws its own chrome.
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
  {#if player.ready}
    <div class="frame" style={ui.miniFramed ? `transform: translate(-50%, -50%) scale(${scale})` : ''}>
      <SkinHost skinId={ui.skinId} vm={player.vm} actions={player.actions} size={ui.size} />
      {#if ui.consoleOpen && ui.size === 'full'}<ConsoleDrawer />{/if}
      <!-- Mini/panel never shows Tune, but an open drawer stays mounted (hidden) so its unapplied drafts survive a
           switch to mini and back; they are only ever dropped through the drawer's discard confirm. -->
      {#if ui.tuneOpen}<div class="tune-host" class:off={ui.size !== 'full'}><TuneDrawer /></div>{/if}
      <!-- Records is a full-window screen over the skin (and over Tune and the console): full size only, and not
           mounted in mini, so closing it by switching layout loses nothing but its view state. -->
      {#if ui.recordsOpen && ui.size === 'full'}<RecordsView />{/if}
    </div>
  {:else}
    <!-- Native host before the core's first view model: no invented numbers, just the window. -->
    <div class="waiting" data-tauri-drag-region="deep">
      <p>{player.nativeError ? `KLIF core unavailable: ${player.nativeError}` : (player.nativeWaiting ?? 'Connecting to the KLIF core')}</p>
    </div>
  {/if}
  <!-- Every layout: a refusal of a mini Launch / Stop / Restart is otherwise invisible. -->
  <!-- Every layout: a broken record is celebrated on the panel too. -->
  {#if player.ready}<RecordMoment />{/if}
  <Toast />
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
  .tune-host {
    display: contents;
  }
  .tune-host.off {
    display: none;
  }
  .waiting {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  .waiting p {
    margin: 0;
    color: var(--k-muted, #8a8a8a);
    font: 500 13px/1.3 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.04em;
  }
</style>
