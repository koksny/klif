// Regenerates src/lib/mock/fixtures/*.json from local inference session logs.
//
//   node src/lib/mock/tools/extract-fixtures.mjs --logs <runtime-logs dir> [--bench <benchmark-results.json>]
//
// READ-ONLY on the log directory. Writes ONLY numeric statistics: no prompt text, no file paths,
// no model file names, no addresses. The log directory is an argument on purpose (no machine paths
// are baked into this file).
import { readFileSync, readdirSync, statSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const outDir = resolve(here, '../fixtures');

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 && i + 1 < process.argv.length ? process.argv[i + 1] : fallback;
}
const logsDir = arg('logs');
if (!logsDir) {
  console.error('usage: node extract-fixtures.mjs --logs <runtime-logs dir> [--bench <benchmark-results.json>]');
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });

const r1 = (v) => Math.round(v * 10) / 10;
const r2 = (v) => Math.round(v * 100) / 100;
const r3 = (v) => Math.round(v * 1000) / 1000;

function quantiles(arr, qs = [0.05, 0.25, 0.5, 0.75, 0.95]) {
  if (!arr.length) return null;
  const s = [...arr].sort((a, b) => a - b);
  return qs.map((q) => s[Math.min(s.length - 1, Math.floor(q * s.length))]);
}
function mean(a) {
  return a.reduce((x, y) => x + y, 0) / Math.max(1, a.length);
}
function sd(a) {
  const m = mean(a);
  return Math.sqrt(mean(a.map((x) => (x - m) ** 2)));
}

// ---------------------------------------------------------------------------------------------
// llama-server log parser
// ---------------------------------------------------------------------------------------------
const LINE = /^(\d+)\.(\d{2})\.(\d{3})\.(\d{3}) ([A-Z]) (.*)$/;
const tsOf = (m) => +m[1] * 60 + +m[2] + +m[3] / 1000 + +m[4] / 1e6;

function parseLlama(text) {
  const out = { load: {}, requests: [] };
  let cur = null;
  let lastRelease = null;
  let sim = null;
  let pendingPromptTotal = null;
  for (const raw of text.split(/\r?\n/)) {
    const m = LINE.exec(raw);
    if (!m) continue;
    const t = tsOf(m);
    const msg = m[6];
    if (/load_model: loading model/.test(msg) && out.load.start === undefined) out.load.start = t;
    else if (/llama threadpool init/.test(msg) && out.load.threads === undefined) out.load.threads = t;
    else if (/load_model: loaded multimodal model/.test(msg)) out.load.vision = t;
    else if (/load_model: initializing, n_slots/.test(msg) && out.load.init === undefined) out.load.init = t;
    else if (/llama_server: model loaded/.test(msg) && out.load.loaded === undefined) out.load.loaded = t;
    else if (/selected slot by LCP similarity, f_sim_best = ([\d.]+)/.test(msg)) sim = +/f_sim_best = ([\d.]+)/.exec(msg)[1];
    else if (/selected slot by LRU/.test(msg)) sim = 0;
    else if (/new prompt, .*task\.n_tokens = (\d+)/.test(msg)) pendingPromptTotal = +/task\.n_tokens = (\d+)/.exec(msg)[1];
    else if (/launch_slot_: id\s+\d+ \| task (-?\d+) \| processing task/.test(msg)) {
      cur = { tStart: t, sim, progress: [], gen: [], promptTotal: pendingPromptTotal };
      sim = null;
      pendingPromptTotal = null;
    } else if (cur) {
      let g;
      if ((g = /prompt processing, n_tokens =\s+(\d+), progress = ([\d.]+), t =\s+([\d.]+) s \/ ([\d.]+) tokens per second/.exec(msg)))
        cur.progress.push({ n: +g[1], p: +g[2], t: +g[3] });
      else if ((g = /n_gen =\s+(\d+), tg =\s+([\d.]+) t\/s, tg_3s =\s+([\d.]+) t\/s/.exec(msg)))
        cur.gen.push({ n: +g[1], tg: +g[2], tg3: +g[3] });
      else if ((g = /\| prompt eval time =\s+([\d.]+) ms \/\s+(\d+) tokens .*?([\d.]+) tokens per second/.exec(msg)))
        cur.pe = { ms: +g[1], n: +g[2], tps: +g[3] };
      else if ((g = /\|\s+eval time =\s+([\d.]+) ms \/\s+(\d+) tokens .*?([\d.]+) tokens per second/.exec(msg)))
        cur.ev = { ms: +g[1], n: +g[2], tps: +g[3] };
      else if ((g = /draft acceptance = ([\d.]+) \(\s*(\d+) accepted \/\s*(\d+) generated\), mean len =\s+([\d.]+)/.exec(msg)))
        cur.draft = { acc: +g[1], accepted: +g[2], generated: +g[3], meanLen: +g[4] };
      else if ((g = /slot\s+release: id\s+\d+ \| task -?\d+ \| stop processing: n_tokens = (\d+)/.exec(msg))) {
        cur.tEnd = t;
        cur.ctxAfter = +g[1];
        cur.gap = Math.max(0, cur.tStart - (lastRelease ?? out.load.loaded ?? 0));
        lastRelease = t;
        if (cur.pe && cur.ev) out.requests.push(cur);
        cur = null;
      }
    }
  }
  // A session cut off mid-request (no release line): keep it as a partial record.
  if (cur && cur.progress.length >= 8 && !cur.pe) {
    const last = cur.progress[cur.progress.length - 1];
    const lastGen = cur.gen[cur.gen.length - 1];
    const total = cur.promptTotal ?? last.n;
    cur.pe = { ms: (last.t * total * 1000) / last.n, n: total, tps: last.n / last.t };
    cur.ev = lastGen ? { ms: 0, n: lastGen.n, tps: lastGen.tg } : { ms: 0, n: 0, tps: 0 };
    cur.partial = true;
    out.partial = cur;
  }
  return out;
}

/** One compact tuple per request. */
function tuple(r) {
  const newTok = r.pe.n;
  const gen = r.ev.n;
  let cached = r.promptTotal != null ? Math.max(0, r.promptTotal - newTok) : Math.max(0, r.ctxAfter - newTok - gen);
  return {
    gapS: r1(r.gap),
    newTokens: newTok,
    cachedTokens: cached,
    prefillTps: r1(r.pe.tps),
    genTokens: gen,
    decodeTps: r2(r.ev.tps),
    // -1 = this session ran without speculative decoding.
    accept: r.draft ? r3(r.draft.acc) : -1,
    meanLen: r.draft ? r2(r.draft.meanLen) : 0,
    ctxAfter: r.ctxAfter,
  };
}

/** Normalised prefill ramp: instantaneous rate / plateau rate, in ten progress bins. */
function prefillRamp(reqs) {
  const bins = Array.from({ length: 10 }, () => []);
  const firstDelay = [];
  for (const r of reqs) {
    const pts = r.progress;
    if (pts.length < 6 || r.pe.n < 4000) continue;
    const plateau = r.pe.tps;
    firstDelay.push(pts[0].t);
    for (let i = 1; i < pts.length; i++) {
      const dt = pts[i].t - pts[i - 1].t;
      const dn = pts[i].n - pts[i - 1].n;
      if (dt <= 0.2) continue;
      const frac = Math.min(0.999, pts[i].p);
      bins[Math.floor(frac * 10)].push(dn / dt / plateau);
    }
  }
  return {
    rampByDecile: bins.map((b) => (b.length ? r2(mean(b)) : 1)),
    firstProgressDelayS: firstDelay.length ? r2(mean(firstDelay)) : 0,
    samples: bins.map((b) => b.length),
  };
}

/** AR(1) description of decode-speed jitter from the 3-second throughput samples. */
function decodeJitter(reqs) {
  const ratios = [];
  const lag = [];
  for (const r of reqs) {
    const ser = r.gen.filter((g) => g.n >= 40).map((g) => g.tg3 / Math.max(1, r.ev.tps));
    for (let i = 0; i < ser.length; i++) {
      if (ser[i] > 0.2 && ser[i] < 2.5) ratios.push(ser[i]);
      if (i > 0 && ser[i] > 0.2 && ser[i] < 2.5 && ser[i - 1] > 0.2 && ser[i - 1] < 2.5) lag.push([ser[i - 1], ser[i]]);
    }
  }
  const m = mean(ratios);
  const v = mean(ratios.map((x) => (x - m) ** 2));
  const cov = mean(lag.map(([a, b]) => (a - m) * (b - m)));
  return { ratioMean: r3(m), ratioSd: r3(Math.sqrt(v)), ar1: r3(Math.max(0, Math.min(0.95, cov / v))), n: ratios.length };
}

function loadStats(sessions) {
  const weights = [];
  const aux = [];
  const total = [];
  for (const s of sessions) {
    const L = s.load;
    if (L.start === undefined || L.loaded === undefined) continue;
    const wEnd = L.threads ?? L.init;
    if (wEnd === undefined) continue;
    weights.push(wEnd - L.start);
    aux.push(L.loaded - wEnd);
    total.push(L.loaded - L.start);
  }
  return {
    weightsS: weights.map(r1),
    auxS: aux.map(r1),
    totalS: total.map(r1),
  };
}

function listLogs(filter) {
  return readdirSync(logsDir)
    .filter(filter)
    .sort()
    .map((f) => join(logsDir, f));
}
const dayOf = (p) => /(\d{8})/.exec(p)?.[1];
const dateRange = (paths) => {
  const d = paths.map(dayOf).filter(Boolean).sort();
  return d.length ? [d[0], d[d.length - 1]] : null;
};

// ---------------------------------------------------------------------------------------------
// MEDIUM tier: Qwen 3.8 27B GSQ sessions
// ---------------------------------------------------------------------------------------------
const gsqPaths = listLogs((f) => /qwen-gsq.*\.err\.log$/.test(f));
const gsq = gsqPaths.map((p) => ({ p, ...parseLlama(readFileSync(p, 'utf8')) }));
const gsqGood = gsq.filter((s) => s.requests.length >= 5);
const gsqAllReqs = gsq.flatMap((s) => s.requests);
const gsqSessionsOut = gsqGood.map((s) => s.requests.map(tuple));
const mediumFixture = {
  provenance: {
    source: 'llama-server session logs (stderr), Qwen 3.8 27B GSQ profile',
    sessionsParsed: gsq.length,
    sessionsUsed: gsqGood.length,
    requests: gsqAllReqs.length,
    dateRange: dateRange(gsqPaths),
  },
  load: loadStats(gsq),
  prefillRamp: prefillRamp(gsqAllReqs),
  decodeJitter: decodeJitter(gsqAllReqs),
  idleGapQuantilesS: quantiles(gsqAllReqs.map((r) => r.gap), [0.1, 0.25, 0.5, 0.75, 0.9, 0.99]),
  prefillTpsPlateauQuantiles: quantiles(gsqAllReqs.filter((r) => r.pe.n >= 2000).map((r) => r.pe.tps)),
  decodeTpsQuantiles: quantiles(gsqAllReqs.filter((r) => r.ev.n >= 100).map((r) => r.ev.tps)),
  acceptQuantiles: quantiles(gsqAllReqs.filter((r) => r.draft && r.draft.generated >= 50).map((r) => r.draft.acc)),
  // Format: arrays of request tuples, oldest first, one array per real session.
  sessions: gsqSessionsOut,
};

// ---------------------------------------------------------------------------------------------
// HIGH tier: Flash-Next (all quants; one big prefill session is split out)
// ---------------------------------------------------------------------------------------------
const fnPaths = [
  ...listLogs((f) => /^flashnext-.*\.log$/.test(f)),
  ...listLogs((f) => /qwen-fn.*\.err\.log$/.test(f)),
];
const fn = fnPaths.map((p) => ({ p, ...parseLlama(readFileSync(p, 'utf8')) }));
// Several flashnext-*.log files duplicate the matching klif-*-qwen-fn-*.err.log: dedupe by identical request tuples.
const seen = new Set();
const fnUnique = [];
for (const s of fn) {
  if (!s.requests.length) continue;
  const key = JSON.stringify(s.requests.slice(0, 3).map((r) => [r.pe.n, r.ev.n]));
  if (seen.has(key)) continue;
  seen.add(key);
  fnUnique.push(s);
}
const fnAllReqs = fnUnique.flatMap((s) => s.requests);
const bigPrefill = [...fnAllReqs, ...fn.map((s) => s.partial).filter(Boolean)]
  .filter((r) => r.pe.n >= 50000 && r.progress.length >= 8)
  .sort((a, b) => b.pe.tps - a.pe.tps)[0];
let bigFixture = null;
if (bigPrefill) {
  bigFixture = {
    promptTokens: bigPrefill.promptTotal ?? bigPrefill.pe.n,
    processedTokens: bigPrefill.pe.n,
    prefillTpsPlateau: r1(bigPrefill.pe.tps),
    prefillS: r1(bigPrefill.pe.ms / 1000),
    // [doneTokens, elapsedS] checkpoints from the real prompt-processing progress lines (elapsed from slot start).
    progress: bigPrefill.progress.map((p) => [p.n, r2(p.t)]),
    decodeTps: r2(bigPrefill.ev.tps),
    genTokens: bigPrefill.ev.n,
    accept: bigPrefill.draft ? r3(bigPrefill.draft.acc) : -1,
    // Decode tps (3 s window) samples, oldest first; one per ~3 s of the real session.
    decodeSeries: bigPrefill.gen.map((g) => r2(g.tg3)),
  };
}
const highFixture = {
  provenance: {
    source: 'llama-server session logs (stderr), Flash-Next profiles (several quantisations)',
    sessionsUsed: fnUnique.length,
    requests: fnAllReqs.length,
    dateRange: dateRange(fnPaths),
  },
  load: loadStats(fnUnique),
  prefillRamp: prefillRamp(fnAllReqs),
  decodeJitter: decodeJitter(fnAllReqs),
  idleGapQuantilesS: quantiles(fnAllReqs.map((r) => r.gap), [0.1, 0.25, 0.5, 0.75, 0.9, 0.99]),
  prefillTpsPlateauQuantiles: quantiles(fnAllReqs.filter((r) => r.pe.n >= 2000).map((r) => r.pe.tps)),
  decodeTpsQuantiles: quantiles(fnAllReqs.filter((r) => r.ev.n >= 100).map((r) => r.ev.tps)),
  acceptQuantiles: quantiles(fnAllReqs.filter((r) => r.draft && r.draft.generated >= 50).map((r) => r.draft.acc)),
  bigPrefill: bigFixture,
  // Replay pool (capped), oldest first.
  sessions: fnUnique
    .map((s) => s.requests.map(tuple))
    .filter((a) => a.length >= 3)
    .map((a) => a.slice(0, 120)),
};

// ---------------------------------------------------------------------------------------------
// Krea / sd.cpp image server (stdout, \r-separated progress)
// ---------------------------------------------------------------------------------------------
function parseKrea(text) {
  const lines = text
    .replace(/\x1b\[K/g, '')
    .split(/[\r\n]+/);
  const jobs = [];
  let job = null;
  let listeningSeen = false;
  const loadTaking = [];
  for (const l of lines) {
    let g;
    if (/main\.cpp:\d+\s+- listening on/.test(l)) listeningSeen = true;
    if (!listeningSeen && (g = /loading tensors completed, taking ([\d.]+)s/.exec(l))) loadTaking.push(+g[1]);
    if ((g = /generate_image (\d+)x(\d+)/.exec(l)) && /stable-diffusion\.cpp:\d+\s+- generate_image \d+x\d+/.test(l)) {
      job = { w: +g[1], h: +g[2], edit: false, lora: 0, encode: 0, cond: 0, steps: [], stepRates: [], sample: 0, decode: 0, total: 0 };
      continue;
    }
    if (!job) continue;
    if (/EDIT mode/.test(l)) job.edit = true;
    else if ((g = /apply_loras completed, taking ([\d.]+)s/.exec(l))) job.lora = +g[1];
    else if ((g = /encode_first_stage completed, taking ([\d.]+)s/.exec(l))) job.encode = +g[1];
    else if ((g = /get_learned_condition completed, taking ([\d.]+)s/.exec(l))) job.cond = +g[1];
    else if ((g = /sampling completed, taking ([\d.]+)s/.exec(l))) job.sample = +g[1];
    else if ((g = /decode_first_stage completed, taking ([\d.]+)s/.exec(l))) job.decode = +g[1];
    else if ((g = /generate_image completed in ([\d.]+)s/.exec(l))) {
      job.total = +g[1];
      jobs.push(job);
      job = null;
    } else {
      const re = /(\d+)\/(\d+) - ([\d.]+)(s\/it|it\/s)/g;
      let s;
      while ((s = re.exec(l))) {
        const step = +s[1];
        const total = +s[2];
        if (total > 64 || total < 2) continue; // tensor-load bars, not sampling steps
        const v = +s[3];
        const sec = s[4] === 's/it' ? v : 1 / v;
        if (step === job.steps.length + 1) {
          job.steps.push(sec);
          job.nSteps = total;
        }
      }
    }
  }
  return { jobs, loadTaking };
}

const kreaPaths = listLogs((f) => /krea.*\.out\.log$/.test(f));
const krea = kreaPaths.map((p) => {
  const t = readFileSync(p, 'utf8');
  const st = statSync(p);
  const startStamp = /-(\d{8})-(\d{6})-/.exec(p);
  return { p, st, startStamp, ...parseKrea(t) };
});
const kreaJobs = [];
const dutyEstimates = [];
for (const k of krea) {
  const good = k.jobs.filter((j) => j.steps.length >= 4 && j.nSteps === j.steps.length && j.total > 0);
  for (const j of good) kreaJobs.push(j);
  if (good.length >= 5 && k.startStamp) {
    // wall span of the session from the filename start stamp (local time) to the file mtime (local time)
    const [, d, t] = k.startStamp;
    const start = new Date(+d.slice(0, 4), +d.slice(4, 6) - 1, +d.slice(6, 8), +t.slice(0, 2), +t.slice(2, 4), +t.slice(4, 6));
    const spanS = (k.st.mtimeMs - start.getTime()) / 1000;
    const busyS = good.reduce((a, j) => a + j.total, 0);
    if (spanS > busyS) dutyEstimates.push({ spanS, busyS, n: good.length });
  }
}
function jobTuple(j) {
  const rest = j.steps.slice(1);
  return [
    j.w,
    j.h,
    j.edit ? 1 : 0,
    j.steps.length,
    r2(j.lora + j.encode), // preparation before conditioning (edit jobs only)
    r2(j.cond),
    r2(j.steps[0]), // first sampling step is slower (graph build / streaming warm-up)
    r2(mean(rest)),
    r2(j.decode),
    r2(j.total),
  ];
}
const pool = kreaJobs;
const edits = pool.filter((j) => j.edit);
const plain = pool.filter((j) => !j.edit);
// Cap the replay pool: keep every edit job we can (they are rarer) and a stride of the rest.
const stride = Math.max(1, Math.floor(plain.length / 260));
const poolOut = [...plain.filter((_, i) => i % stride === 0), ...edits.filter((_, i) => i % Math.max(1, Math.floor(edits.length / 90)) === 0)];
const sizeCounts = {};
for (const j of pool) sizeCounts[`${j.w}x${j.h}`] = (sizeCounts[`${j.w}x${j.h}`] ?? 0) + 1;
const duty = dutyEstimates.length ? dutyEstimates.reduce((a, d) => a + d.busyS, 0) / dutyEstimates.reduce((a, d) => a + d.spanS, 0) : null;
const kreaFixture = {
  provenance: {
    source: 'sd.cpp image server session logs (stdout), Krea 2 profiles',
    sessionsParsed: krea.length,
    jobsParsed: pool.length,
    editJobs: edits.length,
    dateRange: dateRange(kreaPaths),
  },
  sizeCounts,
  editShare: r3(edits.length / pool.length),
  totalSQuantiles: quantiles(pool.map((j) => j.total)),
  totalSQuantilesEdit: quantiles(edits.map((j) => j.total)),
  totalSQuantilesPlain: quantiles(plain.map((j) => j.total)),
  steps: quantiles(pool.map((j) => j.steps.length), [0.05, 0.5, 0.95]),
  // fraction of wall time the server spent generating (sessions with >= 5 jobs): upper bound on duty cycle.
  dutyCycle: duty === null ? null : r3(duty),
  // Typical load stage durations (sum of "loading tensors" seconds logged before the server starts listening).
  loadSecondsQuantiles: quantiles(krea.map((k) => k.loadTaking.reduce((a, b) => a + b, 0)).filter((v) => v > 0)),
  // [width, height, edit(0|1), steps, prepS, condS, firstStepS, otherStepsMeanS, decodeS, totalS]
  jobsFormat: ['w', 'h', 'edit', 'steps', 'prepS', 'condS', 'firstStepS', 'restStepS', 'decodeS', 'totalS'],
  jobs: poolOut.map(jobTuple),
};

// ---------------------------------------------------------------------------------------------
// LOW tier: Gemma 4 26B-A4B. No session logs exist for this tier yet; use the measured benchmark file.
// ---------------------------------------------------------------------------------------------
let lowFixture = {
  provenance: { source: 'none (no Gemma session logs); decode figures from the local benchmark summary if provided', measured: false },
};
const benchPath = arg('bench');
if (benchPath) {
  try {
    const j = JSON.parse(readFileSync(benchPath, 'utf8').replace(/^﻿/, ''));
    const g = j.gemma_4;
    const m26 = g.margin_8192_vs_1024_at_128k.find((x) => x.model === '26B-A4B');
    const v32 = g.converted_profiles_verified.find((x) => /26B-A4B 32k/.test(x.label));
    lowFixture = {
      provenance: { source: 'local benchmark summary, Gemma 4 26B-A4B (no session logs)', measured: true, date: j.measured_at },
      decodeTpsAt128k: m26?.after_tps ?? null,
      decodeTpsAt32k: v32?.tokens_per_second ?? null,
      committedGiBAt32k: v32?.committed_gib ?? null,
      committedGiBAt128k: m26?.committed_gib_after ?? null,
    };
  } catch (e) {
    console.error('bench parse failed:', e.message);
  }
}

// Gemma load time: a short 16k session in the measurement logs shares the same weights file size
// class; if a measure-logs folder sits next to runtime-logs, take the load time from its Gemma 26B logs.
try {
  const measureDir = join(dirname(resolve(logsDir)), 'measure-logs');
  const g = readdirSync(measureDir).filter((f) => /Gemma_26B-A4B.*\.err\.log$/.test(f));
  const loads = g.map((f) => parseLlama(readFileSync(join(measureDir, f), 'utf8')).load).filter((L) => L.start !== undefined && L.loaded !== undefined);
  if (loads.length) lowFixture.loadTotalS = loads.map((L) => r1(L.loaded - L.start));
} catch {
  /* optional */
}

function write(name, obj) {
  const p = join(outDir, name);
  writeFileSync(p, JSON.stringify(obj) + '\n');
  console.log(name, statSync(p).size, 'bytes');
}
write('llm-medium.json', mediumFixture);
write('llm-high.json', highFixture);
write('image-krea.json', kreaFixture);
write('llm-low.json', lowFixture);
