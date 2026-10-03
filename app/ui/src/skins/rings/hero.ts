// The hero card's content, shared by the full window and the mini panel. KLIF rules: idle = standby + the last
// session; loading = % + the active step; prefill = % of the prompt (never "0.0 tok/s") + done / total + tok/s;
// decode = tok/s; CGI = step / steps + s/it + size; GPU asleep = dimmed + paged-out GiB; fault = ERR + its title.
import type { System, SystemKind, ViewModel } from '../../lib/model/types';
import { fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../lib/model/format';
import { idleState, KIND_LABEL, shortLabel as tierShort } from '../../lib/model/systems';
import type { Sleep } from './sleep.svelte';
import { fmtAgo, fmtDur, fmtEta, lastSessionShort, lastSessionText } from './text';

export type Tone = '' | 'dim' | 'warn' | 'danger';

export interface Hero {
  label: string;
  value: string;
  unit: string;
  sub: string;
  tone: Tone;
  subTone: '' | 'warn';
  /** Fault only (full window): the exit code and the last log lines. */
  exit?: string;
  log?: string[];
}

export function heroOf(vm: ViewModel, o: { kind: SystemKind; sel: System | undefined; dz: Sleep | null; waking: boolean; full: boolean }): Hero {
  const s = vm.session;
  const base = { tone: '' as Tone, subTone: '' as const };
  const llm = s?.llm ?? null;
  const img = s?.image ?? null;
  const gen = s?.generic ?? null;
  if (!s) {
    const unit = o.kind === 'image' ? 'steps' : o.kind === 'llm' ? 'tok/s' : 'requests';
    const why = idleState(o.sel);
    if (o.sel && (why.warn || o.sel.status === 'not-set'))
      return { ...base, label: 'Standby', value: '—', unit, sub: `${tierShort(o.sel.label)} · ${(o.sel.reason ?? why.text).toLowerCase()}`, tone: 'dim', subTone: why.warn ? 'warn' : '' };
    const ls = vm.lastSession;
    const sub = ls ? (o.full ? lastSessionText(ls, vm.systems) : lastSessionShort(ls, vm.systems)) : 'nothing running';
    return { ...base, label: 'Standby', value: '—', unit, sub, tone: 'dim' };
  }
  const phase = s.phase;
  if (phase === 'fault') {
    const f = s.fault;
    const exit = f?.exitCode === undefined ? 'exit code not reported' : `exit ${f.exitCodeHex ? `${f.exitCodeHex} (${f.exitCode})` : f.exitCode}`;
    return {
      ...base,
      label: `Fault${f ? ` · ${fmtAgo(f.sinceS)}` : ''}`,
      value: 'ERR',
      unit: '',
      sub: f?.title ?? 'The server stopped',
      tone: 'danger',
      exit: o.full ? exit : undefined,
      log: o.full ? (f?.logTail ?? []).slice(-4) : undefined,
    };
  }
  if (phase === 'stopping') return { ...base, label: 'Stopping', value: '—', unit: '', sub: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB`, tone: 'dim' };
  if (phase === 'starting' || phase === 'loading') {
    const st = s.loading?.steps.find((x) => x.state === 'active');
    return {
      ...base,
      label: 'Loading',
      value: String(Math.floor((s.loading?.fraction ?? 0) * 100)),
      unit: '%',
      sub: st ? `${st.label}${st.detail ? ` · ${st.detail}` : ''}` : phase === 'starting' ? 'starting the server' : 'loading',
    };
  }
  if (o.dz) {
    if (o.waking)
      return {
        ...base,
        label: `GPU waking${o.dz.powerState ? ` · ${o.dz.powerState}` : ''}`,
        value: String(Math.round(o.dz.restoredFrac * 100)),
        unit: '%',
        sub: `${fmtGiB(o.dz.pagedOutGiB)} GiB still in system RAM`,
        tone: 'warn',
      };
    return {
      ...base,
      label: `GPU asleep${o.dz.powerState ? ` · ${o.dz.powerState}` : ''}`,
      value: llm ? fmtTps(llm.decodeTps) : '—',
      unit: llm ? 'tok/s' : '',
      sub: `${fmtGiB(o.dz.pagedOutGiB)} GiB paged out${o.full ? ` · ${fmtDur(o.dz.sinceS)}` : ''}`,
      tone: 'dim',
    };
  }
  if (llm?.activity === 'prefill' && llm.prefill) {
    const p = llm.prefill;
    const cached = p.cachedTokens ?? 0;
    return {
      ...base,
      label: 'Prefill',
      value: String(p.tokens > 0 ? Math.floor((p.doneTokens / p.tokens) * 100) : 0),
      unit: '%',
      sub: o.full
        ? `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s · ${fmtEta(p.etaS)} left${cached > 0 ? ` · ${fmtInt(cached)} cached` : ''}`
        : `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s`,
    };
  }
  if (llm) {
    const on = llm.activity === 'decode';
    return { ...base, label: on ? 'Decode' : 'Decode · idle', value: fmtTps(llm.decodeTps), unit: 'tok/s', sub: `${fmtInt(llm.generatedTokens)} tok generated${on ? '' : ' · waiting for a request'}`, tone: on ? '' : 'dim' };
  }
  if (gen) {
    const on = (gen.requestsInFlight ?? 0) > 0;
    const name = KIND_LABEL[o.kind];
    return {
      ...base,
      label: on ? `${name} · working` : `${name} · idle`,
      value: gen.requestsTotal !== undefined ? fmtInt(gen.requestsTotal) : '—',
      unit: 'requests',
      sub: on ? `${gen.requestsInFlight} in flight` : gen.lastActivityS !== undefined ? `last activity ${fmtDur(gen.lastActivityS)} ago` : 'waiting for the next request',
      tone: on ? '' : 'dim',
    };
  }
  if (img) {
    if (img.activity === 'generating' && img.steps > 0)
      return {
        ...base,
        label: `Diffusion${img.edit ? ' · edit' : ''}`,
        value: `${img.step}/${img.steps}`,
        unit: 'steps',
        sub: `${img.sPerIt.toFixed(2)} s/it · ${img.width}×${img.height}${o.full ? ` · ${fmtSeconds(img.elapsedS)}` : ''}`,
      };
    return { ...base, label: 'Diffusion · idle', value: '—', unit: 'steps', sub: 'waiting for the next job', tone: 'dim' };
  }
  return { ...base, label: 'Live', value: '—', unit: '', sub: 'waiting for data', tone: 'dim' };
}
