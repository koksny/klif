<script lang="ts">
  // The machine at the top of Records: its FP32 TFLOPS total as the hero number, every counted device to scale,
  // then the VRAM pool and the RAM.
  import type { HardwareInfo } from '../../model/types';

  let { hw, name }: { hw: HardwareInfo | undefined; name: string } = $props();

  const devices = $derived(hw ? [...hw.gpus.filter((g) => g.counted), ...(hw.cpu?.counted ? [hw.cpu] : [])] : []);
  const top = $derived(Math.max(1, ...devices.map((d) => d.tflopsFp32 ?? 0)));
  const gib = (v: number) => (v >= 100 ? Math.round(v).toString() : v.toFixed(1));
</script>

<section class="plate" aria-label="This machine's compute and memory">
  <div class="hero">
    <span class="lbl">{name}</span>
    {#if hw && hw.tflopsFp32 > 0}
      <div class="num">
        <b>{hw.tflopsFp32.toFixed(1)}</b>
        <span class="u">TFLOPS<br />FP32{hw.tflopsUnknown ? ` +${hw.tflopsUnknown}?` : ''}</span>
      </div>
    {:else}
      <div class="num none"><b>—</b><span class="u">TFLOPS<br />FP32</span></div>
    {/if}
  </div>
  <ul class="dev">
    {#each devices as d (d.id)}
      <li title={d.detail ?? ''}>
        <span class="dn">{d.name}</span>
        <span class="dm">{d.kind === 'cpu' ? (d.detail ?? 'CPU') : `${d.vramGiB ? gib(d.vramGiB) + ' GB' : ''} ${d.memoryType ?? ''}`.trim()}</span>
        <em title={d.tflopsSource === 'computed' ? 'Estimated: cores x FLOP per cycle x base clock' : "The vendor's published peak"}>{d.tflopsSource === 'computed' ? '~' : ''}{d.tflopsFp32 !== undefined ? d.tflopsFp32 : '?'}</em>
        <i style="width:{((d.tflopsFp32 ?? 0) / top) * 100}%"></i>
      </li>
    {/each}
  </ul>
  {#if hw}
    <div class="mem">
      <div><span class="lbl">VRAM pool</span><b>{gib(hw.vramPoolGiB)}</b><small>GB{hw.unified ? ' unified' : ''}</small></div>
      <div><span class="lbl">RAM</span><b>{gib(hw.ramTotalGiB)}</b><small>GB</small></div>
    </div>
  {/if}
</section>

<style>
  .plate {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: end;
    gap: 14px 34px;
  }
  .lbl {
    font: 600 10px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .num {
    display: flex;
    align-items: flex-end;
    gap: 10px;
    margin-top: 4px;
  }
  .num b {
    font: 800 76px/0.86 var(--k-font-display, system-ui, sans-serif);
    letter-spacing: -0.02em;
    color: var(--k-ink, #eee);
    text-shadow: 0 0 36px color-mix(in srgb, var(--k-accent, #5ab6eb) 30%, transparent);
  }
  .num.none b {
    color: var(--k-muted, #8a8a8a);
    text-shadow: none;
  }
  .num .u {
    padding-bottom: 5px;
    font: 600 11px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.14em;
    color: var(--k-accent, #5ab6eb);
  }
  .dev {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 10px 22px;
  }
  .dev li {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    padding-bottom: 7px;
    min-width: 0;
  }
  .dn {
    font: 600 13px/1.3 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dm {
    grid-row: 2;
    font: 400 11.5px/1.3 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dev em {
    grid-row: 1 / 3;
    grid-column: 2;
    align-self: center;
    padding-left: 10px;
    font: 500 15px var(--k-font-data, monospace);
    font-style: normal;
    color: var(--k-ink, #eee);
  }
  .dev i {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    border-radius: 1px;
    background: linear-gradient(90deg, var(--k-accent, #5ab6eb), color-mix(in srgb, var(--k-accent, #5ab6eb) 25%, transparent));
  }
  .mem {
    display: flex;
    gap: 22px;
  }
  .mem div {
    display: grid;
    grid-template-columns: auto auto;
    align-items: baseline;
    column-gap: 4px;
  }
  .mem .lbl {
    grid-column: 1 / 3;
  }
  .mem b {
    font: 500 22px var(--k-font-data, monospace);
    color: var(--k-ink, #eee);
  }
  .mem small {
    font: 400 11px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  @container records (max-width: 860px) {
    .plate {
      grid-template-columns: auto minmax(0, 1fr);
    }
    .mem {
      grid-column: 1 / 3;
    }
    .num b {
      font-size: 60px;
    }
  }
</style>
