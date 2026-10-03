// Spirit: everything the camera OSD says, derived once from the view model and shared by the full window and
// the mini panel (they differ in layout only). Never an invented value: every string is a ViewModel field.
import type { SystemId, ViewModel } from '../../lib/model/types';
import { canStop, EXTERNAL_NOTE, EXTERNAL_TITLE, idleState, isPendingLaunch, KIND_LABEL, launchCtl, selectedSystem } from '../../lib/model/systems';
import { fmtCtx, fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../lib/model/format';
import { held, useSleep } from './state.svelte';
import { fitOf, fmtAgo, fmtDur, fmtEta, gib1, lastSessionText, phaseWord, shapeText, short, sizeText, stepLine } from './text';

export type Tone = '' | 'dim' | 'warn' | 'red';

export interface Hero {
  label: string;
  labelTone: Tone;
  value: string;
  unit: string;
  sub: string;
  subTone: Tone;
  tone: Tone;
}

/** OSD indicator marks (Ic.svelte). */
export type IcKind = '' | 'dot' | 'ring' | 'square' | 'pause' | 'play' | 'restart';

export interface Rec {
  kind: 'stby' | 'load' | 'rec' | 'pause' | 'wake' | 'stop' | 'err';
  glyph: IcKind;
  text: string;
}

/** One camera setting: "TOK/S 39.4", "3.88 S/IT", "S2 · QWEN 3.8 27B" (model = only where there is room). */
export interface Setting {
  k?: string;
  v: string;
  u?: string;
  model?: boolean;
}

export interface Meter {
  frac: number;
  label: string;
  tone: Tone;
}

export type Seg = '' | 'fill' | 'paged' | 'ghost';
export interface Battery {
  segs: Seg[];
  tone: Tone;
  text: string;
  over: boolean;
  spill: string;
  title: string;
}

export interface Act {
  kind: 'go' | 'stop' | 'hot' | 'ext';
  glyph: IcKind;
  red: boolean;
  text: string;
  title: string;
  disabled: boolean;
  run: () => void;
}

export function useOsd(get: () => ViewModel, launch: (system: SystemId, stopOthers: boolean) => void, stop: (system: SystemId) => void, restart: (system: SystemId) => void) {
  const vm = $derived(get());
  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  // vm.session is always the selected System's session.
  const selSlot = $derived(selectedSystem(vm) ?? undefined);
  const runSlot = $derived(s ? selSlot : undefined);
  const slot = $derived(selSlot);
  const kind = $derived(selSlot?.kind ?? 'llm');
  const model = $derived(s?.model ?? selSlot?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);
  const loading = $derived(phase === 'starting' || phase === 'loading');
  const busy = $derived(loading || phase === 'stopping');
  const faulted = $derived(phase === 'fault');
  const online = $derived(phase === 'live');
  const why = $derived(idleState(selSlot));
  /** The selected System has no reason against launching (a broken preset, an unreachable node...). */
  const selReady = $derived(!why.warn && selSlot?.status !== 'not-set');
  /** The launch control: with conflicts it reads "STOP S1 & LAUNCH" and sends stopOthers. */
  const ctl = $derived(launchCtl(vm, selSlot, { short: true }));
  const mine = $derived(!!selSlot && selSlot.controllable && !selSlot.external);

  const sleep = useSleep(get);
  const dz = $derived(online ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);

  const tps = held(() => llm?.decodeTps ?? 0);
  const prefilling = $derived(online && !!llm?.prefill && llm.activity === 'prefill');
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? Math.min(1, llm.context.usedTokens / llm.context.totalTokens) : 0);
  const generating = $derived(!!img && img.activity === 'generating' && img.steps > 0);
  const working = $derived((gen?.requestsInFlight ?? 0) > 0);
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const exitText = $derived.by(() => {
    if (!fault) return '';
    if (fault.exitCode === undefined) return 'NO EXIT CODE';
    return fault.exitCodeHex ? `EXIT ${fault.exitCodeHex} (${fault.exitCode})` : `EXIT ${fault.exitCode}`;
  });

  // ---- recording state (top left of the frame) -------------------------------------------------------------
  const rec = $derived.by<Rec>(() => {
    if (!s) return { kind: 'stby', glyph: '', text: 'STBY' };
    if (faulted) return { kind: 'err', glyph: 'square', text: 'ERR' };
    if (loading) return { kind: 'load', glyph: 'ring', text: 'LOAD' };
    if (phase === 'stopping') return { kind: 'stop', glyph: 'square', text: 'STOP' };
    if (dz) return waking ? { kind: 'wake', glyph: 'ring', text: 'WAKE' } : { kind: 'pause', glyph: 'pause', text: 'PAUSE' };
    return { kind: 'rec', glyph: 'dot', text: 'REC' };
  });

  // ---- hero readout (top right, under the timecode) ----------------------------------------------------------
  const hero = $derived.by<Hero>(() => {
    // no reading: the camera's dashes, in the shape of the figure that would stand there
    const base: Hero = { label: '', labelTone: '', value: kind === 'image' ? '--/--' : kind === 'llm' ? '--.-' : '--', unit: '', sub: '', subTone: '', tone: '' };
    if (!s) {
      const unit = kind === 'image' ? 'STEPS' : kind === 'llm' ? 'TOK/S' : 'REQUESTS';
      if (!selReady)
        return { ...base, label: 'STANDBY', unit, sub: `${short(selSlot?.label ?? '')} · ${(selSlot?.reason ?? why.text).toUpperCase()}`, subTone: why.warn ? 'warn' : '', tone: 'dim' };
      return { ...base, label: 'STANDBY', unit, sub: vm.lastSession ? lastSessionText(vm.lastSession, vm.systems) : 'NOTHING RUNNING · PICK A SYSTEM AND LAUNCH', tone: 'dim' };
    }
    if (faulted) return { ...base, label: `FAULT${fault ? ` · ${fmtAgo(fault.sinceS)}` : ''}`, labelTone: 'red', value: 'ERR', sub: (fault?.title ?? 'The server stopped').toUpperCase(), subTone: 'red', tone: 'red' };
    if (phase === 'stopping') return { ...base, label: 'STOPPING', value: '--', sub: `RELEASING ${fmtGiB(vm.vram.usedGiB)} GIB`, tone: 'dim' };
    if (loading) {
      // the active step itself is the AF line at the frame's centre; here: how far along, and for how long
      const steps = (s.loading?.steps ?? []).filter((x) => x.id !== 'ready');
      const at = steps.findIndex((x) => x.state === 'active');
      const n = at >= 0 ? at + 1 : steps.filter((x) => x.state === 'done').length;
      const elapsed = fmtSeconds(s.loading?.elapsedS ?? s.uptimeS).toUpperCase();
      return { ...base, label: phase === 'starting' ? 'START' : 'LOAD', value: String(Math.floor((s.loading?.fraction ?? 0) * 100)), unit: '%', sub: steps.length ? `STEP ${n} OF ${steps.length} · ${elapsed}` : elapsed };
    }
    if (dz && waking)
      return { ...base, label: `GPU WAKING${dz.powerState ? ` · ${dz.powerState}` : ''}`, labelTone: 'warn', value: String(Math.round(dz.restoredFrac * 100)), unit: '%', sub: `${fmtGiB(dz.pagedOutGiB)} GIB STILL IN SYSTEM RAM`, subTone: 'warn' };
    if (dz)
      return {
        ...base,
        label: `GPU ASLEEP${dz.powerState ? ` · ${dz.powerState}` : ''}`,
        labelTone: 'dim',
        value: llm ? fmtTps(tps.current) : '--',
        unit: llm ? 'TOK/S' : '',
        sub: `${fmtGiB(dz.pagedOutGiB)} GIB PAGED OUT · ${fmtDur(dz.sinceS)}`,
        tone: 'dim',
      };
    if (prefilling && llm?.prefill) {
      const p = llm.prefill;
      const cached = p.cachedTokens ?? 0;
      return {
        ...base,
        label: 'PREFILL',
        value: String(p.tokens > 0 ? Math.floor((p.doneTokens / p.tokens) * 100) : 0),
        unit: '%',
        sub: `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} TOK · ${fmtInt(p.tps)} TOK/S · ${fmtEta(p.etaS)} LEFT${cached > 0 ? ` · ${fmtInt(cached)} CACHED` : ''}`,
      };
    }
    if (llm) {
      const on = llm.activity === 'decode';
      return { ...base, label: on ? 'DECODE' : 'DECODE · IDLE', value: fmtTps(tps.current), unit: 'TOK/S', sub: `${fmtInt(llm.generatedTokens)} TOK GENERATED`, tone: on ? '' : 'dim' };
    }
    if (gen)
      return {
        ...base,
        label: `${KIND_LABEL[kind].toUpperCase()}${working ? ' · WORKING' : ''}`,
        value: gen.requestsTotal !== undefined ? fmtInt(gen.requestsTotal) : '--',
        unit: 'REQUESTS',
        sub: working ? `${gen.requestsInFlight} IN FLIGHT` : gen.lastActivityS !== undefined ? `LAST ACTIVITY ${fmtDur(gen.lastActivityS)} AGO` : 'WAITING FOR THE NEXT REQUEST',
        tone: working ? '' : 'dim',
      };
    if (img) {
      if (generating)
        return {
          ...base,
          label: `DIFFUSION${img.edit ? ' · EDIT' : ''}`,
          value: `${img.step}/${img.steps}`,
          unit: 'STEPS',
          sub: `${img.sPerIt.toFixed(2)} S/IT · ${img.width}×${img.height} · ${fmtSeconds(img.elapsedS).toUpperCase()}`,
        };
      return { ...base, label: 'DIFFUSION', unit: 'STEPS', sub: 'WAITING FOR THE NEXT JOB', tone: 'dim' };
    }
    return { ...base, label: 'LIVE', value: '--', sub: 'WAITING FOR DATA', tone: 'dim' };
  });

  // ---- the settings line (bottom of the frame): ISO / shutter / aperture, KLIF style ---------------------------
  const settings = $derived.by<Setting[]>(() => {
    const who: Setting = { v: `${short(slot?.label ?? '')} · ${(model?.name ?? '').toUpperCase()}`, model: true };
    if (kind === 'image') {
      if (generating && img) return [{ k: 'STEP', v: `${img.step}/${img.steps}` }, { v: img.sPerIt.toFixed(2), u: 'S/IT' }, { v: `${img.width}×${img.height}` }, who];
      return [{ k: 'STEP', v: '--/--' }, { v: '--.--', u: 'S/IT' }, { v: sizeText(model?.imageSize) ?? '--' }, who];
    }
    if (kind !== 'llm') {
      return [{ k: 'REQ', v: gen?.requestsTotal !== undefined ? fmtInt(gen.requestsTotal) : '--' }, { k: 'FLIGHT', v: gen ? fmtInt(gen.requestsInFlight ?? 0) : '--' }, { v: (model?.backend || model?.engine || '--').toUpperCase() }, who];
    }
    // prefill: its own speed once it has one, else how far the prompt is (never a 0 tok/s); asleep: the last speed
    const pf = llm?.prefill;
    const first: Setting =
      prefilling && pf && !dz
        ? pf.tps > 0
          ? { k: 'PREFILL', v: fmtInt(pf.tps), u: 'TOK/S' }
          : { k: 'PREFILL', v: `${pf.tokens > 0 ? Math.floor((pf.doneTokens / pf.tokens) * 100) : 0}%` }
        : { k: 'TOK/S', v: llm && online ? fmtTps(tps.current) : '--.-' };
    const ctx: Setting = llm && llm.context.totalTokens > 0 ? { k: 'CTX', v: `${Math.round(ctxFrac * 100)}%` } : { k: 'CTX', v: model?.ctxTokens ? fmtCtx(model.ctxTokens).toUpperCase() : '--' };
    const spec: Setting = llm?.spec ? { k: 'SPEC', v: `${Math.round(llm.spec.acceptancePct)}%` } : { k: 'SPEC', v: llm ? 'OFF' : (model?.specMode?.toUpperCase() ?? 'OFF') };
    return [first, ctx, spec, who];
  });

  // ---- exposure meter: context fill (LLM), step progress (CGI), load progress while loading -------------------
  const meter = $derived.by<Meter>(() => {
    if (loading) return { frac: s?.loading?.fraction ?? 0, label: 'LOAD', tone: '' };
    if (kind === 'image') return { frac: generating && img ? img.step / img.steps : 0, label: 'STEP', tone: faulted ? 'red' : generating ? '' : 'dim' };
    if (kind !== 'llm') return { frac: working ? 1 : 0, label: 'REQ', tone: faulted ? 'red' : working ? '' : 'dim' };
    if (!llm) return { frac: 0, label: 'CTX', tone: faulted ? 'red' : 'dim' };
    return { frac: ctxFrac, label: 'CTX', tone: faulted ? 'red' : dz ? 'dim' : ctxFrac >= 0.95 ? 'red' : ctxFrac >= 0.9 ? 'warn' : '' };
  });

  // ---- VRAM as the camera battery: 10 segments, filled = used share -------------------------------------------
  const battery = $derived.by<Battery>(() => {
    const v = vm.vram;
    const total = Math.max(0.01, v.totalGiB);
    const used = Math.min(total, Math.max(0, v.usedGiB));
    const lit = Math.round((used / total) * 10);
    const fit = !s ? fitOf(v, selSlot) : null;
    const paged = dz ? Math.min(10, Math.round(((used + dz.pagedOutGiB) / total) * 10)) : lit;
    const g0 = fit ? Math.round((Math.min(total, fit.base) / total) * 10) : 10;
    const g1 = fit ? Math.min(10, Math.ceil((Math.min(total, fit.top) / total) * 10 - 0.05)) : 0;
    const segs: Seg[] = Array.from({ length: 10 }, (_, i) => (i < lit ? 'fill' : i < paged ? 'paged' : i >= g0 && i < g1 ? 'ghost' : ''));
    const spillGiB = v.spillMiB / 1024;
    // Same rule as every skin: a full inference card is normal; warn only below warn_below_gib free.
    const tone: Tone = spillGiB > 0 ? 'red' : dz ? 'dim' : s && total - used < v.warnBelowGiB ? 'warn' : '';
    const text = fit ? `FIT ${gib1(fit.top)}/${gib1(total)}` : `${gib1(used)}/${gib1(total)}`;
    const title = fit
      ? `${selSlot?.label ?? ''} would take the VRAM to ${fmtGiB(fit.top)} of ${fmtGiB(total)} GiB (${fit.spare >= 0 ? `${fmtGiB(fit.spare)} GiB spare` : `over by ${fmtGiB(-fit.spare)} GiB`})`
      : `VRAM ${fmtGiB(v.usedGiB)} of ${fmtGiB(total)} GiB${dz ? ` · ${fmtGiB(dz.pagedOutGiB)} GiB paged out` : ''}${spillGiB > 0 ? ` · ${fmtGiB(spillGiB)} GiB spilled to system memory` : ''}`;
    return { segs, tone, text, over: !!fit && fit.spare < 0, spill: spillGiB > 0 ? `SPILL ${gib1(spillGiB)}` : '', title };
  });

  // ---- the primary act: Launch / Cancel / Stop / Restart (one fixed place) -------------------------------------
  const act = $derived.by<Act>(() => {
    // An external server: a quiet note, never Launch / Stop (KLIF only watches it).
    if (selSlot?.external) return { kind: 'ext', glyph: '', red: false, text: EXTERNAL_NOTE.toUpperCase(), title: EXTERNAL_TITLE, disabled: true, run: () => {} };
    // A launch that waits for other Systems to stop (starting, no session yet): Cancel.
    if (!s && isPendingLaunch(selSlot)) {
      const id = selSlot?.id ?? '';
      return { kind: 'stop', glyph: 'ring', red: false, text: `CANCEL ${short(selSlot?.label ?? '')}`, title: selSlot?.reason ?? 'Cancel the launch', disabled: !canStop(selSlot), run: () => stop(id) };
    }
    if (!s) {
      return {
        kind: 'go',
        glyph: 'play',
        red: false,
        text: ctl.stopOthers ? ctl.text.toUpperCase() : `LAUNCH ${short(selSlot?.label ?? '')}`,
        title: ctl.enabled ? (ctl.stopOthers ? `${ctl.text}: ${selSlot?.reason ?? ''}` : `Launch ${selSlot?.label ?? ''}`) : `${selSlot?.label ?? ''}: ${ctl.blocked}`,
        disabled: !ctl.enabled,
        run: () => launch(vm.selected ?? selSlot?.id ?? '', ctl.stopOthers),
      };
    }
    const tier = short(selSlot?.label ?? '');
    const id = selSlot?.id ?? '';
    if (faulted) return { kind: 'hot', glyph: 'restart', red: false, text: `RESTART ${tier}`, title: `Launch ${selSlot?.label ?? ''} again`, disabled: !mine, run: () => restart(id) };
    if (phase === 'stopping') return { kind: 'stop', glyph: 'square', red: false, text: 'STOPPING', title: 'Stopping', disabled: true, run: () => {} };
    const external = selSlot?.external ? 'External server: it runs where it was started.' : '';
    if (loading) return { kind: 'stop', glyph: 'ring', red: false, text: `CANCEL ${tier}`, title: 'Cancel the launch', disabled: !canStop(selSlot), run: () => stop(id) };
    return { kind: 'stop', glyph: 'dot', red: true, text: `STOP ${tier}`, title: external || 'Stop the server', disabled: !canStop(selSlot), run: () => stop(id) };
  });

  const status = $derived.by<{ text: string; tone: Tone | 'live' }>(() => {
    if (dz) return { text: waking ? 'GPU WAKING' : 'GPU ASLEEP', tone: waking ? 'warn' : 'dim' };
    if (faulted) return { text: 'FAULT', tone: 'red' };
    if (!s) return { text: 'STANDBY', tone: 'dim' };
    return { text: phaseWord(phase), tone: online ? 'live' : '' };
  });

  const shape = $derived(shapeText(s?.model.arch ?? slot?.model.arch ?? null, kind));
  const activeStep = $derived(loading ? (s?.loading?.steps.find((x) => x.state === 'active') ?? null) : null);
  /** The AF line under the focus box: "WEIGHTS" + "5.4 / 11.6 GIB". */
  const afLine = $derived.by<[string, string]>(() => {
    const line = stepLine(activeStep);
    if (!activeStep || !line) return ['STARTING', ''];
    const word = activeStep.label.replace(/^load\s+/i, '').toUpperCase();
    return [word, line.slice(word.length).trim()];
  });

  return {
    get vm() {
      return vm;
    },
    get s() {
      return s;
    },
    get phase() {
      return phase;
    },
    get runSlot() {
      return runSlot;
    },
    get selSlot() {
      return selSlot;
    },
    get slot() {
      return slot;
    },
    get kind() {
      return kind;
    },
    get model() {
      return model;
    },
    get llm() {
      return llm;
    },
    get img() {
      return img;
    },
    get loading() {
      return loading;
    },
    get busy() {
      return busy;
    },
    get faulted() {
      return faulted;
    },
    get online() {
      return online;
    },
    get selReady() {
      return selReady;
    },
    get ctl() {
      return ctl;
    },
    get mine() {
      return mine;
    },
    get gen() {
      return gen;
    },
    get dz() {
      return dz;
    },
    get prefilling() {
      return prefilling;
    },
    get fault() {
      return fault;
    },
    get exitText() {
      return exitText;
    },
    get rec() {
      return rec;
    },
    get hero() {
      return hero;
    },
    get settings() {
      return settings;
    },
    get meter() {
      return meter;
    },
    get battery() {
      return battery;
    },
    get act() {
      return act;
    },
    get status() {
      return status;
    },
    get shape() {
      return shape;
    },
    get afLine() {
      return afLine;
    },
    /** Session clock for the timecode (null = idle), and whether frames advance (recording). */
    get uptime() {
      return s ? (loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS) : null;
    },
  };
}
