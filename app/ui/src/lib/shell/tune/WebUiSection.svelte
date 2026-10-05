<script lang="ts">
  // klif-webui on this machine ([webui] in klif.toml): on or off, where it listens, pairing a phone or browser by QR
  // code or 6-digit code, and the paired devices.
  import { onDestroy } from 'svelte';
  import type { WebUiInfo } from '../../model/types';
  import { encodeQr, qrPath } from '../../qr';
  import { player } from '../../state/player.svelte';
  import Section from './Section.svelte';
  import { getTune } from './state.svelte';
  import { attempt, QUIET } from './util';

  interface Props {
    info: WebUiInfo;
  }
  let { info }: Props = $props();
  const t = getTune();

  let busy = $state(false);
  let err = $state('');
  let host = $state('');
  let port = $state('');
  let editedFor = '';
  // The address fields follow the config until the user types in them.
  $effect(() => {
    const now = `${info.host}|${info.port}`;
    if (now !== editedFor) {
      editedFor = now;
      host = info.host;
      port = String(info.port);
    }
  });
  const dirty = $derived(host.trim() !== info.host || port.trim() !== String(info.port));

  let clock = $state(Date.now() / 1000);
  const timer = setInterval(() => (clock = Date.now() / 1000), 1000);
  onDestroy(() => clearInterval(timer));
  const left = $derived(info.pairing ? Math.max(0, Math.round(info.pairing.expiresAt - clock)) : 0);

  const qr = $derived.by(() => {
    if (!info.pairing?.url) return null;
    try {
      const m = encodeQr(info.pairing.url, 'M');
      return { size: m.length + 8, d: qrPath(m, 4) };
    } catch {
      return null;
    }
  });

  async function run(f: () => Promise<void>) {
    busy = true;
    err = await attempt(f);
    busy = false;
  }

  const toggle = () => run(() => player.actions.updateSettings({ webuiEnabled: !info.enabled }, QUIET));
  function saveAddress() {
    const p = Number(port.trim());
    if (!Number.isInteger(p) || p < 1 || p > 65535) {
      err = 'The port is a number from 1 to 65535.';
      return;
    }
    void run(() => player.actions.updateSettings({ webuiHost: host.trim() || '0.0.0.0', webuiPort: p }, QUIET));
  }
  const pair = () => run(() => player.actions.pairWebDevice(QUIET));
  const cancelPair = () => run(() => player.actions.cancelWebPairing(QUIET));

  async function forget(id?: string, name?: string) {
    const ok = await t.confirm({
      title: id ? `Remove ${name ?? 'this device'}?` : 'Remove every paired device?',
      detail: 'It stops seeing and controlling KLIF at once and has to pair again.',
      confirm: id ? 'Remove' : 'Remove all',
      danger: true,
    });
    if (ok) void run(() => player.actions.forgetWebDevice(id, QUIET));
  }

  const date = (s: number) => new Date(s * 1000).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  function ago(s?: number): string {
    if (!s) return 'never';
    const d = Math.max(0, clock - s);
    if (d < 90) return 'just now';
    if (d < 3600) return `${Math.round(d / 60)} min ago`;
    if (d < 172800) return `${Math.round(d / 3600)} h ago`;
    return date(s);
  }
  const headline = $derived(
    !info.enabled ? 'off' : info.listening ? (info.urls[0] ?? `port ${info.port}`) : info.error ? 'not serving' : 'starting',
  );
</script>

<Section id="tune-webui" title="Web UI" open={t.webuiOpen} ontoggle={(o) => (t.webuiOpen = o)}>
  {#snippet summary()}{headline}{/snippet}
  <div class="row">
    <div class="desc">
      <b>klif-webui</b>
      <span>A simple page for a phone or a browser on this network: every System's status, launch, stop, restart, preset and params.</span>
    </div>
    <button
      type="button"
      class="switch"
      class:on={info.enabled}
      role="switch"
      aria-checked={info.enabled}
      aria-label="klif-webui"
      disabled={busy}
      onclick={() => void toggle()}
    >
      <i></i>{info.enabled ? 'on' : 'off'}
    </button>
  </div>
  {#if info.enabled}
    {#if info.listening && info.urls.length}
      <p class="where">Open <code>{info.urls[0]}</code> on the device.</p>
    {:else if info.error}
      <p class="err" role="alert">{info.error}</p>
    {/if}
    <div class="addr">
      <label>
        <span>Address</span>
        <input bind:value={host} spellcheck="false" placeholder="0.0.0.0" aria-describedby="webui-addr-hint" />
      </label>
      <label class="port">
        <span>Port</span>
        <input bind:value={port} inputmode="numeric" maxlength="5" placeholder="7341" />
      </label>
      <button type="button" class="mini text primary" disabled={busy || !dirty} onclick={saveAddress}>Save</button>
    </div>
    <p class="hint" id="webui-addr-hint">0.0.0.0 listens on every network of this machine; an IP address limits it to that network.</p>

    <div class="pair">
      {#if info.pairing}
        <div class="qr">
          {#if qr}
            <svg viewBox="0 0 {qr.size} {qr.size}" role="img" aria-label="QR code that opens the page and pairs the device" shape-rendering="crispEdges">
              <rect width={qr.size} height={qr.size} fill="#fff" />
              <path d={qr.d} fill="#000" />
            </svg>
          {:else}
            <p class="hint">No QR code: KLIF does not know this machine's network address. Type the code instead.</p>
          {/if}
        </div>
        <div class="code">
          <span class="lbl">Scan with the phone, or open the page and type</span>
          <b>{info.pairing.code.slice(0, 3)} {info.pairing.code.slice(3)}</b>
          <span class="lbl">{left > 0 ? `Valid for ${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}` : 'Expired'}</span>
          <button type="button" class="mini text" disabled={busy} onclick={() => void cancelPair()}>Cancel</button>
        </div>
      {:else}
        <button type="button" class="mini text primary" disabled={busy || !info.listening} onclick={() => void pair()}>Pair a device</button>
        <span class="hint">Shows a QR code and a 6-digit code for 5 minutes.</span>
      {/if}
    </div>
  {/if}
  {#if info.devices.length}
    <ul class="devices">
      {#each info.devices as d (d.id)}
        <li>
          <b>{d.name}</b>
          <span>paired {date(d.pairedAt)} · seen {ago(d.lastSeen)}</span>
          <button type="button" class="mini text" disabled={busy} onclick={() => void forget(d.id, d.name)}>Remove</button>
        </li>
      {/each}
    </ul>
    {#if info.devices.length > 1}
      <button type="button" class="mini text all" disabled={busy} onclick={() => void forget()}>Remove all devices</button>
    {/if}
  {/if}
  {#if err}<p class="err" role="alert">{err}</p>{/if}
  <p class="hint">
    Anyone on the network can open the page, but only paired devices see or control anything. The connection is not
    encrypted: use it on a network you trust, never forwarded to the internet.
  </p>
</Section>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 2px 0 10px;
  }
  .desc {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  b {
    font: 600 13px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  .desc span,
  .hint,
  .devices span,
  .lbl {
    font: 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .where {
    margin: 0 0 8px;
    font: 12.5px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  code {
    font: 500 12.5px var(--k-font-data, ui-monospace, monospace);
    color: var(--k-accent, #5ab6eb);
    user-select: all;
  }
  .addr {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .addr label {
    display: grid;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }
  .addr label.port {
    flex: 0 0 76px;
  }
  .addr span {
    font: 600 10px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  input {
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    background: var(--k-bg, #111);
    color: var(--k-ink, #eee);
    font: 12.5px var(--k-font-data, ui-monospace, monospace);
    min-width: 0;
  }
  .hint {
    margin: 6px 0 0;
  }
  .pair {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 14px;
    margin-top: 12px;
    padding: 10px;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
  }
  .qr svg {
    display: block;
    width: 168px;
    height: 168px;
  }
  .code {
    display: grid;
    gap: 6px;
    justify-items: start;
  }
  .code b {
    font: 600 28px/1 var(--k-font-data, ui-monospace, monospace);
    letter-spacing: 0.12em;
  }
  .devices {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
  }
  .devices li {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 6px 0;
    border-top: 1px solid var(--k-line, #2e2e2e);
  }
  .devices span {
    flex: 1;
    min-width: 0;
  }
  .all {
    margin-top: 6px;
  }
  .err {
    margin: 6px 0 0;
    color: var(--k-danger, #e05a5a);
    font: 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
</style>
