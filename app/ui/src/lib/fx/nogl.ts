// What a WebGL skin says when the window cannot draw it (NoGl.svelte shows it).

/** The browser's sentence worth showing next to "needs WebGL": none when it only says that there is no context. */
export function glDetail(e: unknown): string {
  const m = (e instanceof Error ? e.message : String(e)).trim();
  return /WebGL2? is not available|creating WebGL context/i.test(m) ? '' : m;
}
