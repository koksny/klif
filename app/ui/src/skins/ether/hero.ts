// The hero (one figure with its label and sub-line) and the facts under it, shared by the full window and the
// mini panel. Pure functions of the view model; the KLIF hero rules:
//   idle      standby + the last session (or why the selected tier cannot launch)
//   loading   % + the active load step; the six steps as dots
//   prefill   % of the prompt (never "0.0 tok/s" during prefill), done / total tok, tok/s
//   decode    tok/s
//   image     step / steps, s/it, size
//   asleep    the last tok/s dimmed, "GPU asleep · D3", paged-out GiB
//   fault     ERR + the fault title (+ the log tail in the full window)
import type { LoadStep, ModelRef, Session, Slot, SlotKind, ViewModel } from '../../lib/model/types';
import { fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../lib/model/format';
import type { Sleep } from './state.svelte';
import { availabilityText, fmtAgo, fmtDur, fmtEta, lastSessionText, lastSpeed, shapeText } from './text';

export interface Hero {
  label: string;
  value: string;
  unit: string;
  sub: string;
  /** dim: nothing measured right now (standby, asleep, stopping, no request in flight); red: fault. */
  tone: '' | 'dim' | 'red';
  /** The sub-line in the warn colour (the selected tier cannot launch). */
  warn: boolean;
  /** The load steps while loading. */
  steps: LoadStep[] | null;
  /** 0..1 for the progress hairline (load, prefill, sampling, waking), or null. */
  frac: number | null;
  /** The fault's last log lines. */
  log: string[];
}

export interface Ctx {
  vm: ViewModel;
  s: Session | null;
  kind: SlotKind;
  sel: Slot | undefined;
  model: ModelRef | undefined;
  /** GPU dormant (live sessions only). */
  dz: Sleep | null;
  waking: boolean;
  /** The decode speed, held so it changes at most ~2x a second. */
  tps: number;
  compact: boolean;
}

export function heroOf(c: Ctx): Hero {
  const { vm, s, kind, sel, dz } = c;
  // the placeholder figure is an en dash: an em dash at hero size reads as a bar
  const base: Hero = { label: '', value: '–', unit: '', sub: '', tone: '', warn: false, steps: null, frac: null, log: [] };
  if (!s) {
    const unit = kind === 'image' ? 'steps' : 'tok/s';
    if (sel && sel.availability !== 'ready')
      return { ...base, label: 'STANDBY', unit, sub: `${sel.label.toLowerCase()} · ${(sel.reason ?? availabilityText(sel.availability)).toLowerCase()}`, tone: 'dim', warn: true };
    const ls = vm.lastSession;
    if (!ls) return { ...base, label: 'STANDBY', unit, sub: 'nothing running · pick a tier and launch', tone: 'dim' };
    // standby shows the previous session: its median speed as the (dimmed) figure, the rest as the sub-line
    const sp = lastSpeed(ls);
    return { ...base, label: 'STANDBY', value: sp?.value ?? base.value, unit: sp?.unit ?? unit, sub: lastSessionText(ls, vm.slots, c.compact), tone: 'dim' };
  }
  const phase = s.phase;
  if (phase === 'fault') {
    const f = s.fault;
    return { ...base, label: `FAULT${f ? ` · ${fmtAgo(f.sinceS)}` : ''}`, value: 'ERR', sub: f?.title ?? 'The server stopped.', tone: 'red', log: (f?.logTail ?? []).slice(-4) };
  }
  if (phase === 'stopping') return { ...base, label: 'STOPPING', sub: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB`, tone: 'dim' };
  if (phase === 'starting' || phase === 'loading') {
    const ld = s.loading;
    const st = ld?.steps.find((x) => x.state === 'active');
    return {
      ...base,
      label: phase === 'starting' ? 'STARTING' : 'LOADING',
      value: String(Math.floor((ld?.fraction ?? 0) * 100)),
      unit: '%',
      sub: st ? `${st.label.toLowerCase()}${st.detail ? ` · ${st.detail}` : ''}` : 'starting the server',
      steps: ld?.steps ?? null,
      frac: ld?.fraction ?? 0,
    };
  }
  const llm = s.llm;
  const img = s.image;
  if (dz) {
    if (c.waking)
      return { ...base, label: `GPU WAKING${dz.powerState ? ` · ${dz.powerState}` : ''}`, value: String(Math.round(dz.restoredFrac * 100)), unit: '%', sub: `${fmtGiB(dz.pagedOutGiB)} GiB still in system RAM`, frac: dz.restoredFrac };
    return {
      ...base,
      label: `GPU ASLEEP${dz.powerState ? ` · ${dz.powerState}` : ''}`,
      value: llm ? fmtTps(c.tps) : '–',
      unit: llm ? 'tok/s' : '',
      sub: c.compact ? `${fmtGiB(dz.pagedOutGiB)} GiB paged out` : `${fmtGiB(dz.pagedOutGiB)} GiB paged out to RAM · ${fmtDur(dz.sinceS)}`,
      tone: 'dim',
    };
  }
  if (llm && llm.activity === 'prefill' && llm.prefill) {
    const p = llm.prefill;
    const frac = p.tokens > 0 ? Math.min(1, p.doneTokens / p.tokens) : 0;
    const cached = p.cachedTokens ? ` · ${fmtInt(p.cachedTokens)} cached` : '';
    return {
      ...base,
      label: 'PREFILL',
      value: String(Math.floor(frac * 100)),
      unit: '%',
      sub: c.compact ? `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s` : `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s · ${fmtEta(p.etaS)} left${cached}`,
      frac,
    };
  }
  if (llm) {
    if (llm.activity === 'decode') return { ...base, label: 'DECODE', value: fmtTps(c.tps), unit: 'tok/s', sub: `${fmtInt(llm.generatedTokens)} tok generated` };
    if (!llm.requests.length && !llm.generatedTokens) return { ...base, label: 'READY', unit: 'tok/s', sub: 'waiting for the first request', tone: 'dim' };
    return { ...base, label: 'READY', value: fmtTps(c.tps), unit: 'tok/s', sub: `last request · ${fmtInt(llm.generatedTokens)} tok`, tone: 'dim' };
  }
  if (img) {
    if (img.activity === 'generating' && img.steps > 0)
      return {
        ...base,
        label: `DIFFUSION${img.edit ? ' · EDIT' : ''}`,
        value: `${img.step}/${img.steps}`,
        unit: 'steps',
        sub: c.compact ? `${img.sPerIt.toFixed(2)} s/it · ${img.width}x${img.height}` : `${img.sPerIt.toFixed(2)} s/it · ${img.width}x${img.height} · ${fmtSeconds(img.elapsedS)}`,
        frac: Math.min(1, img.step / img.steps),
      };
    return { ...base, label: 'READY', unit: 'steps', sub: 'waiting for the next job', tone: 'dim' };
  }
  return { ...base, label: 'LIVE', sub: 'waiting for data', tone: 'dim' };
}

/** The facts under the hero: LLM context / shape / speculative; image last image / images. */
export function factsOf(c: Ctx, n: 2 | 3): [string, string][] {
  const { s, kind, model } = c;
  const llm = s?.llm ?? null;
  const img = s?.image ?? null;
  if (kind === 'image') {
    const j = img?.recent[img.recent.length - 1];
    return [
      ['last image', j ? `${fmtSeconds(j.seconds)} · ${j.width}x${j.height}${j.edit ? ' · edit' : ''}` : img ? 'none yet' : '—'],
      ['images', img ? `${fmtInt(img.imagesThisSession)} this session` : '—'],
    ];
  }
  const total = llm?.context.totalTokens || model?.ctxTokens || 0;
  const ctx = llm
    ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(total)} (${Math.round((llm.context.usedTokens / Math.max(1, total)) * 100)}%)`
    : total
      ? `— / ${fmtInt(total)}`
      : '—';
  const rows: [string, string][] = [
    ['context', ctx],
    ['shape', shapeText(model?.arch ?? c.sel?.model.arch)],
  ];
  if (n === 3) rows.push(['speculative', llm?.spec ? `${Math.round(llm.spec.acceptancePct)}% accepted` : (model?.specMode ?? 'off')]);
  return rows;
}
