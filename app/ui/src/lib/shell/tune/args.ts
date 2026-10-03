// Display grouping of argument tokens: a flag and the value after it share one row. Grouping is for display
// only: the tokens are never split, merged or re-tokenized (one token = one argv entry, exactly as typed).

/** `{p.NAME}` expands to 0..n whole tokens (a param's args): never the value of the flag before it. */
const WHOLE_PARAM = /^\{p\.[^.{}]+\}$/;

/** A flag: starts with "-" and is not a number ("-1", "-0.5" and "5" are values: `^-?\d`). */
export function isFlagToken(t: string): boolean {
  return t.length > 1 && t.startsWith('-') && !/^-?\d/.test(t);
}

function isValueToken(t: string): boolean {
  return !isFlagToken(t) && !WHOLE_PARAM.test(t);
}

/** Row lengths (1 or 2) partitioning `tokens`: a flag without "=" followed by a value token makes a row of 2. */
export function groupRows(tokens: readonly string[]): number[] {
  const rows: number[] = [];
  let i = 0;
  while (i < tokens.length) {
    const t = tokens[i];
    if (isFlagToken(t) && !t.includes('=') && i + 1 < tokens.length && isValueToken(tokens[i + 1])) {
      rows.push(2);
      i += 2;
    } else {
      rows.push(1);
      i += 1;
    }
  }
  return rows;
}

/** Start index of every row. */
export function rowStarts(rows: readonly number[]): number[] {
  const out: number[] = [];
  let at = 0;
  for (const n of rows) {
    out.push(at);
    at += n;
  }
  return out;
}

/** Raw text (one token per line) -> tokens. Trailing empty lines are not tokens; inner empty lines are "" tokens. */
export function tokensFromLines(text: string): string[] {
  const lines = text.split(/\r?\n/);
  while (lines.length && lines[lines.length - 1] === '') lines.pop();
  return lines;
}
