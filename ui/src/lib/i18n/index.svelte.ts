// Interface translations (#344). English is the source language: the key of a message is its English
// text, so `t('Close')` reads as plain English and falls back to it. `fr.json` / `de.json` map each key to
// its translation (a `{ one, other }` pair for `tn`). `i18n.test.ts` fails when a key used in the source
// is missing from either catalog. `{name}` placeholders are filled from `params`.

export const LOCALES = ['en', 'fr', 'de'] as const;
export type Locale = (typeof LOCALES)[number];
export type Plural = { one: string; other: string };
export type Catalog = Record<string, string | Plural>;
type Params = Record<string, string | number>;

// Lazy: a catalog weighs ~45 KB gz, kept out of the initial bundle (ARCHITECTURE §13).
const LOADERS: Record<Exclude<Locale, 'en'>, () => Promise<{ default: Catalog }>> = {
  fr: () => import('./fr.json'),
  de: () => import('./de.json'),
};

function isLocale(v: unknown): v is Locale {
  return LOCALES.includes(v as Locale);
}

/** The OS (webview) language when Kelta has it, else English. */
export function osLocale(): Locale {
  const lang = typeof navigator === 'undefined' ? 'en' : navigator.language.slice(0, 2).toLowerCase();
  return isLocale(lang) ? lang : 'en';
}

let current = $state<Locale>('en');
let catalog = $state.raw<Catalog>({});
let pending = 0;

/** The active language; reading it in a template or `$derived` re-renders on a change. */
export function locale(): Locale {
  return current;
}

/** Applies `app.language` (`system` or unset follows the OS) once its catalog is loaded; no restart. */
export async function setLocale(pref: string | null | undefined): Promise<void> {
  const next = isLocale(pref) ? pref : osLocale();
  const seq = ++pending;
  const loaded = next === 'en' ? {} : (await LOADERS[next]()).default;
  if (seq !== pending) return; // a later choice won
  current = next;
  catalog = loaded;
  if (typeof document !== 'undefined') document.documentElement.lang = next;
}

function fill(text: string, params?: Params): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m));
}

/** Translates an English message. A key with no translation (dynamic text) is shown as is. */
export function t(key: string, params?: Params): string {
  const v = catalog[key];
  return fill(typeof v === 'string' ? v : key, params);
}

/** Plural message: `one` / `other` English forms, `{n}` is the count. The catalog key is `other`. */
export function tn(one: string, other: string, n: number, params?: Params): string {
  const v = catalog[other];
  const forms = typeof v === 'object' ? v : { one, other };
  const form = new Intl.PluralRules(current).select(n) === 'one' ? forms.one : forms.other;
  return fill(form, { n, ...params });
}

/** Marks an English message kept as data (translated later with `t(value)`) so the catalog check sees it. */
export function msg<S extends string>(key: S): S {
  return key;
}

/** Date / time in the active language (`Intl.DateTimeFormat` options). */
export function formatDate(d: Date | number, opts: Intl.DateTimeFormatOptions): string {
  return new Intl.DateTimeFormat(current, opts).format(d);
}
