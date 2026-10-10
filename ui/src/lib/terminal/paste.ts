// Paste helpers (SPEC §4): bracketed-paste marker stripping and the multi-line confirmation rule.

/** Removes bracketed-paste markers from pasted text (paste injection). */
export function sanitizePaste(text: string): string {
  // Repeat until stable: removing one marker can join the pieces of a nested one.
  let prev;
  do {
    prev = text;
    // eslint-disable-next-line no-control-regex
    text = text.replace(/\x1b\[20[01]~/g, '');
  } while (text !== prev);
  return text;
}

/**
 * Multi-line paste into a prompt without bracketed paste asks for confirmation
 * (`terminal.confirm_multiline_paste`). A single line with a trailing newline does not count.
 */
export function needsPasteConfirmation(text: string, bracketedPaste: boolean, enabled: boolean): boolean {
  if (!enabled || bracketedPaste) return false;
  return /[\r\n]/.test(text.replace(/[\r\n]+$/, ''));
}
