// Key manager (SPEC §4, ARCHITECTURE §9.4). L4's keybinding editor imports `parseChord` and
// `findConflicts`; the shell starts the `keyManager` singleton, terminal views route their key
// events through it.

import { dispatch, hasAction } from '$lib/actions';
import { settings, toasts, tools } from '$lib/stores';

import { KeyManager, type ExtraBinding } from './manager';

export {
  chordFromEvent,
  chordToString,
  codeToKeyName,
  findConflicts,
  isReservedChord,
  keyNameToCode,
  matchChord,
  parseChord,
  type Chord,
  type Conflict,
  type Platform,
} from './chords';
export {
  contextFromEvent,
  effectiveBindings,
  effectiveChords,
  isEditable,
  isWebviewDefault,
  KeyManager,
  type ExtraBinding,
  type KeyContext,
  type KeyManagerOptions,
  type KeyResult,
} from './manager';

/** Tool keybindings (`ToolInfo.keybinding`) open the tool through the `tools.open` action. */
function toolBindings(): ExtraBinding[] {
  const out: ExtraBinding[] = [];
  const seen = new Set<string>();
  for (const slot of Object.values(tools.byProject)) {
    for (const tool of slot.data ?? []) {
      if (!tool.keybinding || seen.has(tool.id)) continue;
      seen.add(tool.id);
      out.push({ action: 'tools.open', chords: [tool.keybinding], args: { tool_id: tool.id } });
    }
  }
  return out;
}

/** The window-wide key manager (started by the shell). */
export const keyManager = new KeyManager({
  keys: () => settings.value()?.keys ?? null,
  dispatch,
  hasAction,
  extraBindings: toolBindings,
  onError: (err, id) => toasts.error(err, `Action ${id} failed`),
});
