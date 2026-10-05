<script lang="ts">
  // klif-webui: pair (QR secret in the address, or the typed code), then the stack with one System's sheet at a time.
  import { onMount } from 'svelte';
  import { client, guessDeviceName } from './client.svelte';
  import Pair from './Pair.svelte';
  import Sheet from './Sheet.svelte';
  import Stack from './Stack.svelte';
  import { applySkin } from './theme';

  let pairError = $state('');
  let open = $state<string | null>(null);
  let booting = $state(true);

  onMount(() => {
    void applySkin();
    void (async () => {
      // The QR code's secret: read it from the # part, take it out of the address at once, pair with it.
      const m = /^#pair=([0-9a-f]{64})$/i.exec(location.hash);
      if (m) {
        history.replaceState(null, '', location.pathname + location.search);
        try {
          await client.pair(m[1], guessDeviceName());
        } catch (e) {
          pairError = e instanceof Error ? e.message : String(e);
        }
      }
      if (client.paired) client.start();
      booting = false;
    })();
    return () => client.stop();
  });

  $effect(() => {
    void applySkin(client.state?.skin);
  });

  // The poll is quiet while the page is hidden; coming back asks at once.
  onMount(() => {
    const vis = () => document.visibilityState === 'visible' && client.paired && client.refresh();
    document.addEventListener('visibilitychange', vis);
    return () => document.removeEventListener('visibilitychange', vis);
  });

  // Set while KLIF does not answer the poll.
  const stale = $derived(client.down);
</script>

{#if booting}
  <p class="wait">Connecting to KLIF…</p>
{:else if !client.paired}
  <Pair error={pairError} />
{:else if !client.state}
  <p class="wait">{client.down ?? 'Connecting to KLIF…'}</p>
{:else}
  <Stack web={client.state} {stale} onopen={(id) => (open = id)} />
  <footer>
    <span>Paired as “{client.state.device.name}”</span>
    <button type="button" class="link" onclick={() => void client.forget()}>Unpair this device</button>
  </footer>
  {#if open}<Sheet web={client.state} id={open} onclose={() => (open = null)} />{/if}
{/if}

<style>
  .wait {
    padding: 40px 20px;
    text-align: center;
    font-size: 16px;
    color: var(--k-muted);
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    padding: 14px 16px calc(18px + env(safe-area-inset-bottom));
    font-size: 13px;
    color: var(--k-muted);
  }
  .link {
    min-height: 40px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--k-accent);
    font-size: 13px;
    text-decoration: underline;
    cursor: pointer;
  }
</style>
