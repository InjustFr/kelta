// Settings > Tools quick form: command-line splitting, tool ids and tool keybinding conflicts.
import { ACTIONS, RESERVED_CHORDS } from '$lib/gen/actions';
import { findConflicts } from '$lib/keys';
import { formatChord } from '$lib/ui/format';

import { slugify, uniqueId } from './accounts';

/** Shell-like split: whitespace separates, single/double quotes group, backslash escapes. Null on an unclosed quote. */
export function splitArgv(line: string): string[] | null {
  const out: string[] = [];
  let cur: string | null = null;
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i]!;
    if (quote === "'") {
      if (c === "'") quote = null;
      else cur = (cur ?? '') + c;
    } else if (c === '\\' && i + 1 < line.length && (quote === null || /["\\$`]/.test(line[i + 1]!))) {
      cur = (cur ?? '') + line[++i];
    } else if (quote === '"') {
      if (c === '"') quote = null;
      else cur = (cur ?? '') + c;
    } else if (c === '"' || c === "'") {
      quote = c;
      cur ??= '';
    } else if (/\s/.test(c)) {
      if (cur !== null) out.push(cur);
      cur = null;
    } else {
      cur = (cur ?? '') + c;
    }
  }
  if (quote) return null;
  if (cur !== null) out.push(cur);
  return out;
}

/** Slug of `name`, suffixed `-2`, `-3`… past the ids already taken. */
export function uniqueToolId(name: string, taken: readonly string[]): string {
  return uniqueId(slugify(name) || 'tool', taken);
}

/**
 * Why tool `selfId`'s `chord` will not fire, or null: an action (`bindings`, effective chords per
 * action), a reserved chord or another tool already owns it.
 */
export function toolKeyConflict(
  chord: string,
  selfId: string,
  bindings: Record<string, readonly string[]>,
  tools: readonly { id: string; label: string; keybinding?: string | null }[],
  platform: 'macos' | 'linux',
): string | null {
  const self = `tool:${selfId}`;
  const next: Record<string, readonly string[]> = { ...bindings };
  for (const t of tools) if (t.keybinding && t.id !== selfId) next[`tool:${t.id}`] = [t.keybinding];
  next[self] = [chord];
  const hit = findConflicts(next, RESERVED_CHORDS, { platform }).find((c) => c.actions.includes(self));
  if (!hit) return null;
  const keys = formatChord(chord, platform).join('');
  const other = hit.actions.find((a) => a !== self);
  if (!other) return `${keys} is reserved for terminal programs; this tool's key will not work`;
  const label = other.startsWith('tool:')
    ? tools.find((t) => `tool:${t.id}` === other)?.label
    : ACTIONS.find((a) => a.id === other)?.label;
  return `${keys} is already "${label ?? other}"; this tool's key will not work`;
}
