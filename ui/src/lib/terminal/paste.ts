// Paste helpers (SPEC §4): bracketed-paste marker stripping, control-byte replacement and the multi-line confirmation rule.

/** Removes bracketed-paste markers and neutralises control bytes in pasted text (paste injection). */
export function sanitizePaste(text: string): string {
  // One linear pass: drop a marker as soon as the output ends with one, so nested markers
  // (removal joins the pieces of another) go too, without a quadratic re-scan per level.
  const out: string[] = [];
  for (const ch of text) {
    out.push(ch);
    const tail = out.slice(-6).join('');
    if (tail === '\x1b[200~' || tail === '\x1b[201~') out.length -= 6;
  }
  // C0 controls (except tab, LF, CR), DEL and C1 controls (U+009B is a one-character CSI) become spaces so a pasted ESC or ^C cannot drive the program (CVE-2026-26982).
  // eslint-disable-next-line no-control-regex -- matching control bytes is the point
  return out.join('').replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]/g, ' ');
}

/**
 * Multi-line paste into a prompt without bracketed paste asks for confirmation
 * (`terminal.confirm_multiline_paste`). A single line with a trailing newline does not count.
 */
export function needsPasteConfirmation(text: string, bracketedPaste: boolean, enabled: boolean): boolean {
  if (!enabled || bracketedPaste) return false;
  return /[\r\n]/.test(text.replace(/[\r\n]+$/, ''));
}
