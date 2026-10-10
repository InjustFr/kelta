// Permission / elicitation menu detection for the peek popover (ticket #139). The only place that knows
// Claude's menu layout: a numbered list at the bottom of the screen (`❯ 1. Yes`, `  2. No, ...`).
// Version-fragile on purpose: callers gate it on `needs_input` and fall back to the reply field on null.

export interface MenuOption {
  /** The digit Claude takes to pick it. */
  key: string;
  label: string;
}

// Box borders, the selection cursor, then `N.` or `N)` and the label.
const OPTION = /^[\s│┃|]*(?:[❯›>▶]\s*)?([1-9])[.)]\s+(.+?)[\s│┃|]*$/;
/** The menu must end this close to the bottom (footer hint, box border) or it is text Claude wrote. */
const FOOTER_LINES = 4;

/** The menu options at the bottom of `tail`, or null when there is no menu there. */
export function detectMenu(tail: string): MenuOption[] | null {
  const lines = tail.split('\n');
  while (lines.length && !lines[lines.length - 1]!.trim()) lines.pop();
  const found: { n: number; label: string; at: number }[] = [];
  lines.forEach((line, at) => {
    const m = OPTION.exec(line);
    if (m) found.push({ n: Number(m[1]), label: m[2]!, at });
  });
  const last = found[found.length - 1];
  if (!last || lines.length - 1 - last.at > FOOTER_LINES) return null;
  // Walk back n, n-1, ..., 1: the last run is the menu, an older numbered list above it is not.
  const run = [last];
  for (let i = found.length - 2; i >= 0 && run[0]!.n > 1; i--)
    if (found[i]!.n === run[0]!.n - 1) run.unshift(found[i]!);
  if (run[0]!.n !== 1 || run.length < 2) return null;
  return run.map((o) => ({ key: String(o.n), label: o.label }));
}
