// Settings search (#469): one index of everything the pane can change (section names, schema keys incl.
// TOML-only ones, rows sections draw themselves, keymap actions) and the ranking: label, keywords,
// group and section, help, key; Advanced last.
import type { SettingsSection, SettingsSectionId } from '$app/registry';
import { ACTIONS } from '$lib/gen/actions';
import { t } from '$lib/i18n/index.svelte';

import { fuzzyScore } from '../../../shell/palette/fuzzy';
import { COPY, onThisPlatform } from '../copy';

import {
  enumOptions,
  helpOf,
  keywordsOf,
  labelOf,
  leafFields,
  type FieldKind,
  type SchemaNode,
} from './schema';
import {
  SECTION_BLURB,
  SECTION_KEYWORDS,
  SECTION_ROOTS,
  SECTION_ROWS,
  TOML_ROOTS,
  sectionLabel,
  underAdvanced,
} from './sections';

export interface SearchEntry {
  id: string; // section:<id> | key:<path> | row:<anchor or section id> | action:<id>
  kind: 'section' | 'key' | 'row' | 'action';
  section: SettingsSectionId;
  label: string;
  /** false: the label is the key's name (no copy label yet), ranked like the group and section */
  realLabel: boolean;
  keywords: string[];
  crumb: string; // "Terminal" or "Claude › Profiles"
  help: string;
  options?: string[]; // enum option labels, matched like help
  path?: string; // settings key, or action id
  node?: SchemaNode;
  field?: FieldKind;
  toml?: boolean;
  advanced?: boolean;
  anchor?: string;
  chords?: string[];
}
export interface SearchHit extends SearchEntry {
  score: number;
  why: string;
}

const INLINE: readonly FieldKind[] = ['bool', 'enum', 'int', 'float', 'string', 'secret'];
/** Simple values are edited in the results; lists, maps and TOML-only keys show one line with Open. */
export const isInline = (e: SearchEntry): boolean => e.kind === 'key' && !e.toml && INLINE.includes(e.field!);

export function buildSearchIndex(
  schema: SchemaNode,
  sections: readonly SettingsSection[],
  bindings: Record<string, readonly string[]>,
): SearchEntry[] {
  const out: SearchEntry[] = [];
  for (const s of sections) {
    const name = sectionLabel(s);
    const crumb = (group?: string): string => (group ? `${name} › ${t(group)}` : name);
    out.push({
      id: `section:${s.id}`,
      kind: 'section',
      section: s.id,
      label: name,
      realLabel: true,
      keywords: (SECTION_KEYWORDS[s.id] ?? []).map((k) => t(k)),
      crumb: name,
      help: t(SECTION_BLURB[s.id] ?? ''),
    });
    const keys = (roots: readonly string[], toml: boolean): SearchEntry[] =>
      leafFields(
        schema,
        roots.filter((r) => onThisPlatform(COPY[r])),
      ).map((f) => ({
        id: `key:${f.path}`,
        kind: 'key',
        section: s.id,
        label: labelOf(f.path),
        realLabel: !!COPY[f.path]?.label,
        keywords: keywordsOf(f.path),
        crumb: crumb(COPY[f.path]?.group),
        help: helpOf(f.path, f.node),
        options: enumOptions(f.node, f.path).map((o) => o.label),
        path: f.path,
        node: f.node,
        field: f.kind,
        toml,
        advanced: underAdvanced(f.path),
      }));
    out.push(...keys(SECTION_ROOTS[s.id] ?? [], false), ...keys(TOML_ROOTS[s.id] ?? [], true));
    for (const r of SECTION_ROWS[s.id] ?? [])
      out.push({
        id: `row:${r.anchor ?? s.id}`,
        kind: 'row',
        section: s.id,
        label: t(r.label),
        realLabel: true,
        keywords: (r.keywords ?? []).map((k) => t(k)),
        crumb: crumb(r.group),
        help: '',
        anchor: r.anchor,
      });
  }
  const keysSection = sections.find((s) => s.id === 'keys');
  if (keysSection)
    for (const a of ACTIONS)
      out.push({
        id: `action:${a.id}`,
        kind: 'action',
        section: 'keys',
        label: t(a.label),
        realLabel: true,
        keywords: [],
        crumb: sectionLabel(keysSection),
        help: '',
        path: a.id,
        chords: [...(bindings[a.id] ?? [])],
      });
  return out;
}

const KIND_ORDER = { section: 0, key: 1, row: 2, action: 3 } as const;
const words = (s: string): string[] =>
  s
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);

function score(e: SearchEntry, qs: string[], phrase: string): { score: number; why: string } | null {
  const lw = e.realLabel ? words(e.label) : [];
  const kws = e.keywords.map((k) => k.toLowerCase());
  const sw = words(`${e.crumb} ${e.realLabel ? '' : e.label}`);
  const hw = words(`${e.help} ${(e.options ?? []).join(' ')}`);
  const pw = words(`${e.path ?? ''} ${(e.chords ?? []).join(' ')}`);
  let total = 0;
  let top = 0;
  let kwHit = '';
  let labelHit = false;
  let fuzzy = false;
  for (const w of qs) {
    // one- and two-letter words match whole words only: "pr" is not "prefix"
    const pre = (x: string): boolean => x === w || (w.length > 2 && x.startsWith(w));
    let best = lw.includes(w) ? 5 : lw.some(pre) ? 3.5 : 0;
    if (best) labelHit = true;
    if (best < 4.5) {
      const k = kws.find((x) => words(x).includes(w)) ?? kws.find((x) => words(x).some(pre));
      if (k) {
        const v = words(k).includes(w) ? 4.5 : 3;
        if (v > best) {
          best = v;
          kwHit ||= k;
        }
      }
    }
    if (best < 2 && sw.some(pre)) best = 2;
    if (best < 1.5 && hw.some(pre)) best = 1.5;
    if (best < 1 && pw.some(pre)) best = 1;
    if (!best && w.length >= 4 && [e.label, e.crumb, ...kws].some((x) => fuzzyScore(w, x) !== null)) {
      best = 0.5;
      fuzzy = true;
    }
    if (!best) return null;
    top = Math.max(top, best);
    total += best;
  }
  if (lw.length && ` ${lw.join(' ')} `.includes(` ${phrase} `)) total += 4;
  if (kws.includes(phrase)) {
    total += 5; // a seeded keyword equal to the query is its answer
    kwHit = phrase;
  }
  const why = fuzzy
    ? t('close match')
    : !labelHit && kwHit
      ? t('matches “{keyword}”', { keyword: kwHit })
      : top === 1.5
        ? t('match in description')
        : '';
  return { score: total, why };
}

/** Hits for `query`, best first; Advanced keys after the rest. */
export function searchSettings(index: readonly SearchEntry[], query: string): SearchHit[] {
  const qs = words(query);
  if (qs.length === 0) return [];
  const phrase = qs.join(' ');
  const hits: (SearchHit & { i: number })[] = [];
  index.forEach((e, i) => {
    const m = score(e, qs, phrase);
    if (m) hits.push({ ...e, ...m, i });
  });
  hits.sort(
    (a, b) =>
      Number(!!a.advanced) - Number(!!b.advanced) ||
      b.score - a.score ||
      KIND_ORDER[a.kind] - KIND_ORDER[b.kind] ||
      a.i - b.i,
  );
  return hits;
}
