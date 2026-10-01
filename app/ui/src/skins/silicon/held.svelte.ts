// A value that follows its source but changes at most once per `ms` (so digits never re-roll
// faster than ~2x per second, whatever the telemetry rate is). Call during component init.
export function held<T>(get: () => T, ms = 500): { readonly current: T } {
  let value = $state(get());
  let last = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const next = get();
    const now = performance.now();
    clearTimeout(timer);
    if (now - last >= ms) {
      value = next;
      last = now;
    } else {
      timer = setTimeout(() => {
        value = get();
        last = performance.now();
      }, ms - (now - last));
    }
    return () => clearTimeout(timer);
  });

  return {
    get current() {
      return value;
    },
  };
}
