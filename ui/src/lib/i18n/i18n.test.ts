// The catalog check (#344): every message the source passes to `t` / `tn` / `msg`, every settings label
// and description the schema generates, and every action name must be translated in fr.json and de.json,
// with the same `{placeholders}`. Keys no longer used fail too, so the catalogs stay in step.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { ACTIONS } from '$lib/gen/actions';
import settingsSchema from '../../../../schema/settings.schema.json';

import {
  LANGUAGE_NAMES,
  descriptionOf,
  enumOptions,
  titleOf,
  type SchemaNode,
} from '../../views/settings/lib/schema';
import de from './de.json';
import fr from './fr.json';
import { setLocale, t, tn, type Catalog } from './index.svelte';

const CATALOGS: Record<string, Catalog> = { fr, de };

const SRC = join(import.meta.dirname, '../..');
const LIT = String.raw`('(?:[^'\\\n]|\\.)*'|"(?:[^"\\\n]|\\.)*")`;
const CALL = String.raw`(?<![\w$.])`;
const T_RE = new RegExp(`${CALL}(?:t|msg)\\(\\s*${LIT}`, 'g');
const TN_RE = new RegExp(`${CALL}tn\\(\\s*${LIT}\\s*,\\s*${LIT}`, 'g');
const TEMPLATE_RE = new RegExp(`${CALL}(?:t|tn|msg)\\(\\s*\``, 'g');

function unquote(lit: string): string {
  return lit.slice(1, -1).replace(/\\(.)/g, (_, c: string) => ({ n: '\n', t: '\t' })[c] ?? c);
}

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return ['gen', 'i18n'].includes(e.name) ? [] : sources(p);
    const test = /\.test\.(ts|svelte)$/.test(e.name) || e.name === 'mock.ts';
    return /\.(ts|svelte)$/.test(e.name) && !test ? [p] : [];
  });
}

function schemaStrings(node: unknown, out: Set<string>): void {
  if (Array.isArray(node)) return node.forEach((n) => schemaStrings(n, out));
  if (!node || typeof node !== 'object') return;
  const n = node as SchemaNode & Record<string, unknown>;
  for (const key of Object.keys(n.properties ?? {})) out.add(titleOf(key));
  if (typeof n.description === 'string' && n.description.trim()) out.add(descriptionOf(n));
  for (const o of enumOptions(n)) {
    if (!(o.value in LANGUAGE_NAMES)) out.add(o.label);
    if (o.description) out.add(o.description);
  }
  Object.values(n).forEach((v) => schemaStrings(v, out));
}

/** Every key the UI translates: `[key, plural]`. */
function usedKeys(): { keys: Map<string, boolean>; templates: string[] } {
  const keys = new Map<string, boolean>();
  const templates: string[] = [];
  for (const file of sources(SRC)) {
    const text = readFileSync(file, 'utf8');
    for (const m of text.matchAll(T_RE)) keys.set(unquote(m[1]!), keys.get(unquote(m[1]!)) ?? false);
    for (const m of text.matchAll(TN_RE)) keys.set(unquote(m[2]!), true);
    for (const _ of text.matchAll(TEMPLATE_RE)) templates.push(file.slice(SRC.length));
  }
  const generated = new Set<string>();
  schemaStrings(settingsSchema, generated);
  for (const a of ACTIONS) generated.add(a.label);
  for (const k of generated) if (!keys.has(k)) keys.set(k, false);
  return { keys, templates };
}

const placeholders = (s: string): string[] => [...new Set(s.match(/\{\w+\}/g) ?? [])].sort();

describe('i18n catalogs', () => {
  afterEach(() => setLocale('en'));
  const { keys, templates } = usedKeys();

  it('messages are literal strings, not template literals', () => {
    expect(templates).toEqual([]);
  });

  for (const [name, catalog] of Object.entries(CATALOGS)) {
    it(`${name}.json translates every key with the same placeholders`, () => {
      const problems: string[] = [];
      for (const [key, plural] of keys) {
        const v = catalog[key];
        if (v === undefined) problems.push(`missing: ${key}`);
        else if (plural !== (typeof v === 'object'))
          problems.push(`${plural ? 'not plural' : 'plural'}: ${key}`);
        else {
          // `tn` always passes `{n}`; a plural `one` form may spell the count out ("une session").
          const allowed = new Set([...placeholders(key), ...(plural ? ['{n}'] : [])]);
          const forms = typeof v === 'object' ? [v.one, v.other] : [v];
          for (const f of forms) {
            const extra = placeholders(f).filter((p) => !allowed.has(p));
            const lost =
              typeof v === 'object' && f === v.one ? [] : placeholders(key).filter((p) => !f.includes(p));
            if (extra.length || lost.length) problems.push(`placeholders ${[...extra, ...lost]}: ${key}`);
          }
        }
      }
      for (const key of Object.keys(catalog)) if (!keys.has(key)) problems.push(`unused: ${key}`);
      expect(problems).toEqual([]);
    });
  }

  it('switches language without a reload, with plurals and English fallback', async () => {
    await setLocale('fr');
    expect(t('Settings')).toBe(CATALOGS.fr['Settings']);
    expect(t('no such key {x}', { x: 1 })).toBe('no such key 1');
    expect(tn('{n} repo', '{n} repos', 1)).toBe(
      (CATALOGS.fr['{n} repos'] as { one: string }).one.replace('{n}', '1'),
    );
    await setLocale('de');
    expect(t('Settings')).toBe(CATALOGS.de['Settings']);
    await setLocale('en');
    expect(tn('{n} repo', '{n} repos', 2)).toBe('2 repos');
  });
});
