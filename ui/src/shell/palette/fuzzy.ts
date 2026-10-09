// Fuzzy matching for the command palette and the project switcher: every whitespace-separated token
// of the query must match as a subsequence of the text; contiguous runs, word starts and matches
// near the beginning score higher.

/** Score of one token in `text` (both lower-case), or null when it is not a subsequence. */
function tokenScore(token: string, text: string): number | null {
  const sub = text.indexOf(token);
  if (sub >= 0) {
    // Contiguous substring: strongest signal, best at a word start or at the start of the text.
    const boundary = sub === 0 || !/[a-z0-9]/.test(text[sub - 1] ?? ' ');
    return 100 + (sub === 0 ? 40 : boundary ? 25 : 0) - Math.min(sub, 30);
  }
  let score = 0;
  let from = 0;
  let prev = -2;
  for (const ch of token) {
    const at = text.indexOf(ch, from);
    if (at < 0) return null;
    const boundary = at === 0 || !/[a-z0-9]/.test(text[at - 1] ?? ' ');
    score += 1 + (at === prev + 1 ? 6 : 0) + (boundary ? 8 : 0) - Math.min(at - from, 5) * 0.5;
    prev = at;
    from = at + 1;
  }
  return score;
}

/** Higher is better; null = no match. An empty query matches everything with score 0. */
export function fuzzyScore(query: string, text: string): number | null {
  const tokens = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return 0;
  const hay = text.toLowerCase();
  let total = 0;
  for (const token of tokens) {
    const s = tokenScore(token, hay);
    if (s === null) return null;
    total += s;
  }
  return total;
}

/** Matches and sorts `items` by score (stable for equal scores), at most `limit` entries. */
export function rank<T>(items: readonly T[], query: string, text: (item: T) => string, limit = 60): T[] {
  if (query.trim() === '') return items.slice(0, limit);
  const scored: { item: T; score: number; index: number }[] = [];
  items.forEach((item, index) => {
    const score = fuzzyScore(query, text(item));
    if (score !== null) scored.push({ item, score, index });
  });
  scored.sort((a, b) => b.score - a.score || a.index - b.index);
  return scored.slice(0, limit).map((s) => s.item);
}
