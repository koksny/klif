// KLIF fx "Ether" fragment shader (MIT, part of KLIF).
//
// The algorithm is nimitz's "Ether" (shadertoy.com/view/MsjSW3): a sphere-like distance field, scaled by a slow
// logarithmic term and warped by three nested sines of a drifting coordinate, seen through a handful of coarse,
// non-converging march steps; each step tints the light with a directional difference of the field (a cheap
// "light from one side") and compounds it with what came before, which gives the glowing nested shells. This file
// is KLIF's own implementation of that method, with KLIF's signals built in.
//
// At rest (shells 5, warp .5, radius 1, form 0, ripple 0, grain 0, gain 1, tint 1, saturation 1) it draws the
// reference look.
export const ETHER_FRAG = `#version 300 es
precision highp float;

uniform vec2 uRes;        // drawing buffer, px
uniform vec2 uCenter;     // the cloud's centre, in screen heights from the bottom-left (0.9, 0.5 = reference)
uniform mat3 uSpin;       // this frame's tumble (built in JS)
uniform float uTime;      // the cloud's own clock (runs faster or stops with the session)
uniform float uWobble;    // sideways drift of the body, sin(0.7 t), from JS
uniform float uShells;    // march steps after the first (5 = all six)
uniform float uWarp;      // depth of the nested-sine warp (0.5)
uniform float uRadius;    // body size (1)
uniform float uForm;      // > 0 thins the cloud towards nothing (loading)
uniform float uRipple;    // decode: depth of the travelling surface echoes
uniform float uRipplePhase;
uniform float uGrain;     // Krea: depth of the fine surface noise
uniform float uGain;
uniform vec3 uTint;
uniform float uSat;

out vec4 outColor;

const vec3 EYE = vec3(0.0, 0.0, 5.0);
const vec3 SHADE = vec3(0.1, 0.3, 0.4);     // light in the shadowed direction: deep teal
const vec3 HEAT = vec3(5.0, 2.5, 3.0);      // added per unit of facing: hot magenta to white
const vec3 LUMA = vec3(0.299, 0.587, 0.114);

float field(vec3 p) {
  p = uSpin * p;
  vec3 drift = 2.0 * p + uTime;
  float body = length(p + uWobble) * log(length(p) + 1.0);
  float warp = sin(drift.x + sin(drift.z + sin(drift.y)));
  float ripple = uRipple * sin(length(p) * 7.0 - uRipplePhase * 6.2831853);
  float grain = uGrain * sin(p.x * 9.0 + uTime * 2.0) * sin(p.y * 8.0 - uTime * 1.7) * sin(p.z * 10.0 + uTime * 2.3);
  return body + warp * uWarp - uRadius + ripple + grain;
}

void main() {
  vec3 dir = normalize(vec3(gl_FragCoord.xy / uRes.y - uCenter, -1.0));
  int shells = int(uShells);
  vec3 col = vec3(0.0);
  float depth = 5.0;
  for (int i = 0; i <= 5; i++) {
    if (i > shells) break;
    vec3 p = EYE + dir * depth;
    float here = field(p);
    float near = here + uForm;
    float facing = clamp((here - field(p + 0.1)) * 0.5, -0.1, 1.0);
    vec3 light = SHADE + HEAT * facing;
    col = col * light + smoothstep(2.5, 0.0, near) * 0.7 * light;
    depth += min(near, 1.0);
  }
  col *= uGain * uTint;
  col = mix(vec3(dot(col, LUMA)), col, uSat);
  outColor = vec4(col, 1.0);
}
`;
