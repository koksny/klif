// Console ring buffer plus the line formats the real servers print (llama.cpp and stable-diffusion.cpp).

const MAX_LINES = 200;

export class ConsoleBuf {
  lines: string[] = [];

  push(line: string) {
    this.lines.push(line);
    if (this.lines.length > MAX_LINES) this.lines.splice(0, this.lines.length - MAX_LINES);
  }

  pushAll(lines: string[]) {
    for (const l of lines) this.push(l);
  }

  clear() {
    this.lines = [];
  }

  snapshot(): string[] {
    return this.lines.slice();
  }
}

const pad = (v: number, n: number) => Math.floor(v).toString().padStart(n, '0');

/** llama.cpp log timestamp "M.SS.mmm.uuu" (minutes, seconds, milliseconds, microseconds since process start). */
export function llamaTs(sec: number): string {
  const s = Math.max(0, sec);
  const m = Math.floor(s / 60);
  const rest = s - m * 60;
  const whole = Math.floor(rest);
  const ms = Math.floor((rest - whole) * 1000);
  const us = Math.floor(((rest - whole) * 1000 - ms) * 1000);
  return `${m}.${pad(whole, 2)}.${pad(ms, 3)}.${pad(us, 3)}`;
}

/** "I slot print_timing: id  0 | task 7192 | ..." */
export function llamaLine(sec: number, level: 'I' | 'W' | 'E', msg: string): string {
  return `${llamaTs(sec)} ${level} ${msg}`;
}

export const padNum = (v: number, width: number, digits = 0) => v.toFixed(digits).padStart(width, ' ');

/** sd.cpp style progress bar line: "  |======>      | 3/8 - 1.16it/s" */
export function sdBar(step: number, steps: number, stepS: number): string {
  const width = 50;
  const filled = Math.round((step / steps) * width);
  const bar = step >= steps ? '='.repeat(width) : '='.repeat(Math.max(0, filled - 1)) + '>' + ' '.repeat(Math.max(0, width - filled));
  const rate = step === 1 || stepS >= 1 ? `${stepS.toFixed(2)}s/it` : `${(1 / stepS).toFixed(2)}it/s`;
  return `  |${bar}| ${step}/${steps} - ${rate}`;
}
