<script lang="ts">
  // The VRAM dial. Outer ring: the GiB scale (0 .. totalGiB, linear, 180 degrees from 20 deg below
  // 9 o'clock to 20 deg above 3 o'clock) and a short pointer whose angle is usedGiB / totalGiB. The
  // pointer lives in the outer ring only (as in the approved mockup), so it never crosses the printed
  // title, readout or cliff labels in the middle of the face.
  // Middle: the printed cliff. Its plateau is the usedGiB history (left = oldest sample, the cliff
  // face = now); its strata are the current layers stacked bottom-first, strictly proportional to GiB
  // on the same vertical axis as the dashed capacity line. The free headroom is dimensioned at the face.
  // Red zone = the last warnBelowGiB of the scale; bright only while free < warnBelowGiB.
  //
  // Modes (same scale, same axis):
  //   live  - as above. Optional `target` (loading): the GiB the session will hold once loaded, dashed.
  //   fit   - idle launcher preview: the selected tier's expectedVram stacked on baselineGiB, drawn as a
  //           projected (dashed) block with no time axis; the pointer stands at the expected total.
  //   fault - the session died: the history above what is still resident is drawn as a dashed ghost,
  //           the collapse is the fracture in the history, and the rubble at its foot scales with the
  //           GiB that fell (labelled).
  //   sleep - (live mode, vram.dormant set) the GPU powered down with the model loaded: the allocations
  //           still exist but are paged out to system RAM. The pointer drops to what is resident (usedGiB)
  //           while a dashed ghost pointer stays at the allocated total; the strata (the allocations,
  //           stacked on the same GiB axis) are outlines inside a dashed envelope, and an amber lamp prints
  //           SLEEP (WAKING while the amount resident is climbing back) with the GiB paged out.
  import { onMount } from 'svelte';
  import type { GpuMemory, VramLayer } from '../../../lib/model/types';
  import { fmtGiB } from '../../../lib/model/format';
  import { VRAM_FULL_A0 as A0, VRAM_FULL_A1 as A1, clamp, fmtLayer, polar, rockTexture, sectorPath, sleepDetail, sleepOf, sleepWord } from '../theme';

  let {
    vram,
    mode = 'live',
    fit = null,
    target = null,
    title = 'VRAM',
  }: {
    vram: GpuMemory;
    mode?: 'live' | 'fit' | 'fault';
    fit?: { baseline: number; layers: VramLayer[] } | null;
    target?: number | null;
    title?: string;
  } = $props();

  const uid = $props.id();
  const C = 500;
  const BOX = { x0: 136, x1: 860, top: 374, bottom: 742 };
  const XC = 742; // where the plateau ends: "now", the cliff edge
  const BOXH = BOX.bottom - BOX.top;
  const R_HUB = 336;
  const R_TIP = 438; // stops short of the red band so the zone stays visible past the tip

  let rock = $state('');
  onMount(() => {
    rock = rockTexture();
  });

  const isFit = $derived(mode === 'fit' && !!fit);
  const isFault = $derived(mode === 'fault');
  const sleep = $derived(mode === 'live' ? sleepOf(vram) : null);
  const layers = $derived.by<VramLayer[]>(() => {
    if (!isFit || !fit) return vram.layers;
    const base = Math.max(0, fit.baseline);
    const out: VramLayer[] = base >= 0.005 ? [{ id: 'other', label: 'other', gib: base }] : [];
    return out.concat(fit.layers);
  });
  const total = $derived(Math.max(0.01, vram.totalGiB));
  const used = $derived(isFit ? layers.reduce((a, l) => a + Math.max(0, l.gib), 0) : Math.max(0, vram.usedGiB));
  const hist = $derived(isFit ? [used, used] : vram.history.length ? vram.history : [used]);
  // Free headroom is measured against the allocations: a paged-out model still owns its VRAM.
  const free = $derived(Math.max(0, total - (sleep ? Math.max(used, sleep.allocGiB) : used)));
  const spare = $derived(total - used);
  const warn = $derived(Math.max(0, Math.min(vram.warnBelowGiB, total)));
  const warnOn = $derived(free < warn);
  // The pointer is drawn at A0 (0 GiB) and turned clockwise by used/total of the sweep.
  const needleDeg = $derived((A0 - A1) * clamp(used / total));
  // Asleep: the ghost pointer stays where the allocations put it.
  const ghostDeg = $derived(sleep ? (A0 - A1) * clamp(sleep.allocGiB / total) : 0);

  const ang = (g: number) => A0 - (A0 - A1) * clamp(g / total);
  const yOf = (g: number) => BOX.bottom - clamp(g / total, 0, 1.02) * BOXH;
  const xOf = (i: number, n: number) => BOX.x0 + (n > 1 ? i / (n - 1) : 0) * (XC - BOX.x0);

  const scale = $derived.by(() => {
    const major = total <= 20 ? 4 : total <= 40 ? 8 : 16;
    const minor = major / 16;
    const n = Math.floor(total / minor + 1e-6);
    const ticks: { x1: number; y1: number; x2: number; y2: number; cls: string; red: boolean }[] = [];
    const nums: { x: number; y: number; t: string }[] = [];
    for (let i = 0; i <= n; i++) {
      const g = i * minor;
      if (total - g < minor * 0.35 && i > 0) continue; // too close to the capacity tick
      const kind = i % 16 === 0 ? 'major' : i % 4 === 0 ? 'mid' : 'minor';
      const a = ang(g);
      const [x1, y1] = polar(C, C, 444, a);
      const [x2, y2] = polar(C, C, kind === 'major' ? 400 : kind === 'mid' ? 414 : 428, a);
      ticks.push({ x1, y1, x2, y2, cls: kind, red: g > total - warn + 1e-6 });
      // Major ticks are numbered; 0 is the scale's start tick and stays unprinted to keep the
      // cliff's left edge clear.
      if (kind === 'major' && g > 0) {
        const [x, y] = polar(C, C, 374, a);
        nums.push({ x, y: y + 10, t: String(g) });
      }
    }
    const a = ang(total);
    const [x1, y1] = polar(C, C, 450, a);
    const [x2, y2] = polar(C, C, 394, a);
    ticks.push({ x1, y1, x2, y2, cls: 'cap', red: false });
    return { ticks, nums };
  });

  // The zone's angular extent is the configured warnBelowGiB; its radial depth is just print.
  const redPath = $derived(warn > 0 ? sectorPath(C, C, 442, 470, ang(total - warn), ang(total)) : '');

  const land = $derived.by(() => {
    const N = hist.length;
    const stride = Math.max(1, Math.floor(N / 160));
    let d = `M${BOX.x0},${BOX.bottom}`;
    for (let i = 0; i < N; i += stride) d += ` L${xOf(i, N).toFixed(1)},${yOf(hist[i]).toFixed(1)}`;
    const yT = yOf(used);
    const H = BOX.bottom - yT;
    d += ` L${XC},${yT.toFixed(1)}`;
    // The face: a steep drop with a talus at the foot (shape only, no axis meaning).
    const face =
      ` C${XC + 14},${(yT + H * 0.22).toFixed(1)} ${XC + 20},${(BOX.bottom - H * 0.3).toFixed(1)} ${XC + 36},${(BOX.bottom - H * 0.09).toFixed(1)}` +
      ` C${XC + 50},${BOX.bottom - 3} ${XC + 74},${BOX.bottom} ${XC + 98},${BOX.bottom}`;
    return {
      fill: `${d}${face} Z`,
      top: d.replace(`M${BOX.x0},${BOX.bottom}`, `M${BOX.x0},${yOf(hist[0]).toFixed(1)}`),
      face: `M${XC},${yT.toFixed(1)}${face}`,
      yT,
      spanLabel: spanLabel(N),
    };
  });

  // Time-axis start label for N one-second samples: whole minutes as "−5 min", a young session's
  // window as "−48 s" or "−2.8 min" (never rounded to a minute it does not span).
  function spanLabel(N: number): string {
    if (N < 120) return `−${N} s`;
    const r = N % 60;
    return r < 3 || r > 57 ? `−${Math.round(N / 60)} min` : `−${(N / 60).toFixed(1)} min`;
  }

  const TONES = ['#5e5a54', '#4d4a45', '#66615a', '#45423e', '#5a564f', '#4a4742'];
  const LABEL_MIN_H = 44;

  const bands = $derived.by(() => {
    const N = hist.length;
    let acc = 0;
    return layers.map((l, i) => {
      const lo = acc;
      acc += Math.max(0, l.gib);
      const yLo = yOf(lo);
      const yHi = yOf(acc);
      // In-band labels sit at the left as in the mockup, but only over solid rock: start after the
      // last history sample (oldest first) that dipped below this band's top.
      let k = N - 1;
      // Asleep the strata are the allocations, not the resident history: labels sit over the envelope.
      if (sleep) k = 0;
      else while (k > 0 && hist[k - 1] >= acc - 0.02) k--;
      const solidX = xOf(k, N);
      const text = `${l.label}  ${fmtLayer(l.gib)}`;
      const textW = text.length * 14.5;
      const lx = Math.max(BOX.x0 + (sleep ? 92 : 40), solidX + 28);
      const left = XC - 24 - lx >= textW;
      // Too little solid rock for the label either way (a dipping history): it becomes a callout.
      const inBand = yLo - yHi >= LABEL_MIN_H && (left || XC - 26 - textW >= solidX + 6);
      return { l, i, yLo, yHi, h: yLo - yHi, mid: (yLo + yHi) / 2, text, inBand, lx: left ? lx : XC - 26, anchor: left ? 'start' : 'end' };
    });
  });

  // Thin layers get callouts in the open air right of the cliff face, stacked top-down in band order,
  // each with an elbow leader back to its stratum at the face. They start below the pointer's reach.
  const callouts = $derived.by(() => {
    const out: { text: string; pts: string; dx: number; dy: number; ty: number }[] = [];
    let ty = BOX.top + 76;
    // Above the rubble when the cliff collapsed, so the pile never covers a label.
    const floorY = collapse ? Math.min(BOX.bottom - 18, collapse.top) : BOX.bottom - 18;
    const rows: { b: (typeof bands)[number]; ty: number }[] = [];
    for (const b of [...bands].reverse()) {
      if (b.inBand) continue;
      ty = Math.max(ty, b.mid + 12);
      rows.push({ b, ty });
      ty += 38;
    }
    // Several thin layers near the floor (early load): stack upwards from the floor instead of
    // piling onto the same line.
    for (let i = rows.length - 1; i >= 0; i--) {
      const lim = i === rows.length - 1 ? floorY : rows[i + 1].ty - 38;
      rows[i].ty = Math.min(rows[i].ty, lim);
    }
    for (const { b, ty: y } of rows) {
      const ex = XC + 34;
      out.push({ text: b.text, pts: `${XC + 6},${b.mid} ${ex},${b.mid} ${ex},${y - 10} ${ex + 10},${y - 10}`, dx: XC + 6, dy: b.mid, ty: y });
    }
    return out;
  });

  // Asleep: the envelope of the allocations (flat top at the allocated total, the same cliff face), drawn
  // dashed; the strata lines inside it are clipped to it.
  const envelope = $derived.by(() => {
    if (!sleep) return null;
    const yT = yOf(sleep.allocGiB);
    const H = BOX.bottom - yT;
    const face =
      ` C${XC + 14},${(yT + H * 0.22).toFixed(1)} ${XC + 20},${(BOX.bottom - H * 0.3).toFixed(1)} ${XC + 36},${(BOX.bottom - H * 0.09).toFixed(1)}` +
      ` C${XC + 50},${BOX.bottom - 3} ${XC + 74},${BOX.bottom} ${XC + 98},${BOX.bottom}`;
    const top = `M${BOX.x0},${yT.toFixed(1)} L${XC},${yT.toFixed(1)}`;
    return { fill: `M${BOX.x0},${BOX.bottom} L${BOX.x0},${yT.toFixed(1)} L${XC},${yT.toFixed(1)}${face} Z`, outline: `${top}${face}`, yT };
  });

  // Free headroom: a dimension at the cliff edge from the capacity line down to the plateau top.
  const dimY0 = BOX.top;
  const dimY1 = $derived(Math.max(BOX.top, land.yT));

  // Loading: where the session is headed (sum of the slot's expected layers), a dashed line.
  const targetY = $derived(target !== null && target > 0 ? yOf(target) : null);

  // Fault: the collapse. The sharpest fall in the history marks the fracture; the rubble scales with
  // the GiB that fell there (from the level just before the fall down to what is still resident).
  const collapse = $derived.by(() => {
    if (!isFault) return null;
    const N = hist.length;
    let best = 0;
    let at = -1;
    for (let i = 1; i < N; i++) {
      const dv = hist[i - 1] - hist[i];
      if (dv > best) {
        best = dv;
        at = i;
      }
    }
    if (at < 0 || best < 0.25) return null;
    const before = Math.max(...hist.slice(Math.max(0, at - 4), at));
    const lost = Math.max(0, before - used);
    const x = xOf(at, N);
    const yTop = yOf(before);
    const yFloor = yOf(used);
    // Deterministic rubble: rock count and pile size follow the lost GiB.
    let seed = 0x9e3779b1;
    const rnd = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 4294967296;
    };
    const share = clamp(lost / total, 0, 1);
    const count = Math.round(share * 26) + 7;
    const spread = 70 + share * 120;
    const pileTop = 28 + share * 118;
    const rocks: { d: string; tone: string; hot: boolean }[] = [];
    for (let k = 0; k < count; k++) {
      const u = rnd();
      const dx = Math.pow(u, 1.3) * spread;
      const pileH = (1 - dx / (spread + 8)) * pileTop;
      const cy = yFloor - 7 - rnd() * pileH;
      const cx = x + 6 + dx;
      const r = 12 + rnd() * 16;
      const sides = 5 + Math.floor(rnd() * 3);
      let d = '';
      for (let j = 0; j < sides; j++) {
        const a = (j / sides) * Math.PI * 2 + rnd() * 0.6;
        const rr = r * (0.65 + rnd() * 0.5);
        d += `${j ? 'L' : 'M'}${(cx + Math.cos(a) * rr).toFixed(1)},${(cy + Math.sin(a) * rr * 0.8).toFixed(1)}`;
      }
      rocks.push({ d: `${d}Z`, tone: TONES[k % TONES.length], hot: rnd() < 0.24 });
    }
    return { x, yTop, yFloor, lost, rocks, top: yFloor - pileTop - 16 };
  });

  // Pointer geometry, drawn pointing at A0; the layer above turns it.
  const ptr = (() => {
    const p = (r: number, off: number) => {
      const a = (A0 * Math.PI) / 180;
      const ux = Math.cos(a);
      const uy = -Math.sin(a);
      return `${(C + r * ux - off * uy).toFixed(1)},${(C + r * uy + off * ux).toFixed(1)}`;
    };
    const [hx, hy] = polar(C, C, R_HUB, A0);
    return {
      shaft: `${p(R_HUB, -8)} ${p(R_TIP - 8, -3.6)} ${p(R_TIP, 0)} ${p(R_TIP - 8, 3.6)} ${p(R_HUB, 8)}`,
      hx,
      hy,
    };
  })();

  const aria = $derived(
    sleep
      ? `${title}: GPU asleep, ${fmtGiB(used)} of ${fmtGiB(total)} GiB resident, ${fmtGiB(sleep.pagedGiB)} GiB paged out to system memory`
      : isFit
      ? `${title} fit preview: ${fmtGiB(used)} of ${fmtGiB(total)} GiB expected, ${spare >= 0 ? `${fmtGiB(spare)} GiB spare` : `${fmtGiB(-spare)} GiB over`}`
      : `${title}: ${fmtGiB(used)} of ${fmtGiB(total)} GiB used, ${fmtGiB(free)} GiB free`,
  );
</script>

<div class="dialbox">
<svg class="vram" class:fit={isFit} class:fault={isFault} class:sleep={!!sleep} viewBox="0 0 1000 1000" preserveAspectRatio="xMidYMid meet" role="img" aria-label={aria}>
  <defs>
    <linearGradient id="{uid}-bezel" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#3b3c40" />
      <stop offset="0.5" stop-color="#232427" />
      <stop offset="1" stop-color="#121314" />
    </linearGradient>
    <radialGradient id="{uid}-face" cx="50%" cy="46%" r="54%">
      <stop offset="0" stop-color="#1f2023" />
      <stop offset="0.85" stop-color="#18191b" />
      <stop offset="1" stop-color="#101112" />
    </radialGradient>
    <linearGradient id="{uid}-facet" gradientUnits="userSpaceOnUse" x1={XC - 36} y1="0" x2={XC + 104} y2="0">
      <stop offset="0" stop-color="rgba(255,255,255,0)" />
      <stop offset="0.3" stop-color="rgba(255,250,240,0.16)" />
      <stop offset="1" stop-color="rgba(255,250,240,0.02)" />
    </linearGradient>
    <linearGradient id="{uid}-wash" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="rgba(255,250,240,0.10)" />
      <stop offset="1" stop-color="rgba(0,0,0,0.12)" />
    </linearGradient>
    <pattern id="{uid}-hatch" width="14" height="14" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
      <line x1="0" y1="0" x2="0" y2="14" stroke="rgba(237,230,214,0.09)" stroke-width="3" />
    </pattern>
    <clipPath id="{uid}-land"><path d={land.fill} /></clipPath>
    {#if envelope}<clipPath id="{uid}-env"><path d={envelope.fill} /></clipPath>{/if}
    <radialGradient id="{uid}-amber" cx="50%" cy="42%" r="60%">
      <stop offset="0" stop-color="#fff0c8" />
      <stop offset="0.5" stop-color="#ffb02e" />
      <stop offset="1" stop-color="#d98a0a" />
    </radialGradient>
    {#if rock}
      <pattern id="{uid}-rock" width="256" height="256" patternUnits="userSpaceOnUse">
        <image href={rock} width="256" height="256" />
      </pattern>
    {/if}
  </defs>

  <!-- housing -->
  <circle cx={C} cy={C} r="497" fill="#0b0c0d" />
  <circle cx={C} cy={C} r="491" fill="url(#{uid}-bezel)" />
  <circle cx={C} cy={C} r="479" fill="#0a0b0c" />
  <circle cx={C} cy={C} r="474" fill="url(#{uid}-face)" />

  <!-- scale -->
  {#if redPath}<path d={redPath} class="red" class:hot={warnOn} />{/if}
  {#each scale.ticks as t, i (i)}
    <line x1={t.x1} y1={t.y1} x2={t.x2} y2={t.y2} class="tick {t.cls}" class:redtick={t.red} class:hot={warnOn} />
  {/each}
  {#each scale.nums as n (n.t)}
    <text x={n.x} y={n.y} class="num">{n.t}</text>
  {/each}

  <text x={C} y="238" class="title">{title}</text>
  {#if sleep}
    <!-- the SLEEP lamp: amber, breathing while the GPU is waking up -->
    <g class="lamp" class:wake={sleep.phase === 'waking'}>
      <title>{sleepDetail(sleep)}</title>
      <circle cx="602" cy="218" r="24" fill="#0b0c0d" />
      <circle cx="602" cy="218" r="19" class="lens" fill="url(#{uid}-amber)" />
      <circle cx="596" cy="211" r="5" fill="rgba(255,255,255,0.55)" />
    </g>
    <text x="636" y="230" class="sleepword">{sleepWord(sleep)}</text>
  {/if}
  {#if isFit}
    <text x={C} y="296" class="sub">{vram.device} · {fmtGiB(total)} GiB</text>
    <text x={C} y="346" class="fitlbl">FIT PREVIEW</text>
  {:else}
    <text x={C} y="296" class="sub">{vram.device} · {fmtGiB(used)} / {fmtGiB(total)} GiB</text>
  {/if}

  <!-- the cliff -->
  {#if envelope}
    <!-- asleep: the allocations are hatched where they are not resident; the solid body below is what was -->
    <path d={envelope.fill} fill="url(#{uid}-hatch)" />
  {/if}
  {#if isFault}
    <!-- what was resident before the collapse: a dashed ghost -->
    <path d={land.fill} fill="url(#{uid}-hatch)" />
    <path d={land.fill} fill="rgba(59,57,53,0.28)" />
  {:else}
    <path d={land.fill} fill="#3b3935" class="landfill" />
  {/if}
  {#if envelope}
    <!-- asleep: what was resident is a plain dim body (no rock); the strata (allocations) are outlines only -->
    <g clip-path="url(#{uid}-env)" class="strata">
      {#each bands as b (b.i)}
        <line x1={BOX.x0} x2={BOX.x1 + 120} y1={b.yHi} y2={b.yHi} class="stratum ghostline" />
      {/each}
    </g>
    <path d={envelope.outline} class="envline" />
  {:else}
    <g clip-path="url(#{uid}-land)" class="strata">
      {#each bands as b (b.i)}
        <rect x={BOX.x0} y={b.yHi} width={BOX.x1 - BOX.x0 + 120} height={Math.max(0, b.h)} fill={TONES[b.i % TONES.length]} />
      {/each}
      {#if rock}<rect x={BOX.x0} y={BOX.top - 20} width={BOX.x1 - BOX.x0 + 120} height={BOXH + 40} fill="url(#{uid}-rock)" opacity={isFault ? 0.25 : 0.55} />{/if}
      <rect x={BOX.x0} y={BOX.top - 20} width={BOX.x1 - BOX.x0 + 120} height={BOXH + 40} fill="url(#{uid}-wash)" />
      {#if !isFault}<rect x={XC - 36} y={BOX.top - 20} width="140" height={BOXH + 40} fill="url(#{uid}-facet)" />{/if}
      {#each bands as b (b.i)}
        <line x1={BOX.x0} x2={BOX.x1 + 120} y1={b.yHi} y2={b.yHi} class="stratum" />
      {/each}
    </g>
  {/if}
  <path d={land.top} class="ridge" class:dash={isFit || isFault} class:resident={!!sleep} />
  {#if !isFault}
    <path d={land.face} class="faceglow" class:hot={warnOn} />
    <path d={land.face} class="face" class:hot={warnOn} class:dash={isFit} />
  {/if}
  <line x1={BOX.x0 - 14} x2={BOX.x1} y1={BOX.bottom} y2={BOX.bottom} class="floor" />

  {#if targetY !== null && target !== null}
    <line x1={BOX.x0} x2={XC + 30} y1={targetY} y2={targetY} class="target" />
    <text x={BOX.x0 + 12} y={targetY + 34} class="targetlbl">loading to {fmtLayer(target)} GiB</text>
  {/if}


  <!-- capacity line and the dimensioned free headroom -->
  <line x1={BOX.x0} x2={XC + 30} y1={BOX.top} y2={BOX.top} class="cap" />
  {#if !isFit}
    <text x={XC - 4} y={BOX.top - 30} class="free" class:hot={warnOn} class:quiet={!!sleep} text-anchor="end">{fmtGiB(free)} GiB free</text>
    {#if sleep}
      <text x="236" y={BOX.top - 30} class="paged">paged out {fmtGiB(sleep.pagedGiB)} GiB</text>
    {/if}
  {/if}
  {#if !isFit}
    <polyline points="{XC - 128},{BOX.top - 16} {XC + 8},{BOX.top - 16} {XC + 8},{dimY1 - 3}" class="dim" />
    <line x1={XC - 128} x2={XC - 128} y1={BOX.top - 23} y2={BOX.top - 9} class="dim" />
    <line x1={XC - 60} x2={XC - 60} y1={BOX.top - 21} y2={BOX.top - 11} class="dim" />
    <line x1={XC - 2} x2={XC + 18} y1={dimY0} y2={dimY0} class="dim" />
    <path d="M{XC + 1},{dimY1 - 14} L{XC + 15},{dimY1 - 14} L{XC + 8},{dimY1 - 1} Z" class="arrow" class:hot={warnOn} />
  {/if}

  {#if collapse}
    <!-- the fracture where the history fell, and the rubble at its foot -->
    <line x1={collapse.x} x2={collapse.x} y1={collapse.yTop} y2={collapse.yFloor} class="fracglow" />
    <line x1={collapse.x} x2={collapse.x} y1={collapse.yTop} y2={collapse.yFloor} class="fracture" />
    {#each collapse.rocks as r, i (i)}
      <path d={r.d} fill={r.tone} class="rubble" class:hot={r.hot} />
    {/each}
    <text x={collapse.x - 12} y={Math.min(collapse.yFloor - 70, (collapse.yTop + collapse.yFloor) / 2)} class="fell" text-anchor="end">−{fmtGiB(collapse.lost)} GiB</text>
  {/if}
  {#each bands as b (b.i)}
    {#if b.inBand}
      <text x={b.lx} y={b.mid + 12} class="layer" text-anchor={b.anchor}>{b.text}</text>
    {/if}
  {/each}
  {#each callouts as c, i (i)}
    <polyline points={c.pts} class="leader" />
    <circle cx={c.dx} cy={c.dy} r="3.5" class="leaderdot" />
    <text x={XC + 50} y={c.ty} class="layer small">{c.text}</text>
  {/each}

  {#if !isFit}
    <text x={BOX.x0} y={BOX.bottom + 27} class="axis" text-anchor="start">{land.spanLabel}</text>
    <text x={XC} y={BOX.bottom + 27} class="axis" text-anchor="middle">now</text>
  {/if}

  {#if isFit}
    <text x={C} y="822" class="spill" class:hot={spare < 0}>
      {spare >= 0 ? `fits · ${fmtGiB(spare)} GiB spare` : `over the edge by ${fmtGiB(-spare)} GiB`}
    </text>
  {:else}
    {#if vram.spillMiB > 0}
      <rect x={XC + 104} width="46" y={BOX.bottom - Math.max(5, (vram.spillMiB / 1024 / total) * BOXH)}
        height={Math.max(5, (vram.spillMiB / 1024 / total) * BOXH)} class="spillpile" />
    {/if}
    <text x={C} y="822" class="spill" class:hot={vram.spillMiB > 0}>spill to shared memory: {Math.round(vram.spillMiB)} MiB</text>
    {#if sleep}<text x={C} y="884" class="sleepline">{sleepDetail(sleep)}</text>{/if}
  {/if}
</svg>
{#if sleep}
  <!-- Ghost pointer: dashed outline standing at the allocated total while the real one sits at what is resident. -->
  <div class="nlayer" style="transform: rotate({ghostDeg}deg)" aria-hidden="true">
    <svg class="vram" viewBox="0 0 1000 1000" preserveAspectRatio="xMidYMid meet">
      <polygon points={ptr.shaft} class="gshaft" />
      <circle cx={ptr.hx} cy={ptr.hy} r="15" class="ghub" />
    </svg>
  </div>
{/if}
<!-- Pointer on its own layer (a composited CSS rotation; the printed face is never repainted).
     It rests at 0 GiB and turns clockwise by used/total of the sweep around the dial centre. -->
<div class="nlayer" style="transform: rotate({needleDeg}deg)" aria-hidden="true">
  <svg class="vram" viewBox="0 0 1000 1000" preserveAspectRatio="xMidYMid meet">
    <polygon points={ptr.shaft} class="shaft-edge" />
    <polygon points={ptr.shaft} class="shaft" />
    <circle cx={ptr.hx} cy={ptr.hy} r="21" fill="#0b0c0d" />
    <circle cx={ptr.hx} cy={ptr.hy} r="15" class="hub" />
    <circle cx={ptr.hx - 4} cy={ptr.hy - 4} r="5" fill="rgba(255,255,255,0.45)" />
  </svg>
</div>
</div>

<style>
  .vram {
    display: block;
    width: 100%;
    height: 100%;
    font-family: var(--font-label);
  }
  .tick {
    stroke-linecap: butt;
  }
  .tick.minor {
    stroke: rgba(237, 230, 214, 0.42);
    stroke-width: 2.4;
  }
  .tick.mid {
    stroke: rgba(237, 230, 214, 0.72);
    stroke-width: 3.4;
  }
  .tick.major {
    stroke: #ede6d6;
    stroke-width: 5.5;
  }
  .tick.cap {
    stroke: #ede6d6;
    stroke-width: 6;
  }
  .tick.redtick {
    stroke: rgba(255, 107, 44, 0.45);
  }
  .tick.redtick.hot {
    stroke: #ff6b2c;
  }
  .red {
    fill: rgba(255, 107, 44, 0.3);
  }
  .red.hot {
    fill: #ff6b2c;
  }
  .num {
    fill: rgba(237, 230, 214, 0.7);
    font-size: 27px;
    font-weight: 500;
    text-anchor: middle;
  }
  .title {
    fill: #ede6d6;
    font-size: 54px;
    font-weight: 600;
    letter-spacing: 3px;
    text-anchor: middle;
  }
  .sub {
    fill: #ede6d6;
    font-size: 31px;
    font-weight: 400;
    letter-spacing: 1.5px;
    text-anchor: middle;
  }
  .fitlbl {
    fill: #ede6d6;
    font-size: 32px;
    font-weight: 600;
    letter-spacing: 4px;
    text-anchor: middle;
  }
  .stratum {
    stroke: rgba(237, 230, 214, 0.7);
    stroke-width: 2.6;
  }
  .ridge {
    fill: none;
    stroke: #ddd6c6;
    stroke-width: 3.2;
    stroke-linejoin: round;
  }
  .ridge.dash {
    stroke: rgba(221, 214, 198, 0.75);
    stroke-dasharray: 10 7;
  }
  /* projected block: the expected layers are not resident yet */
  .fit .strata {
    opacity: 0.62;
  }
  .fit .landfill {
    fill: rgba(59, 57, 53, 0.55);
  }
  .faceglow {
    fill: none;
    stroke: rgba(255, 107, 44, 0.18);
    stroke-width: 14;
    stroke-linecap: round;
  }
  .faceglow.hot {
    stroke: rgba(255, 107, 44, 0.4);
    stroke-width: 18;
  }
  .face {
    fill: none;
    stroke: rgba(255, 107, 44, 0.7);
    stroke-width: 5;
    stroke-linecap: round;
  }
  .face.dash {
    stroke-dasharray: 10 7;
  }
  .face.hot {
    stroke: #ff6b2c;
    stroke-width: 7;
  }
  .floor {
    stroke: rgba(237, 230, 214, 0.8);
    stroke-width: 2.6;
  }
  .target {
    stroke: rgba(90, 182, 235, 0.85);
    stroke-width: 2.6;
    stroke-dasharray: 12 9;
  }
  .targetlbl {
    fill: rgba(90, 182, 235, 0.95);
    font-size: 26px;
    font-weight: 500;
    letter-spacing: 1px;
  }
  .fracture {
    stroke: #ff6b2c;
    stroke-width: 4;
    stroke-linecap: round;
  }
  .fracglow {
    stroke: rgba(255, 107, 44, 0.3);
    stroke-width: 14;
    stroke-linecap: round;
  }
  .rubble {
    stroke: #0d0e0f;
    stroke-width: 1.6;
    stroke-linejoin: round;
  }
  .rubble.hot {
    stroke: #ff6b2c;
    stroke-width: 2.2;
  }
  .fell {
    fill: #ff6b2c;
    font-size: 28px;
    font-weight: 500;
    letter-spacing: 1px;
  }
  .cap {
    stroke: rgba(237, 230, 214, 0.45);
    stroke-width: 2;
    stroke-dasharray: 7 7;
  }
  .dim {
    fill: none;
    stroke: rgba(237, 230, 214, 0.78);
    stroke-width: 2.2;
  }
  .arrow {
    fill: #ede6d6;
  }
  .arrow.hot {
    fill: #ff6b2c;
  }
  .free {
    fill: #ede6d6;
    font-size: 30px;
    font-weight: 400;
    letter-spacing: 1px;
  }
  .free.hot {
    fill: #ff6b2c;
  }
  .layer {
    fill: #f1ebdd;
    font-size: 31px;
    font-weight: 400;
    letter-spacing: 1px;
    white-space: pre;
  }
  .layer.small {
    font-size: 27px;
    fill: rgba(237, 230, 214, 0.9);
  }
  .leader {
    fill: none;
    stroke: rgba(237, 230, 214, 0.65);
    stroke-width: 1.6;
  }
  .leaderdot {
    fill: #ede6d6;
  }
  .axis {
    fill: rgba(237, 230, 214, 0.58);
    font-size: 24px;
    font-weight: 500;
    letter-spacing: 1px;
  }
  .spill {
    fill: rgba(237, 230, 214, 0.92);
    font-size: 30px;
    font-weight: 400;
    letter-spacing: 1px;
    text-anchor: middle;
  }
  .spill.hot {
    fill: #ff6b2c;
  }
  .spillpile {
    fill: #ff6b2c;
  }
  /* asleep: the SLEEP lamp, the dashed allocation envelope and ghost pointer, outline-only strata */
  .lamp .lens {
    filter: drop-shadow(0 0 9px rgba(255, 176, 46, 0.75));
  }
  .lamp.wake .lens {
    animation: breathe 1.1s ease-in-out infinite;
  }
  @keyframes breathe {
    50% {
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .lamp.wake .lens {
      animation: none;
    }
  }
  .sleepword {
    fill: #ffb02e;
    font-size: 30px;
    font-weight: 600;
    letter-spacing: 3px;
  }
  .paged {
    fill: #ffb02e;
    font-size: 29px;
    font-weight: 500;
    letter-spacing: 1px;
  }
  .sleepline {
    fill: rgba(255, 176, 46, 0.85);
    font-size: 27px;
    font-weight: 500;
    letter-spacing: 1.5px;
    text-anchor: middle;
  }
  .free.quiet {
    fill: rgba(237, 230, 214, 0.5);
  }
  .sleep .landfill {
    fill: rgba(59, 57, 53, 0.62);
  }
  .sleep .dim {
    stroke: rgba(237, 230, 214, 0.42);
  }
  .sleep .arrow {
    fill: rgba(237, 230, 214, 0.5);
  }
  .envline {
    fill: none;
    stroke: rgba(237, 230, 214, 0.78);
    stroke-width: 3.4;
    stroke-dasharray: 10 7;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .stratum.ghostline {
    stroke: rgba(237, 230, 214, 0.5);
    stroke-width: 2.4;
    stroke-dasharray: 8 7;
  }
  .ridge.resident {
    stroke: rgba(221, 214, 198, 0.55);
  }
  .sleep .layer {
    fill: rgba(241, 235, 221, 0.78);
  }
  .sleep .layer.small {
    fill: rgba(237, 230, 214, 0.7);
  }
  .gshaft {
    fill: rgba(11, 12, 13, 0.55);
    stroke: rgba(237, 230, 214, 0.92);
    stroke-width: 3.4;
    stroke-dasharray: 7 6;
    stroke-linejoin: round;
  }
  .ghub {
    fill: none;
    stroke: rgba(237, 230, 214, 0.85);
    stroke-width: 3;
    stroke-dasharray: 5 5;
  }
  .dialbox {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .nlayer {
    position: absolute;
    inset: 0;
    transform-origin: 50% 50%;
    transition: transform 400ms cubic-bezier(0.3, 0.8, 0.3, 1);
    will-change: transform;
    pointer-events: none;
  }
  .shaft-edge {
    fill: #0b0c0d;
    stroke: #0b0c0d;
    stroke-width: 6;
    stroke-linejoin: round;
  }
  .shaft {
    fill: #5ab6eb;
  }
  .hub {
    fill: #5ab6eb;
  }
</style>
