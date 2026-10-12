import { t, tn } from '$lib/i18n/index.svelte';
import { tags } from '$lib/stores';

/** One filter narrowing a list (#370): its chip label and how to turn it off. */
export interface ActiveFilter {
  id: string;
  label: string;
  clear: () => void;
}

/** The `/` text and the toolbar tag chip, the filters the views share; `null` tag = the view has none. */
export function commonFilters(
  text: string,
  clearText: () => void,
  tag: string | null = null,
  clearTag: () => void = () => {},
): ActiveFilter[] {
  const q = text.trim();
  const live = tags.live(tag);
  return [
    ...(q ? [{ id: 'text', label: `"${q}"`, clear: clearText }] : []),
    ...(live ? [{ id: 'tag', label: t('Tag: {name}', { name: live }), clear: clearTag }] : []),
  ];
}

/** The filtered-empty title, naming every filter on. */
export function noMatch(active: readonly ActiveFilter[]): string {
  return tn('No match: {n} filter on ({list}).', 'No match: {n} filters on ({list}).', active.length, {
    list: active.map((f) => f.label).join(', '),
  });
}

export function clearAll(active: readonly ActiveFilter[]): void {
  for (const f of active) f.clear();
}
