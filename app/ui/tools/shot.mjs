// Headless screenshot of the KLIF UI on the registered dev server (no server is started here).
//
//   node app/ui/tools/shot.mjs --skin cliff --size full --scenario live --out .studio/shots/cliff-full.png
//   node app/ui/tools/shot.mjs --url "http://127.0.0.1:5193/?skin=phosphor&size=mini" --w 960 --h 640 --out x.png
//
// Uses the installed Chrome/Edge via playwright-core with software GL (SwiftShader), so it never
// touches the inference GPU. Visual verification only; not a test suite.
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 && i + 1 < process.argv.length ? process.argv[i + 1] : fallback;
}

const reg = JSON.parse(readFileSync(resolve(root, '.studio/devserver.json'), 'utf8').replace(/^﻿/, ''));
const base = `http://${reg.host ?? '127.0.0.1'}:${reg.port}/`;

const size = arg('size', 'full');
const query = new URLSearchParams();
for (const k of ['skin', 'size', 'scenario', 't']) {
  const v = arg(k, undefined);
  if (v !== undefined) query.set(k, v);
}
query.set('shot', '1');
const url = arg('url', `${base}?${query}`);
const w = Number(arg('w', size === 'mini' ? 960 : 1024));
const h = Number(arg('h', size === 'mini' ? 640 : 1152));
const dpr = Number(arg('dpr', 1));
const waitMs = Number(arg('wait', 1500));
const out = resolve(root, arg('out', `.studio/shots/shot-${Date.now()}.png`));

const candidates = [
  process.env.KLIF_SHOT_BROWSER,
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
].filter(Boolean);
const executablePath = candidates.find((p) => existsSync(p));
if (!executablePath) throw new Error('No Chrome/Edge found; set KLIF_SHOT_BROWSER');

const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-gpu-compositing'],
});
try {
  const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: dpr });
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  await page.goto(url, { waitUntil: 'networkidle' });
  await page.waitForTimeout(waitMs);
  mkdirSync(dirname(out), { recursive: true });
  await page.screenshot({ path: out });
  console.log(JSON.stringify({ out, url, w, h, dpr, errors }));
} finally {
  await browser.close();
}
