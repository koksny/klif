// Shared number formatting so every skin prints the same value the same way.

const THIN = ' ';

/** 19716 -> "19 716" (thin-space thousands separator). */
export function fmtInt(n: number): string {
  const s = Math.round(n).toString();
  return s.replace(/\B(?=(\d{3})+(?!\d))/g, THIN);
}

/** Fixed decimals without locale surprises: fmtFixed(15.517, 2) -> "15.52". */
export function fmtFixed(n: number, digits: number): string {
  return n.toFixed(digits);
}

/** 47.31 -> "47.3" */
export function fmtTps(n: number): string {
  return n >= 100 ? Math.round(n).toString() : n.toFixed(1);
}

/** 15.517 -> "15.52" */
export function fmtGiB(n: number): string {
  return n.toFixed(2);
}

/** 98304 -> "96k", 131072 -> "128k", 16384 -> "16k" */
export function fmtCtx(tokens: number): string {
  return `${Math.round(tokens / 1024)}k`;
}

/** 8047 -> "02:14:07" */
export function fmtClock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  return [h, m, r].map((v) => v.toString().padStart(2, '0')).join(':');
}

/** 22.71 -> "22.7 s", 184 -> "3 min 4 s" */
export function fmtSeconds(seconds: number): string {
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${m} min ${s} s`;
}

/** 0.2006 -> "20%" */
export function fmtPct(fraction01: number): string {
  return `${Math.round(fraction01 * 100)}%`;
}

/** A tier's label for tight places: "SYSTEM 2" -> "S2", "SYSTEM CGI" -> "CGI" (an older "AGENT MEDIUM" -> "MEDIUM"). */
export function tierShort(label: string): string {
  const n = /^SYSTEM\s+(\d+)$/i.exec(label.trim());
  if (n) return `S${n[1]}`;
  return label.replace(/^(SYSTEM|AGENT)\s+/i, '');
}
