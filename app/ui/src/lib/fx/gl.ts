// KLIF fx: one full-screen fragment shader on a WebGL2 canvas. Shared by the animated backgrounds.

export class FxQuad {
  readonly gl: WebGL2RenderingContext;
  private prog: WebGLProgram;
  private loc = new Map<string, WebGLUniformLocation | null>();

  constructor(readonly canvas: HTMLCanvasElement, fragment: string) {
    const gl = canvas.getContext('webgl2', { antialias: false, alpha: false, depth: false, preserveDrawingBuffer: false });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? 'shader compile failed');
      return s;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, compile(gl.VERTEX_SHADER, '#version 300 es\nvoid main(){vec2 v=vec2(gl_VertexID&1,gl_VertexID>>1)*4.0-1.0;gl_Position=vec4(v,0.0,1.0);}'));
    gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, fragment));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog) ?? 'link failed');
    this.prog = prog;
    gl.useProgram(prog);
  }

  private at(name: string) {
    if (!this.loc.has(name)) this.loc.set(name, this.gl.getUniformLocation(this.prog, name));
    return this.loc.get(name) ?? null;
  }

  f(name: string, v: number) {
    this.gl.uniform1f(this.at(name), v);
  }

  v2(name: string, x: number, y: number) {
    this.gl.uniform2f(this.at(name), x, y);
  }

  v3(name: string, v: ArrayLike<number>) {
    this.gl.uniform3f(this.at(name), v[0], v[1], v[2]);
  }

  /** Column-major, as GLSL expects. */
  m3(name: string, m: Float32Array) {
    this.gl.uniformMatrix3fv(this.at(name), false, m);
  }

  m4(name: string, m: Float32Array) {
    this.gl.uniformMatrix4fv(this.at(name), false, m);
  }

  /** One full-screen triangle into the canvas's current size. */
  draw() {
    const gl = this.gl;
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    this.v2('uRes', this.canvas.width, this.canvas.height);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  dispose() {
    this.gl.getExtension('WEBGL_lose_context')?.loseContext();
  }
}

/** Size a canvas to CSS w x h at a device-pixel ratio capped at 1 (soft glows), times a quality scale. */
export function fitCanvas(canvas: HTMLCanvasElement, w: number, h: number, quality = 1) {
  const s = Math.min(window.devicePixelRatio || 1, 1) * Math.max(0.25, Math.min(1, quality));
  canvas.width = Math.max(1, Math.round(w * s));
  canvas.height = Math.max(1, Math.round(h * s));
}

export const hexRgb = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
export const mix = (a: number, b: number, t: number) => a + (b - a) * t;
export const mix3 = (a: number[], b: number[], t: number) => a.map((v, i) => v + (b[i] - v) * t);
