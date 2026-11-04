// Fuzzy search for the launcher: how well a query matches a name, and the ranking of every
// candidate (apps, settings pages, files) with the user's own history on top. Pure, so it's tested.

/** Lowercase, without accents: "Café" and "cafe" match. */
export function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

const WORD = /[\p{L}\p{N}]+/gu;

/** The words of a name, for word-prefix and initials matches ("Visual Studio Code" → visual, studio, code). */
function words(text: string): string[] {
  return text.match(WORD) ?? [];
}

/**
 * How well one query word matches `text` (both folded): higher is better, -1 is no match.
 * Exact > prefix > a word's prefix > initials ("vsc") > substring > in-order letters (3 or more).
 */
function wordScore(query: string, text: string): number {
  if (!query) return 0;
  if (text === query) return 1000;
  if (text.startsWith(query)) return 900 - Math.min(100, text.length - query.length);
  const parts = words(text);
  if (parts.some((w) => w.startsWith(query))) return 800 - Math.min(80, parts.findIndex((w) => w.startsWith(query)) * 10);
  if (query.length >= 2 && parts.map((w) => w[0]).join("").startsWith(query)) return 750;
  const at = text.indexOf(query);
  if (at >= 0) return 600 - Math.min(100, at);
  // In-order letters (from 3 typed): reward runs and letters at word starts, punish gaps.
  if (query.length < 3) return -1;
  let score = 300;
  let from = 0;
  let last = -2;
  for (const ch of query) {
    const found = text.indexOf(ch, from);
    if (found < 0) return -1;
    if (found === last + 1) score += 15;
    else score -= Math.min(20, found - from);
    if (found === 0 || !/[\p{L}\p{N}]/u.test(text[found - 1] ?? "")) score += 10;
    last = found;
    from = found + 1;
  }
  return Math.max(1, score);
}

/** How well `query` matches `text`: every word of the query must match; -1 when one doesn't. */
export function fuzzyScore(query: string, text: string): number {
  const q = fold(query).trim();
  if (!q) return 0;
  const t = fold(text);
  const tokens = q.split(/\s+/);
  if (tokens.length === 1) return wordScore(q, t);
  // The whole phrase as one word first ("visual studio" in "Visual Studio Code").
  const whole = wordScore(q, t);
  let worst = Infinity;
  for (const token of tokens) {
    const score = wordScore(token, t);
    if (score < 0) return whole;
    worst = Math.min(worst, score);
  }
  return Math.max(whole, worst);
}

export interface Candidate {
  /** What the history knows it by. */
  id: string;
  name: string;
  keywords?: readonly string[];
  /** Apps come before settings, settings before files, at equal scores. */
  kind: "app" | "setting" | "file";
}

export interface History {
  id: string;
  count: number;
  last: number;
}

const KIND_BONUS: Record<Candidate["kind"], number> = { app: 20, setting: 0, file: -20 };
const DAY = 86_400_000;

/** A bonus for what the user runs often and lately: up to ~180 points. */
export function frecency(entry: History | undefined, now: number): number {
  if (!entry) return 0;
  const often = Math.min(120, 30 * Math.log2(1 + entry.count));
  const age = now - entry.last;
  const lately = age < DAY ? 60 : age < 7 * DAY ? 30 : age < 30 * DAY ? 10 : 0;
  return often + lately;
}

/** The candidates that match `query`, best first. Keywords count a little less than the name. */
export function rank<C extends Candidate>(query: string, candidates: readonly C[], history: readonly History[], now: number): C[] {
  const seen = new Map(history.map((h) => [h.id, h]));
  const scored: { c: C; score: number }[] = [];
  for (const c of candidates) {
    let best = fuzzyScore(query, c.name);
    for (const keyword of c.keywords ?? []) {
      const s = fuzzyScore(query, keyword);
      if (s > 0) best = Math.max(best, s * 0.85);
    }
    if (best < 0) continue;
    scored.push({ c, score: best + KIND_BONUS[c.kind] + frecency(seen.get(c.id), now) });
  }
  scored.sort((a, b) => b.score - a.score || a.c.name.localeCompare(b.c.name));
  return scored.map((s) => s.c);
}
