// KLIF fx "Rings" fragment shader (MIT, part of KLIF).
//
// The algorithm is tdhooper's "Bubble Rings" (shadertoy.com/view/WdB3Dw): a thick-walled Clifford torus in the
// 3-sphere, turned by a double rotation in R⁴, brought into R³ by stereographic projection, with its distance
// corrected back to R³ units and cut by a ball; drawn as frosted glass by a glow march that is deliberately too
// short to converge, coloured by depth with a cosine palette and finished with a three-stage tone curve. The
// smooth max is Mercury's hg_sdf (MIT). This file is KLIF's own implementation, with KLIF's signals built in;
// the 4D turn and placement come from JS.
//
// At rest (ball 1.85, wall .2, beads 0, glow 1, core 1, gain 1, tint 1, saturation 1, no placement) it draws the
// reference look (core #4CFF99, haze #9841B3, palette range 0.7 / 0.7).
export const RINGS_FRAG = `#version 300 es
precision highp float;

uniform vec2 uRes;          // drawing buffer, px
uniform vec2 uPlace;        // screen offset of the sphere, in half screen heights (2·shift, -2·lift)
uniform float uZoom;        // picture scale around the placed centre (1 = reference)
uniform mat4 uTurn;         // this frame's double rotation in R⁴ (built in JS)
uniform float uBall;        // radius of the cutting ball (1.85)
uniform float uWall;        // half thickness of the torus wall (0.2)
uniform float uBeadDepth;   // decode: depth of the bulges travelling along the rings
uniform float uBeadPhase;
uniform float uGlow;        // gain of the haze gathered along the ray (1)
uniform float uCore;        // gain of the light right at the surfaces (1)
uniform vec3 uCoreColor;
uniform vec3 uHazeColor;
uniform vec2 uHue;          // depth palette: (top, bottom)
uniform float uGain;
uniform vec3 uTint;
uniform float uSat;

out vec4 outColor;

const float PI = 3.14159265359;
const vec3 EYE = vec3(1.8, 5.5, -5.5) * 1.75;
const vec3 UP = vec3(-1.0, 0.0, -1.5);
const float FOCAL = 5.0;
const int STEPS = 82;         // too few to pass through the whole body: the frosted look
const float STRIDE = 0.8;     // under-relaxed steps gather more glow
const float MIN_STEP = 0.001;
const float FAR = 20.0;
const vec3 LUMA = vec3(0.299, 0.587, 0.114);

// Smooth maximum, hg_sdf (Mercury, MIT).
float smoothMax(float a, float b, float r) {
  vec2 u = max(vec2(r + a, r + b), vec2(0.0));
  return min(-r, max(a, b)) + length(u);
}

float scene(vec3 p) {
  // Lift onto S³ (inverse stereographic projection); k is the projection's local scale.
  float k = 2.0 / (1.0 + dot(p, p));
  vec4 s = vec4(k * p, k - 1.0);
  // Beads are placed in the unturned frame so they never jump when the loop wraps.
  float bead = uBeadDepth * sin(atan(s.y, s.x) * 4.0 - uBeadPhase * 6.2831853);
  s = uTurn * s;
  // Clifford torus |z1| = |z2|: the ratio of the two plane radii, folded to the side that is below one.
  float a = length(s.xy);
  float b = length(s.zw);
  float t = a / b - 1.0;
  float torus = (t < 0.0 ? -t : b / a - 1.0) / PI;
  float d = abs(torus) - (uWall + bead);
  // Back to R³ distances (tdhooper's fitted correction for the projection).
  float sgn = sign(d);
  d = (pow(abs(d) / k * 1.82 + 1.0, 0.5) - 1.0) * (5.0 / 3.0) * sgn;
  return smoothMax(d, length(p) - uBall, 0.2);
}

void main() {
  vec2 p = (2.0 * gl_FragCoord.xy - uRes) / uRes.y;
  p = (p + uPlace) / uZoom;
  vec3 fwd = normalize(-EYE);
  vec3 side = normalize(cross(fwd, UP));
  vec3 up = normalize(cross(side, fwd));
  vec3 dir = normalize(mat3(side, up, fwd) * vec3(p, FOCAL));

  vec3 core = uCoreColor + 1.1;
  vec3 col = vec3(0.0);
  vec3 pos = EYE;
  float travelled = 0.0;
  float dist = 0.0;
  for (int i = 0; i < STEPS; i++) {
    travelled += max(MIN_STEP, abs(dist) * STRIDE);
    pos = EYE + dir * travelled;
    dist = scene(pos);
    vec3 light = vec3(max(0.0, 0.01 - abs(dist)) * 0.5 * uCore) * core;
    light += uHazeColor * STRIDE / 160.0 * uGlow;
    light *= smoothstep(20.0, 7.0, length(pos));
    float fade = smoothstep(FAR, 0.1, travelled);
    light *= fade;
    light *= 0.5 + 0.5 * cos(6.28318 * (fade * uHue.x - uHue.y + vec3(0.0, 0.33, 0.67)));
    col += light;
    if (travelled > FAR) break;
  }
  col = pow(col, vec3(1.0 / 1.8)) * 2.0;
  col = pow(col, vec3(2.0)) * 3.0;
  col = pow(col, vec3(1.0 / 2.2));

  col *= uGain * uTint;
  col = mix(vec3(dot(col, LUMA)), col, uSat);
  outColor = vec4(col, 1.0);
}
`;
