<script lang="ts">
  // Pairing: the QR code from KLIF brings the secret in the address (handled by App before this shows); here the
  // 6-digit code is typed instead.
  import { client, guessDeviceName } from './client.svelte';

  let { error = '' }: { error?: string } = $props();
  let code = $state('');
  let name = $state(guessDeviceName());
  let busy = $state(false);
  let err = $state('');
  $effect(() => {
    err = error;
  });

  async function pair(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    err = '';
    try {
      await client.pair(code, name);
    } catch (x) {
      err = x instanceof Error ? x.message : String(x);
    } finally {
      busy = false;
    }
  }
</script>

<main class="pair">
  <h1>KLIF</h1>
  <p class="lead">Pair this device to see and control KLIF from here.</p>
  <ol>
    <li>In KLIF on the computer, open <b>Tune</b>, then <b>Web UI</b>, and choose <b>Pair a device</b>.</li>
    <li>Scan the QR code with this device, or type the 6-digit code below.</li>
  </ol>
  <form onsubmit={pair}>
    <label class="field">
      <span>Code</span>
      <input
        bind:value={code}
        inputmode="numeric"
        autocomplete="one-time-code"
        maxlength="7"
        placeholder="000000"
        required
        aria-describedby={err ? 'pair-err' : undefined}
      />
    </label>
    <label class="field">
      <span>Name of this device</span>
      <input bind:value={name} maxlength="48" />
    </label>
    <button type="submit" class="btn main go" disabled={busy || code.replace(/\D/g, '').length !== 6}>{busy ? 'Pairing…' : 'Pair'}</button>
    {#if err}<p class="err" id="pair-err" role="alert">{err}</p>{/if}
  </form>
  <p class="fine">
    Anyone on this network can open this page, but only paired devices see or control anything. The connection is not
    encrypted: use it on a network you trust.
  </p>
</main>

<style>
  .pair {
    max-width: 440px;
    margin: 0 auto;
    padding: 28px 20px 40px;
  }
  h1 {
    margin: 0 0 6px;
    font: 800 28px/1 var(--k-font-display);
    letter-spacing: 0.06em;
  }
  .lead {
    margin: 0 0 14px;
    font-size: 17px;
    line-height: 1.4;
  }
  ol {
    margin: 0 0 6px;
    padding-left: 20px;
    font-size: 15px;
    line-height: 1.5;
    color: var(--k-muted);
  }
  ol b {
    color: var(--k-ink);
  }
  input[inputmode='numeric'] {
    font: 600 24px var(--k-font-data);
    letter-spacing: 0.2em;
  }
  .fine {
    margin: 22px 0 0;
    font-size: 13px;
    line-height: 1.45;
    color: var(--k-muted);
  }
</style>
