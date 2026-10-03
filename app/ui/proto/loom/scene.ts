// Loom prototype: how a transformer runs, in 3D, on an amber phosphor screen, driven by the view model.
//   tower of slabs    = the model's layers (bottom: embedding, top: logits)
//   cells on a slab   = experts: a router forks each token into k beams to the chosen experts and merges them
//                       back above the layer (MoE); a dense layer works with all its units
//   threads           = tokens rising through the residual stream, light-painted (decode: one at a time,
//                       time-dilated; prefill: a curtain of hundreds at once)
//   helix             = the KV cache; its lit length is the real context fill; a prompt-cache hit lights the
//                       reused part at once
//   arcs              = attention from the token being computed to cached tokens (illustrative pattern)
//   distribution      = logits as a probability curve with the top-k lit; the sample flies to the helix
//   draft tower       = speculative decoding: ghost tokens fly in, are verified in one pass, accepted ones are
//                       written, rejected ones burst
//   glass cylinder    = VRAM capacity with the fill level; spill glows red
// The moving light (threads, beams, arcs, sparks, the distribution) keeps a phosphor afterglow; the whole image
// goes through a CRT pass (curvature, scanlines, slight colour fringing, grain). A camera director frames events.
import * as THREE from 'three';
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js';
import { OutputPass } from 'three/examples/jsm/postprocessing/OutputPass.js';
import { ShaderPass } from 'three/examples/jsm/postprocessing/ShaderPass.js';
import { Pass, FullScreenQuad } from 'three/examples/jsm/postprocessing/Pass.js';
import type { ViewModel } from '../../src/lib/model/types';

type Mode = 'idle' | 'decode' | 'prefill' | 'load' | 'denoise' | 'sleep' | 'fault' | 'stop';

/** Architecture per model. The real skin would read n_layer / n_expert / n_expert_used from the server log. */
interface Arch {
  kind: 'moe' | 'dense' | 'dit';
  layers: number;
  experts: number;
  active: number;
  shared: number;
}
const ARCHES: [RegExp, Arch][] = [
  [/flash-next/i, { kind: 'moe', layers: 48, experts: 512, active: 10, shared: 1 }],
  [/gemma/i, { kind: 'moe', layers: 30, experts: 128, active: 8, shared: 1 }],
  [/krea/i, { kind: 'dit', layers: 28, experts: 0, active: 0, shared: 0 }],
  [/./, { kind: 'dense', layers: 64, experts: 0, active: 0, shared: 0 }],
];
export function archOf(name: string | undefined): Arch {
  return (ARCHES.find(([re]) => re.test(name ?? '')) ?? ARCHES[ARCHES.length - 1])[1];
}

// ---- amber phosphor ----------------------------------------------------------------------------------------------
const AMBER = new THREE.Color('#ffb000');
const HOT = new THREE.Color('#ffd98a');
const WHITE = new THREE.Color('#fff4de');
const EMBER = new THREE.Color('#ff6a1a');
const DIM = new THREE.Color('#0e0601');
const GHOST = new THREE.Color('#8a5a1c');
const RED = new THREE.Color('#ff2e2e');

const H = 11; // tower height
const SLAB = 5.2;
const HELIX_R = 4.25;
const HELIX_N = 2600;
const CYL_R = 5.6;
const DENSE_GRID = 16;
const FAN_N = 48;
const TOP_K = 20;
const CRT_CURVE = 0.045;
const DRAFT_POS = new THREE.Vector3(-8.2, -H / 2 + 0.4, 1.2);

const clamp = (x: number, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const approach = (x: number, target: number, rate: number, dt: number) => x + (target - x) * (1 - Math.exp(-rate * dt));
function rng(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return (s >>> 0) / 4294967296;
  };
}

function dotTexture(): THREE.Texture {
  const c = document.createElement('canvas');
  c.width = c.height = 64;
  const x = c.getContext('2d')!;
  const g = x.createRadialGradient(32, 32, 0, 32, 32, 32);
  g.addColorStop(0, 'rgba(255,255,255,1)');
  g.addColorStop(0.25, 'rgba(255,255,255,0.8)');
  g.addColorStop(1, 'rgba(255,255,255,0)');
  x.fillStyle = g;
  x.fillRect(0, 0, 64, 64);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

// ---- post: phosphor afterglow for the moving light, then the CRT screen --------------------------------------------
const QUAD_VS = `varying vec2 vUv; void main() { vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`;

/** Renders the fx scene, keeps max(previous * decay, new) per pixel (a phosphor's persistence), adds it on top. */
class PhosphorPass extends Pass {
  decay = 0.9;
  private fx: THREE.WebGLRenderTarget;
  private a: THREE.WebGLRenderTarget;
  private b: THREE.WebGLRenderTarget;
  private keep: THREE.ShaderMaterial;
  private add: THREE.ShaderMaterial;
  private quad: FullScreenQuad;
  constructor(
    private fxScene: THREE.Scene,
    private camera: THREE.Camera,
  ) {
    super();
    const opt = { type: THREE.HalfFloatType };
    this.fx = new THREE.WebGLRenderTarget(1, 1, opt);
    this.a = new THREE.WebGLRenderTarget(1, 1, opt);
    this.b = new THREE.WebGLRenderTarget(1, 1, opt);
    this.keep = new THREE.ShaderMaterial({
      uniforms: { tPrev: { value: null }, tNew: { value: null }, decay: { value: 0.9 } },
      vertexShader: QUAD_VS,
      fragmentShader: `uniform sampler2D tPrev; uniform sampler2D tNew; uniform float decay; varying vec2 vUv;
        void main() { vec4 p = texture2D(tPrev, vUv) * decay; vec4 n = texture2D(tNew, vUv); gl_FragColor = max(p, n); }`,
    });
    this.add = new THREE.ShaderMaterial({
      uniforms: { tBase: { value: null }, tGlow: { value: null } },
      vertexShader: QUAD_VS,
      fragmentShader: `uniform sampler2D tBase; uniform sampler2D tGlow; varying vec2 vUv;
        void main() { gl_FragColor = vec4(texture2D(tBase, vUv).rgb + texture2D(tGlow, vUv).rgb, 1.0); }`,
    });
    this.quad = new FullScreenQuad(this.keep);
  }
  setSize(w: number, h: number) {
    this.fx.setSize(w, h);
    this.a.setSize(w, h);
    this.b.setSize(w, h);
  }
  render(renderer: THREE.WebGLRenderer, writeBuffer: THREE.WebGLRenderTarget, readBuffer: THREE.WebGLRenderTarget) {
    const clear = renderer.getClearColor(new THREE.Color());
    const alpha = renderer.getClearAlpha();
    renderer.setClearColor(0x000000, 1);
    renderer.setRenderTarget(this.fx);
    renderer.clear();
    renderer.render(this.fxScene, this.camera);
    this.keep.uniforms.tPrev.value = this.a.texture;
    this.keep.uniforms.tNew.value = this.fx.texture;
    this.keep.uniforms.decay.value = this.decay;
    this.quad.material = this.keep;
    renderer.setRenderTarget(this.b);
    this.quad.render(renderer);
    [this.a, this.b] = [this.b, this.a];
    this.add.uniforms.tBase.value = readBuffer.texture;
    this.add.uniforms.tGlow.value = this.a.texture;
    this.quad.material = this.add;
    renderer.setRenderTarget(this.renderToScreen ? null : writeBuffer);
    this.quad.render(renderer);
    renderer.setClearColor(clear, alpha);
  }
}

const CRT = {
  uniforms: { tDiffuse: { value: null }, resolution: { value: new THREE.Vector2(1, 1) }, time: { value: 0 }, curve: { value: CRT_CURVE } },
  vertexShader: QUAD_VS,
  fragmentShader: `uniform sampler2D tDiffuse; uniform vec2 resolution; uniform float time; uniform float curve; varying vec2 vUv;
    vec2 barrel(vec2 uv) { vec2 c = uv * 2.0 - 1.0; c *= 1.0 + curve * dot(c, c); return c * 0.5 + 0.5; }
    void main() {
      vec2 uv = barrel(vUv);
      if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0); return; }
      float ca = 0.0016 * length(uv - 0.5) * 2.0;
      vec3 col = vec3(texture2D(tDiffuse, uv + vec2(ca, 0.0)).r, texture2D(tDiffuse, uv).g, texture2D(tDiffuse, uv - vec2(ca, 0.0)).b);
      col *= 0.8 + 0.2 * sin(uv.y * resolution.y * 2.0944);
      vec2 d = uv - 0.5;
      col *= 1.0 - dot(d, d) * 1.1;
      float n = fract(sin(dot(uv * resolution + time * 61.0, vec2(12.9898, 78.233))) * 43758.5453);
      col += (n - 0.5) * 0.03 * vec3(1.0, 0.7, 0.3);
      gl_FragColor = vec4(col, 1.0);
    }`,
};

// ---- dynamic line / point pools (rebuilt every frame) -----------------------------------------------------------
class Lines {
  private pos: THREE.BufferAttribute;
  private col: THREE.BufferAttribute;
  private obj: THREE.LineSegments;
  private n = 0;
  constructor(
    private cap: number,
    parent: THREE.Object3D,
  ) {
    const g = new THREE.BufferGeometry();
    this.pos = new THREE.BufferAttribute(new Float32Array(cap * 6), 3);
    this.col = new THREE.BufferAttribute(new Float32Array(cap * 6), 3);
    g.setAttribute('position', this.pos);
    g.setAttribute('color', this.col);
    this.obj = new THREE.LineSegments(g, new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
    this.obj.frustumCulled = false;
    parent.add(this.obj);
  }
  begin() {
    this.n = 0;
  }
  seg(a: THREE.Vector3 | number[], b: THREE.Vector3 | number[], ca: THREE.Color, ka: number, cb: THREE.Color, kb: number) {
    if (this.n >= this.cap) return;
    const i = this.n++ * 2;
    const A = a instanceof THREE.Vector3 ? a.toArray() : a;
    const B = b instanceof THREE.Vector3 ? b.toArray() : b;
    this.pos.setXYZ(i, A[0], A[1], A[2]);
    this.pos.setXYZ(i + 1, B[0], B[1], B[2]);
    this.col.setXYZ(i, ca.r * ka, ca.g * ka, ca.b * ka);
    this.col.setXYZ(i + 1, cb.r * kb, cb.g * kb, cb.b * kb);
  }
  end() {
    this.obj.geometry.setDrawRange(0, Math.max(2, this.n * 2));
    this.pos.needsUpdate = this.col.needsUpdate = true;
  }
}

class Dots {
  private pos: THREE.BufferAttribute;
  private col: THREE.BufferAttribute;
  private obj: THREE.Points;
  private n = 0;
  constructor(
    private cap: number,
    size: number,
    map: THREE.Texture,
    parent: THREE.Object3D,
  ) {
    const g = new THREE.BufferGeometry();
    this.pos = new THREE.BufferAttribute(new Float32Array(cap * 3), 3);
    this.col = new THREE.BufferAttribute(new Float32Array(cap * 3), 3);
    g.setAttribute('position', this.pos);
    g.setAttribute('color', this.col);
    this.obj = new THREE.Points(g, new THREE.PointsMaterial({ size, map, vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
    this.obj.frustumCulled = false;
    parent.add(this.obj);
  }
  begin() {
    this.n = 0;
  }
  dot(x: number, y: number, z: number, c: THREE.Color, k: number) {
    if (this.n >= this.cap) return;
    const i = this.n++;
    this.pos.setXYZ(i, x, y, z);
    this.col.setXYZ(i, c.r * k, c.g * k, c.b * k);
  }
  end() {
    this.obj.geometry.setDrawRange(0, Math.max(1, this.n));
    this.pos.needsUpdate = this.col.needsUpdate = true;
  }
}

interface Pulse {
  y: number;
  x: number;
  z: number;
  speed: number;
  ghost: boolean;
  accept: boolean;
  lastLayer: number;
  bright: number;
}
interface Beam {
  a: THREE.Vector3;
  b: THREE.Vector3;
  life: number;
  k: number;
}
interface Arc {
  pts: THREE.Vector3[];
  life: number;
}
interface Mote {
  t: number;
  speed: number;
  from: THREE.Vector3;
  to: THREE.Vector3;
  lift: number;
  kind: 'write' | 'draft';
  ok: boolean;
  x: number;
}
interface Spark {
  p: THREE.Vector3;
  v: THREE.Vector3;
  life: number;
}

export interface Labels {
  [id: string]: HTMLElement | undefined;
}

export class LoomScene {
  private renderer: THREE.WebGLRenderer;
  private composer: EffectComposer;
  private bloom: UnrealBloomPass;
  private phosphor: PhosphorPass;
  private crt: ShaderPass;
  private scene = new THREE.Scene();
  private fx = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(32, 1, 0.1, 200);
  private root = new THREE.Group();
  private tower = new THREE.Group();
  private arch: Arch = archOf('');
  private archKey = '';
  private dot = dotTexture();
  // tower
  private slabEdges!: THREE.LineSegments;
  private slabFill!: THREE.InstancedMesh;
  private cells!: THREE.InstancedMesh;
  private cellsPerLayer = 0;
  private cellXZ: [number, number][] = [];
  private cellAct = new Float32Array(0);
  private activeCells = new Set<number>();
  private layerAct = new Float32Array(0);
  private layerY: number[] = [];
  // helix
  private helix!: THREE.Points;
  private helixPos: THREE.Vector3[] = [];
  private helixUsed = 0;
  private helixFlash = new Float32Array(HELIX_N);
  // fx pools
  private threads!: Lines;
  private heads!: Dots;
  private beamLines!: Lines;
  private arcLines!: Lines;
  private motesDots!: Dots;
  private sparkDots!: Dots;
  private fanLines!: Lines;
  private pulses: Pulse[] = [];
  private beams: Beam[] = [];
  private arcs: Arc[] = [];
  private motes: Mote[] = [];
  private sparks: Spark[] = [];
  // logits
  private fanP = new Float32Array(FAN_N);
  private fanTop: number[] = [];
  private fanWinner = -1;
  private fanFlash = 0;
  // vram
  private cylinder!: THREE.LineSegments;
  private fillRing!: THREE.LineLoop;
  private fillLevel = 0;
  // draft tower
  private draft = new THREE.Group();
  private draftEdges!: THREE.LineSegments;
  private draftAct = 0;
  // krea
  private patches!: THREE.InstancedMesh;
  private patchGroup = new THREE.Group();
  private patchNoise = new Float32Array(24 * 16);
  // state
  private vm: ViewModel | null = null;
  private mode: Mode = 'idle';
  private prevMode: Mode = 'idle';
  private t = 0;
  private last = 0;
  private raf = 0;
  private orbit = 0.6;
  private cam = { dist: 31, y: 6, look: 0, speed: 0.05 };
  private curtainY = -H / 2;
  private reqEndAt = -10;
  private spawnAcc = 0;
  private curtainAcc = 0;
  private stepSeen = -1;
  private reqSeen = -1;
  private visible = 0;
  private glow = 1;
  private faultAt = -1;
  private r = rng(1234);
  private labels: Labels = {};
  private size = new THREE.Vector2(1, 1);
  /** Decode tokens actually visualised per second (time dilation shown in the UI). */
  shownTps = 0;
  /** Prompt-cache tokens reused by the request now prefilling (0 when none). */
  cacheHit = 0;

  constructor(private canvas: HTMLCanvasElement) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, powerPreference: 'high-performance' });
    this.renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1));
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 1.05;
    this.scene.background = new THREE.Color('#050302');
    this.scene.fog = new THREE.FogExp2(0x050302, 0.026);
    this.fx.fog = this.scene.fog;
    this.scene.add(this.root);
    this.root.add(this.tower);
    this.composer = new EffectComposer(this.renderer);
    this.composer.addPass(new RenderPass(this.scene, this.camera));
    this.phosphor = new PhosphorPass(this.fx, this.camera);
    this.composer.addPass(this.phosphor);
    this.bloom = new UnrealBloomPass(new THREE.Vector2(512, 512), 0.85, 0.5, 0.12);
    this.composer.addPass(this.bloom);
    this.crt = new ShaderPass(CRT);
    this.composer.addPass(this.crt);
    this.composer.addPass(new OutputPass());
    this.buildStatic();
    this.buildTower(this.arch);
  }

  setLabels(l: Labels) {
    this.labels = l;
  }

  setVm(vm: ViewModel) {
    this.vm = vm;
    const name = vm.session?.model.name ?? vm.systems.find((s) => s.id === vm.selected)?.model.name;
    const a = archOf(name);
    const key = `${a.kind}${a.layers}${a.experts}`;
    if (key !== this.archKey) {
      this.archKey = key;
      this.arch = a;
      this.buildTower(a);
    }
    const reqs = vm.session?.llm?.requests ?? [];
    const last = reqs[reqs.length - 1];
    if (last && last.id !== this.reqSeen) {
      if (this.reqSeen !== -1) {
        this.fanFlash = 1.5;
        this.reqEndAt = this.t;
      }
      this.reqSeen = last.id;
    }
    const img = vm.session?.image;
    if (img && img.activity === 'generating') {
      if (img.step !== this.stepSeen) {
        this.stepSeen = img.step;
        this.pulses.push({ y: -H / 2 - 0.6, x: 0, z: 0, speed: H / Math.max(0.6, Math.min(2.6, img.sPerIt * 0.7)), ghost: false, accept: true, lastLayer: -1, bright: 1.6 });
      }
    } else this.stepSeen = -1;
    this.draft.visible = !!vm.session?.llm?.spec && this.arch.kind !== 'dit';
  }

  resize(w: number, h: number) {
    this.renderer.setSize(w, h, false);
    this.composer.setSize(w, h);
    this.bloom.setSize(w, h);
    const pr = this.renderer.getPixelRatio();
    this.size.set(w, h);
    (this.crt.uniforms.resolution.value as THREE.Vector2).set(w * pr, h * pr);
    this.camera.aspect = w / Math.max(1, h);
    this.camera.updateProjectionMatrix();
  }

  start() {
    const loop = (now: number) => {
      const dt = this.last ? Math.min(0.05, (now - this.last) / 1000) : 0.016;
      this.last = now;
      this.frame(dt);
      this.raf = requestAnimationFrame(loop);
    };
    this.raf = requestAnimationFrame(loop);
  }

  stop() {
    cancelAnimationFrame(this.raf);
  }

  // ---- building ---------------------------------------------------------------------------------------------------
  private buildStatic() {
    const pos = new Float32Array(HELIX_N * 3);
    this.helixPos = [];
    for (let k = 0; k < HELIX_N; k++) {
      const u = k / HELIX_N;
      const a = u * Math.PI * 2 * 9;
      const p = new THREE.Vector3(Math.cos(a) * HELIX_R, -H / 2 + u * H, Math.sin(a) * HELIX_R);
      this.helixPos.push(p);
      pos.set([p.x, p.y, p.z], k * 3);
    }
    const hg = new THREE.BufferGeometry();
    hg.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    hg.setAttribute('color', new THREE.BufferAttribute(new Float32Array(HELIX_N * 3), 3));
    this.helix = new THREE.Points(hg, new THREE.PointsMaterial({ size: 0.16, map: this.dot, vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
    this.root.add(this.helix);

    // fx pools: everything that moves leaves a phosphor afterglow
    this.threads = new Lines(3200, this.fx);
    this.heads = new Dots(3200, 0.42, this.dot, this.fx);
    this.beamLines = new Lines(5000, this.fx);
    this.arcLines = new Lines(72 * 20, this.fx);
    this.motesDots = new Dots(96, 0.5, this.dot, this.fx);
    this.sparkDots = new Dots(600, 0.22, this.dot, this.fx);
    this.fanLines = new Lines(FAN_N * 2 + 2, this.fx);

    // VRAM cylinder, fill ring, ticks
    const cyl = new THREE.CylinderGeometry(CYL_R, CYL_R, H + 2.4, 64, 1, true);
    this.cylinder = new THREE.LineSegments(new THREE.EdgesGeometry(cyl, 1), new THREE.LineBasicMaterial({ color: GHOST, transparent: true, opacity: 0.4, depthWrite: false }));
    this.root.add(this.cylinder);
    const ringPts: THREE.Vector3[] = [];
    for (let i = 0; i < 96; i++) {
      const a = (i / 96) * Math.PI * 2;
      ringPts.push(new THREE.Vector3(Math.cos(a) * CYL_R, 0, Math.sin(a) * CYL_R));
    }
    this.fillRing = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(ringPts), new THREE.LineBasicMaterial({ color: AMBER, transparent: true, opacity: 0.9, depthWrite: false, blending: THREE.AdditiveBlending }));
    this.root.add(this.fillRing);
    const ticks: number[] = [];
    for (let i = 0; i <= 16; i++) {
      const y = -H / 2 - 1.2 + ((H + 2.4) * i) / 16;
      ticks.push(CYL_R, y, 0, CYL_R + 0.25, y, 0);
    }
    const tg = new THREE.BufferGeometry();
    tg.setAttribute('position', new THREE.Float32BufferAttribute(ticks, 3));
    this.root.add(new THREE.LineSegments(tg, new THREE.LineBasicMaterial({ color: GHOST })));

    // draft tower (speculative decoding): a small stack beside the model
    const dpos: number[] = [];
    const s = 0.75;
    for (let i = 0; i < 6; i++) {
      const y = i * 0.45;
      dpos.push(-s, y, -s, s, y, -s, s, y, -s, s, y, s, s, y, s, -s, y, s, -s, y, s, -s, y, -s);
    }
    dpos.push(-s, 0, -s, -s, 2.25, -s, s, 0, -s, s, 2.25, -s, s, 0, s, s, 2.25, s, -s, 0, s, -s, 2.25, s);
    const dg = new THREE.BufferGeometry();
    dg.setAttribute('position', new THREE.Float32BufferAttribute(dpos, 3));
    this.draftEdges = new THREE.LineSegments(dg, new THREE.LineBasicMaterial({ color: GHOST, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
    this.draft.add(this.draftEdges);
    this.draft.position.copy(DRAFT_POS);
    this.draft.visible = false;
    this.root.add(this.draft);

    // Krea patch plane (billboard beside the tower)
    this.patches = new THREE.InstancedMesh(
      new THREE.PlaneGeometry(0.22, 0.22),
      new THREE.MeshBasicMaterial({ transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, side: THREE.DoubleSide }),
      24 * 16,
    );
    const m = new THREE.Matrix4();
    for (let j = 0; j < 16; j++)
      for (let i = 0; i < 24; i++) {
        m.makeTranslation((i - 11.5) * 0.25, (7.5 - j) * 0.25, 0);
        this.patches.setMatrixAt(j * 24 + i, m);
        this.patches.setColorAt(j * 24 + i, DIM);
      }
    this.patchGroup.add(this.patches);
    this.patchGroup.visible = false;
    this.scene.add(this.patchGroup);

    const grid = new THREE.PolarGridHelper(CYL_R + 2.5, 24, 8, 96, 0x2a1604, 0x1a0e03);
    grid.position.y = -H / 2 - 1.25;
    this.root.add(grid);
  }

  private buildTower(a: Arch) {
    this.tower.clear();
    const n = a.layers;
    this.layerY = Array.from({ length: n }, (_, i) => -H / 2 + (i / (n - 1)) * H);
    this.layerAct = new Float32Array(n);
    const s = SLAB / 2;
    const pos: number[] = [];
    for (const y of this.layerY) pos.push(-s, y, -s, s, y, -s, s, y, -s, s, y, s, s, y, s, -s, y, s, -s, y, s, -s, y, -s);
    const eg = new THREE.BufferGeometry();
    eg.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    eg.setAttribute('color', new THREE.BufferAttribute(new Float32Array(pos.length), 3));
    this.slabEdges = new THREE.LineSegments(eg, new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
    this.tower.add(this.slabEdges);
    this.slabFill = new THREE.InstancedMesh(new THREE.BoxGeometry(SLAB, 0.012, SLAB), new THREE.MeshBasicMaterial({ transparent: true, opacity: 0.5, depthWrite: false, blending: THREE.AdditiveBlending }), n);
    const m = new THREE.Matrix4();
    this.layerY.forEach((y, i) => {
      m.makeTranslation(0, y, 0);
      this.slabFill.setMatrixAt(i, m);
      this.slabFill.setColorAt(i, DIM);
    });
    this.tower.add(this.slabFill);
    const perLayer = a.kind === 'moe' ? a.experts + a.shared : DENSE_GRID * DENSE_GRID;
    this.cellsPerLayer = perLayer;
    const cellSize = a.kind === 'moe' ? (a.experts > 256 ? 0.075 : 0.13) : 0.2;
    this.cells = new THREE.InstancedMesh(new THREE.BoxGeometry(cellSize, 0.03, cellSize), new THREE.MeshBasicMaterial({ transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }), n * perLayer);
    const golden = Math.PI * (3 - Math.sqrt(5));
    this.cellXZ = [];
    for (let k = 0; k < perLayer; k++) {
      if (a.kind === 'moe') {
        if (k < a.shared) this.cellXZ.push([0, 0]);
        else {
          const e = k - a.shared;
          const r = 2.25 * Math.sqrt((e + 0.5) / a.experts) + 0.18;
          this.cellXZ.push([Math.cos(e * golden) * r, Math.sin(e * golden) * r]);
        }
      } else this.cellXZ.push([((k % DENSE_GRID) - (DENSE_GRID - 1) / 2) * 0.3, (Math.floor(k / DENSE_GRID) - (DENSE_GRID - 1) / 2) * 0.3]);
    }
    for (let l = 0; l < n; l++)
      for (let k = 0; k < perLayer; k++) {
        m.makeTranslation(this.cellXZ[k][0], this.layerY[l], this.cellXZ[k][1]);
        if (a.kind === 'moe' && k < a.shared) m.scale(new THREE.Vector3(2.2, 1, 2.2));
        this.cells.setMatrixAt(l * perLayer + k, m);
        this.cells.setColorAt(l * perLayer + k, DIM);
      }
    this.cells.instanceMatrix.needsUpdate = true;
    this.cellAct = new Float32Array(n * perLayer);
    this.activeCells.clear();
    this.tower.add(this.cells);
    this.patchGroup.visible = a.kind === 'dit';
    this.visible = 0;
  }

  // ---- per frame --------------------------------------------------------------------------------------------------
  private modeOf(vm: ViewModel): Mode {
    const s = vm.session;
    if (!s) return 'idle';
    if (s.phase === 'fault') return 'fault';
    if (s.phase === 'stopping') return 'stop';
    if (s.phase === 'starting' || s.phase === 'loading') return 'load';
    if (vm.vram.dormant) return 'sleep';
    if (s.llm?.activity === 'prefill') return 'prefill';
    if (s.llm?.activity === 'decode') return 'decode';
    if (s.image?.activity === 'generating') return 'denoise';
    return 'idle';
  }

  private frame(dt: number) {
    const vm = this.vm;
    if (!vm) return;
    this.t += dt;
    this.prevMode = this.mode;
    this.mode = this.modeOf(vm);
    const s = vm.session;
    const n = this.arch.layers;

    if (this.mode === 'prefill' && this.prevMode !== 'prefill') this.prefillStarts(vm);
    if (this.mode !== 'prefill') this.cacheHit = 0;

    const dz = vm.vram.dormant ?? null;
    const restored = dz ? clamp(vm.vram.usedGiB / Math.max(0.01, vm.vram.usedGiB + dz.pagedOutGiB)) : 1;
    if (this.mode === 'fault' && this.faultAt < 0) this.faultAt = this.t;
    if (this.mode !== 'fault') this.faultAt = -1;
    const targetGlow = this.mode === 'sleep' ? 0.6 : this.mode === 'fault' ? 0.25 : this.mode === 'idle' && !s ? 0.55 : 1;
    this.glow = approach(this.glow, targetGlow, 2, dt);
    const loadFrac = this.mode === 'load' ? (s?.loading?.fraction ?? 0) : 1;
    this.visible = approach(this.visible, loadFrac * n, this.mode === 'load' ? 3 : 1.5, dt);
    this.phosphor.decay = Math.exp(-dt / (this.mode === 'prefill' ? 0.2 : 0.32));
    this.crt.uniforms.time.value = this.t;

    this.spawn(dt, vm);
    this.direct(dt);

    const pools = [this.threads, this.heads, this.beamLines, this.arcLines, this.motesDots, this.sparkDots, this.fanLines];
    for (const p of pools) p.begin();
    this.advancePulses(dt);
    this.updateBeams(dt);
    this.updateArcs(dt);
    this.updateMotes(dt);
    this.updateSparks(dt);
    this.updateFan(dt);
    for (const p of pools) p.end();

    this.decayCells(dt);
    this.updateHelix(dt, vm);
    this.updateVram(dt, vm);
    this.updatePatches(dt, vm);
    this.paintTower(restored);
    this.paintDraft(dt);
    this.placeLabels();
    this.composer.render(dt);
  }

  /** A prompt-cache hit: the reused part of the context lights up at once. */
  private prefillStarts(vm: ViewModel) {
    const llm = vm.session?.llm;
    const pf = llm?.prefill;
    if (!llm || !pf) return;
    this.cacheHit = pf.cachedTokens ?? 0;
    const total = Math.max(1, llm.context.totalTokens);
    if (this.cacheHit > 0) {
      const upto = Math.min(HELIX_N, Math.floor((this.cacheHit / total) * HELIX_N));
      for (let k = 0; k < upto; k++) this.helixFlash[k] = 1 + (k / Math.max(1, upto)) * 0.4;
      this.helixUsed = Math.max(this.helixUsed, upto);
    }
    this.curtainY = -H / 2;
  }

  private spawn(dt: number, vm: ViewModel) {
    const s = vm.session;
    if (this.mode === 'decode' && s?.llm) {
      const tps = s.llm.decodeTps;
      this.shownTps = Math.min(tps, 6);
      this.spawnAcc += this.shownTps * dt;
      const accept = (s.llm.spec?.acceptancePct ?? 0) / 100;
      while (this.spawnAcc >= 1) {
        this.spawnAcc -= 1;
        if (this.draft.visible && this.r() < 0.4) {
          // speculative: the draft proposes three tokens, they fly to the model and are verified in one pass
          const from = DRAFT_POS.clone().add(new THREE.Vector3(0, 2.4, 0));
          for (let k = 0; k < 3; k++)
            this.motes.push({ t: -k * 0.06, speed: 1.9, from, to: new THREE.Vector3((k - 1) * 0.35, -H / 2 - 0.6, 0), lift: 1.2, kind: 'draft', ok: this.r() < accept, x: (k - 1) * 0.35 });
          this.draftAct = 1;
        } else this.pulses.push({ y: -H / 2 - 0.6, x: 0, z: 0, speed: H / 1.25, ghost: false, accept: true, lastLayer: -1, bright: 1.4 });
      }
    } else {
      this.shownTps = 0;
      this.spawnAcc = 0;
    }
    if (this.mode === 'prefill') {
      this.curtainAcc += dt;
      if (this.curtainAcc > 0.75) {
        this.curtainAcc = 0;
        for (let k = 0; k < 170; k++) {
          const x = (this.r() - 0.5) * SLAB * 0.92;
          const z = (this.r() - 0.5) * SLAB * 0.92;
          this.pulses.push({ y: -H / 2 - 0.6 - this.r() * 1.6, x, z, speed: H / 1.7, ghost: false, accept: false, lastLayer: -1, bright: 0.26 });
        }
      }
    }
  }

  /** The camera director: each event gets its shot; the orbit never stops. */
  private direct(dt: number) {
    const sinceEnd = this.t - this.reqEndAt;
    let shot: { dist: number; y: number; look: number; speed: number };
    if (this.mode === 'prefill') {
      // dolly along the rising curtain
      let sum = 0;
      let cnt = 0;
      for (const p of this.pulses)
        if (p.bright < 0.5) {
          sum += p.y;
          cnt++;
        }
      this.curtainY = approach(this.curtainY, clamp(cnt ? sum / cnt : this.curtainY, -H / 2, H / 2), 2, dt);
      shot = { dist: 19.5, y: this.curtainY - 0.5, look: this.curtainY + 1.2, speed: 0.035 };
    } else if (sinceEnd < 2.6 && this.mode !== 'load') {
      // a request ended: pull back to the top to see the write
      shot = { dist: 30, y: 8.5, look: 3.2, speed: 0.07 };
    } else if (this.mode === 'decode') shot = { dist: 25, y: 3 + Math.sin(this.t * 0.13) * 1.6, look: 0.6, speed: 0.085 };
    else if (this.mode === 'load') {
      const top = -H / 2 + (this.visible / Math.max(1, this.arch.layers)) * H;
      shot = { dist: 25, y: top + 3, look: top - 0.5, speed: 0.06 };
    } else if (this.mode === 'denoise') shot = { dist: 27, y: 4.2, look: 1.6, speed: 0.06 };
    else if (this.mode === 'sleep') shot = { dist: 31, y: 10, look: -0.5, speed: 0.015 };
    else shot = { dist: 32, y: 6 + Math.sin(this.t * 0.09) * 1.2, look: 0, speed: 0.04 };
    const c = this.cam;
    c.dist = approach(c.dist, shot.dist, 1.1, dt);
    c.y = approach(c.y, shot.y, 1.1, dt);
    c.look = approach(c.look, shot.look, 1.3, dt);
    c.speed = approach(c.speed, shot.speed, 1, dt);
    this.orbit += dt * c.speed;
    this.camera.position.set(Math.cos(this.orbit) * c.dist, c.y, Math.sin(this.orbit) * c.dist);
    this.camera.lookAt(1.2, c.look, 0);
  }

  private advancePulses(dt: number) {
    const n = this.arch.layers;
    const top = H / 2 + 0.5;
    const fault = this.mode === 'fault';
    const keep: Pulse[] = [];
    for (const p of this.pulses) {
      if (!fault) p.y += p.speed * dt;
      const layer = Math.floor(((p.y + H / 2) / H) * (n - 1) + 1e-6);
      if (layer > p.lastLayer && layer >= 0 && layer < n) {
        for (let l = Math.max(0, p.lastLayer + 1); l <= layer; l++) this.crossLayer(l, p);
        p.lastLayer = layer;
      }
      if (p.y > top) {
        if (p.bright >= 0.6) this.reachTop(p);
        continue;
      }
      keep.push(p);
      const c = p.ghost ? GHOST.clone().lerp(AMBER, 0.7) : p.bright > 1 ? WHITE : HOT;
      const b = p.bright * this.glow;
      this.heads.dot(p.x, p.y, p.z, c, b);
      if (p.bright > 1 || p.ghost) {
        // the residual stream: a thread painted from the embedding up to the token
        const from = -H / 2 - 0.6;
        this.threads.seg([p.x, p.y, p.z], [p.x, Math.max(from, p.y - 2.2), p.z], c, b * 0.8, c, b * 0.25);
        if (p.y - 2.2 > from) this.threads.seg([p.x, p.y - 2.2, p.z], [p.x, from, p.z], c, b * 0.25, c, b * 0.06);
      } else this.threads.seg([p.x, p.y, p.z], [p.x, p.y - 0.7, p.z], c, b * 0.7, c, 0);
    }
    this.pulses = keep;
  }

  private crossLayer(l: number, p: Pulse) {
    const a = this.arch;
    if (l >= this.visible) return;
    const curtain = p.bright < 0.5;
    const dense = a.kind !== 'moe';
    this.layerAct[l] = Math.min(1, this.layerAct[l] + (curtain ? 0.006 : p.ghost ? (dense ? 0.15 : 0.3) : dense ? 0.38 : 0.65));
    const base = l * this.cellsPerLayer;
    const y = this.layerY[l];
    if (a.kind === 'moe') {
      if (curtain) {
        if (this.r() < 0.25) this.fire(base + a.shared + Math.floor(this.r() * a.experts), 0.2);
      } else {
        // the router: the token forks to its experts and merges back above the layer
        const below = new THREE.Vector3(p.x, y - 0.18, p.z);
        const above = new THREE.Vector3(p.x, y + 0.18, p.z);
        // drawn on every other layer so the fork reads as a pattern, not a blaze
        const draw = l % 2 === 0;
        const route = (k: number) => {
          if (!draw) return;
          const [ex, ez] = this.cellXZ[k];
          const e = new THREE.Vector3(ex, y, ez);
          this.beams.push({ a: below, b: e, life: 1, k: p.ghost ? 0.22 : 0.45 });
          this.beams.push({ a: e, b: above, life: 1, k: p.ghost ? 0.12 : 0.22 });
        };
        for (let k = 0; k < a.shared; k++) {
          this.fire(base + k, 1.3);
          route(k);
        }
        for (let k = 0; k < a.active; k++) {
          const idx = a.shared + Math.floor(this.r() * a.experts);
          this.fire(base + idx, p.ghost ? 0.6 : 1.4);
          route(idx);
        }
      }
    } else if (!curtain || this.r() < 0.08) {
      for (let k = 0; k < this.cellsPerLayer; k++) if (this.r() < 0.08) this.fire(base + k, (curtain ? 0.04 : p.ghost ? 0.05 : 0.1) * (0.5 + this.r() * 0.5));
    }
    if (!curtain && this.helixUsed > 8 && l % Math.max(1, Math.round(a.layers / 7)) === 0) for (let k = 0; k < 2; k++) this.arc(p, l);
  }

  private fire(idx: number, v: number) {
    if (idx < 0 || idx >= this.cellAct.length) return;
    this.cellAct[idx] = Math.max(this.cellAct[idx], v);
    this.activeCells.add(idx);
  }

  private decayCells(dt: number) {
    const col = new THREE.Color();
    const k = Math.exp(-dt * (this.arch.kind === 'moe' ? 3.2 : 5));
    for (const idx of this.activeCells) {
      const v = this.cellAct[idx] * k;
      this.cellAct[idx] = v;
      if (v < 0.02) {
        this.cellAct[idx] = 0;
        this.activeCells.delete(idx);
        this.cells.setColorAt(idx, DIM);
        continue;
      }
      col.copy(DIM).lerp(v > 1 ? WHITE : AMBER, Math.min(1, v)).multiplyScalar((0.35 + v * 0.8) * this.glow);
      this.cells.setColorAt(idx, col);
    }
    if (this.cells.instanceColor) this.cells.instanceColor.needsUpdate = true;
    for (let l = 0; l < this.layerAct.length; l++) this.layerAct[l] *= Math.exp(-dt * 7);
  }

  private updateBeams(dt: number) {
    const keep: Beam[] = [];
    for (const b of this.beams) {
      b.life -= dt * 5.5;
      if (b.life <= 0) continue;
      keep.push(b);
      const k = b.life * b.k * this.glow;
      this.beamLines.seg(b.a, b.b, HOT, k, AMBER, k * 0.8);
    }
    this.beams = keep.length > 4800 ? keep.slice(-4800) : keep;
  }

  private arc(p: Pulse, l: number) {
    if (this.arcs.length >= 72) return;
    const used = Math.max(1, Math.floor(this.helixUsed));
    const r = this.r();
    const k = r < 0.6 ? used - 1 - Math.floor(this.r() * Math.min(used, 140)) : r < 0.8 ? Math.floor(this.r() * Math.min(used, 60)) : Math.floor(this.r() * used);
    const to = this.helixPos[clamp(k, 0, HELIX_N - 1)];
    const from = new THREE.Vector3(p.x, this.layerY[l], p.z);
    const mid = from.clone().lerp(to, 0.5).add(new THREE.Vector3(0, 0.8 + this.r() * 0.8, 0));
    this.arcs.push({ pts: new THREE.QuadraticBezierCurve3(from, mid, to).getPoints(19), life: 1 });
    this.helixFlash[clamp(k, 0, HELIX_N - 1)] = 1;
  }

  private updateArcs(dt: number) {
    const keep: Arc[] = [];
    for (const a of this.arcs) {
      a.life -= dt * 2.4;
      if (a.life <= 0) continue;
      keep.push(a);
      const k = a.life * 0.75 * this.glow;
      for (let i = 0; i < a.pts.length - 1; i++) this.arcLines.seg(a.pts[i], a.pts[i + 1], AMBER, k, AMBER, k);
    }
    this.arcs = keep;
  }

  private reachTop(p: Pulse) {
    // logits: a fresh distribution (peak in the middle), the top-k lit, one token sampled from it
    const z = Array.from({ length: FAN_N }, () => this.r() * 2.2 + (this.r() < 0.08 ? 2.5 : 0)).sort((x, y) => y - x);
    const e = z.map((v) => Math.exp(v / 0.7));
    const sum = e.reduce((x, y) => x + y, 0);
    const slotOf = (rank: number) => Math.floor(FAN_N / 2) + (rank % 2 === 0 ? rank / 2 : -(rank + 1) / 2);
    for (let rank = 0; rank < FAN_N; rank++) this.fanP[clamp(slotOf(rank), 0, FAN_N - 1)] = e[rank] / sum;
    this.fanTop = Array.from({ length: TOP_K }, (_, rank) => slotOf(rank));
    let roll = this.r() * e.slice(0, TOP_K).reduce((x, y) => x + y, 0);
    let rank = 0;
    for (; rank < TOP_K - 1; rank++) if ((roll -= e[rank]) <= 0) break;
    this.fanWinner = slotOf(rank);
    this.fanFlash = Math.max(this.fanFlash, p.ghost ? 0.7 : 1);
    if (!p.ghost || p.accept) {
      const used = Math.max(1, Math.floor(this.helixUsed));
      this.motes.push({ t: 0, speed: 1.6, from: this.fanPoint(this.fanWinner, 1), to: this.helixPos[clamp(used, 0, HELIX_N - 1)].clone(), lift: 1.6, kind: 'write', ok: true, x: 0 });
    } else this.burst(new THREE.Vector3(p.x, H / 2 + 0.6, p.z));
  }

  private fanPoint(i: number, h: number): THREE.Vector3 {
    const ang = (i / (FAN_N - 1)) * Math.PI * 1.1 - Math.PI * 0.55;
    const peak = Math.max(1e-6, ...this.fanP);
    const height = 0.08 + (this.fanP[i] / peak) * 1.9 * (0.3 + Math.min(1, this.fanFlash) * 0.7);
    return new THREE.Vector3(Math.sin(ang) * 2.1, H / 2 + 0.8 + h * height, Math.cos(ang) * 2.1 - 0.5);
  }

  private updateFan(dt: number) {
    this.fanFlash = Math.max(0, this.fanFlash - dt * 1.4);
    let prev: THREE.Vector3 | null = null;
    const curve = 0.35 * this.glow * (0.4 + this.fanFlash);
    for (let i = 0; i < FAN_N; i++) {
      const top = this.fanPoint(i, 1);
      const inTop = this.fanTop.includes(i);
      const win = i === this.fanWinner && this.fanFlash > 0.05;
      const c = win ? WHITE : inTop ? AMBER : GHOST;
      const k = (win ? 1.8 : inTop ? 0.5 + this.fanFlash * 0.5 : 0.22) * this.glow;
      this.fanLines.seg(this.fanPoint(i, 0), top, c, k * 0.4, c, k);
      if (prev) this.fanLines.seg(prev, top, HOT, curve, HOT, curve);
      prev = top;
    }
  }

  private updateMotes(dt: number) {
    const keep: Mote[] = [];
    for (const m of this.motes) {
      m.t += dt * m.speed;
      if (m.t < 0) {
        keep.push(m);
        continue;
      }
      if (m.t >= 1) {
        if (m.kind === 'write') this.helixFlash[clamp(Math.floor(this.helixUsed), 0, HELIX_N - 1)] = 1.6;
        else this.pulses.push({ y: -H / 2 - 0.6, x: m.x, z: 0, speed: H / 1.25, ghost: true, accept: m.ok, lastLayer: -1, bright: 0.7 });
        continue;
      }
      keep.push(m);
      const mid = m.from.clone().lerp(m.to, 0.5).add(new THREE.Vector3(0, m.lift, 0));
      const v = new THREE.QuadraticBezierCurve3(m.from, mid, m.to).getPoint(m.t);
      const c = m.kind === 'write' ? WHITE : GHOST.clone().lerp(AMBER, 0.7);
      this.motesDots.dot(v.x, v.y, v.z, c, (m.kind === 'write' ? 1.4 : 0.9) * this.glow);
    }
    this.motes = keep;
  }

  private burst(at: THREE.Vector3) {
    for (let i = 0; i < 18; i++) {
      const v = new THREE.Vector3(this.r() - 0.5, this.r() * 0.8 + 0.2, this.r() - 0.5).normalize().multiplyScalar(1.5 + this.r() * 2.5);
      this.sparks.push({ p: at.clone(), v, life: 0.6 + this.r() * 0.5 });
    }
  }

  private updateSparks(dt: number) {
    const keep: Spark[] = [];
    for (const s of this.sparks) {
      s.life -= dt;
      if (s.life <= 0) continue;
      s.v.y -= 6 * dt;
      s.p.addScaledVector(s.v, dt);
      keep.push(s);
      this.sparkDots.dot(s.p.x, s.p.y, s.p.z, EMBER, s.life * 1.6 * this.glow);
    }
    this.sparks = keep;
  }

  private updateHelix(dt: number, vm: ViewModel) {
    const llm = vm.session?.llm;
    const frac = llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0;
    const target = this.mode === 'fault' ? 0 : frac * HELIX_N;
    this.helixUsed = approach(this.helixUsed, target, this.mode === 'prefill' ? 3 : 1.5, dt);
    const col = this.helix.geometry.getAttribute('color') as THREE.BufferAttribute;
    const used = this.helixUsed;
    const sleep = this.mode === 'sleep';
    const c = new THREE.Color();
    for (let k = 0; k < HELIX_N; k++) {
      const f = this.helixFlash[k];
      if (f > 0) this.helixFlash[k] = Math.max(0, f - dt * 1.2);
      if (k < used) {
        const edge = used - k < 18 ? 1 - (used - k) / 18 : 0;
        c.copy(sleep ? GHOST : AMBER).multiplyScalar(0.32 + edge * 0.9 + f);
        if (f > 0.6) c.lerp(WHITE, 0.6);
      } else c.copy(DIM).multiplyScalar(0.25);
      c.multiplyScalar(this.glow);
      col.setXYZ(k, c.r, c.g, c.b);
    }
    col.needsUpdate = true;
  }

  private updateVram(dt: number, vm: ViewModel) {
    const total = Math.max(0.01, vm.vram.totalGiB);
    this.fillLevel = approach(this.fillLevel, clamp(vm.vram.usedGiB / total), 2, dt);
    this.fillRing.position.y = -H / 2 - 1.2 + (H + 2.4) * this.fillLevel;
    const spill = vm.vram.spillMiB > 0;
    const m = this.fillRing.material as THREE.LineBasicMaterial;
    m.color.copy(spill ? RED : this.mode === 'sleep' ? GHOST : AMBER);
    m.opacity = (spill ? 0.6 + 0.4 * Math.sin(this.t * 8) : 0.9) * this.glow;
    (this.cylinder.material as THREE.LineBasicMaterial).color.copy(spill ? RED.clone().multiplyScalar(0.5) : GHOST);
  }

  private updatePatches(dt: number, vm: ViewModel) {
    if (!this.patchGroup.visible) return;
    const right = new THREE.Vector3().setFromMatrixColumn(this.camera.matrixWorld, 0);
    this.patchGroup.position.set(0, 1.8, 0).addScaledVector(right, 7.4);
    this.patchGroup.quaternion.copy(this.camera.quaternion);
    const img = vm.session?.image;
    const k = img && img.steps > 0 ? img.step / img.steps : 0;
    const c = new THREE.Color();
    for (let j = 0; j < 16; j++)
      for (let i = 0; i < 24; i++) {
        const idx = j * 24 + i;
        if (this.r() < dt * (8 - 7 * k)) this.patchNoise[idx] = this.r();
        const u = i / 23;
        const v = j / 15;
        const sun = clamp(1 - Math.hypot(u - 0.65, v - 0.3) / 0.18);
        const ridge = v > 0.55 + Math.sin(u * 7) * 0.08 ? 0.45 : 0.12;
        const b = this.patchNoise[idx] * (1 - k) + Math.max(sun, ridge) * k;
        c.copy(DIM).lerp(b > 0.8 ? WHITE : AMBER, b).multiplyScalar(this.glow * (0.4 + b));
        this.patches.setColorAt(idx, c);
      }
    if (this.patches.instanceColor) this.patches.instanceColor.needsUpdate = true;
  }

  private paintTower(restored: number) {
    const n = this.arch.layers;
    const col = this.slabEdges.geometry.getAttribute('color') as THREE.BufferAttribute;
    const c = new THREE.Color();
    const fault = this.mode === 'fault';
    const flick = fault && this.t - this.faultAt < 1.2 ? (Math.sin(this.t * 60) > 0 ? 1 : 0.2) : 1;
    for (let l = 0; l < n; l++) {
      const built = l < this.visible ? 1 : l < this.visible + 1 ? this.visible - l : 0;
      const awake = this.mode === 'sleep' ? (l / n < restored ? 1 : 0) : 1;
      const act = this.layerAct[l];
      const base = this.mode === 'sleep' && !awake ? GHOST : fault ? RED : AMBER;
      c.copy(base).multiplyScalar(((this.mode === 'sleep' ? 0.42 : 0.16) + act * 0.85) * built * this.glow * flick);
      if (act > 0.55) c.lerp(WHITE, (act - 0.55) * 0.8);
      for (let v = 0; v < 8; v++) col.setXYZ(l * 8 + v, c.r, c.g, c.b);
      this.slabFill.setColorAt(l, c.clone().multiplyScalar(0.04 + act * 0.12));
    }
    col.needsUpdate = true;
    if (this.slabFill.instanceColor) this.slabFill.instanceColor.needsUpdate = true;
    this.cells.visible = this.visible > 0.5;
  }

  private paintDraft(dt: number) {
    if (!this.draft.visible) return;
    this.draftAct = Math.max(0, this.draftAct - dt * 2);
    (this.draftEdges.material as THREE.LineBasicMaterial).color.copy(GHOST).lerp(HOT, this.draftAct).multiplyScalar(0.7 + this.draftAct);
  }

  /** Screen-space labels that follow the scene, through the same curvature as the CRT pass. */
  private placeLabels() {
    const w = this.size.x;
    const h = this.size.y;
    const put = (id: string, v: THREE.Vector3, show = true) => {
      const el = this.labels[id];
      if (!el) return;
      const p = v.clone().project(this.camera);
      // inverse of the barrel: the screen point whose distorted lookup lands on p
      let x = p.x;
      let y = p.y;
      for (let i = 0; i < 4; i++) {
        const f = 1 + CRT_CURVE * (x * x + y * y);
        x = p.x / f;
        y = p.y / f;
      }
      el.style.transform = `translate(${((x + 1) / 2) * w}px, ${((1 - y) / 2) * h}px)`;
      el.style.opacity = show && p.z < 1 ? '1' : '0';
    };
    const n = this.arch.layers;
    if (this.patchGroup.visible) put('top', this.patchGroup.position.clone().add(new THREE.Vector3(0, 2.4, 0)));
    else put('top', new THREE.Vector3(0, H / 2 + 2.9, 0));
    put('bottom', new THREE.Vector3(SLAB / 2, -H / 2, SLAB / 2));
    put('layer', new THREE.Vector3(SLAB / 2, this.layerY[Math.floor(n / 2)] ?? 0, -SLAB / 2));
    put('kv', this.helixPos[clamp(Math.floor(this.helixUsed), 0, HELIX_N - 1)]);
    put('draft', DRAFT_POS.clone().add(new THREE.Vector3(0, 2.8, 0)), this.draft.visible);
    let best = new THREE.Vector3();
    let bx = -Infinity;
    for (let i = 0; i < 16; i++) {
      const a = (i / 16) * Math.PI * 2;
      const v = new THREE.Vector3(Math.cos(a) * CYL_R, this.fillRing.position.y, Math.sin(a) * CYL_R);
      const sx = v.clone().project(this.camera).x;
      if (sx > bx) {
        bx = sx;
        best = v;
      }
    }
    put('vram', best);
  }
}
