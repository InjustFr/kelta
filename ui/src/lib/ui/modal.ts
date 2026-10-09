// Keyboard rules shared by every Dialog and Sheet (FLOW §7.1): focus starts on the main input,
// Tab / ⇧Tab stay inside, ⌘↵ (Ctrl+↵) runs the primary footer button, Esc cancels, and focus
// returns where it was on close. Space toggles checkboxes and switches natively.

const FOCUSABLE = 'button, input, textarea, select, [tabindex="0"]';
/** Tried in order: the first selector with a match wins (document order inside one selector). */
const MAIN = [
  '[autofocus]',
  'input:not(:disabled), textarea:not(:disabled), select:not(:disabled)',
  'footer .k-button.primary:not(:disabled), footer .k-button.danger:not(:disabled)',
];

/** Focuses the main input of a modal; returns the cleanup that restores the previous focus. */
export function focusModal(el: HTMLElement): () => void {
  const previous = document.activeElement;
  const main = MAIN.map((s) => el.querySelector<HTMLElement>(s)).find((x) => x !== null);
  (main ?? el).focus();
  return () => {
    if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
  };
}

export function modalKeydown(e: KeyboardEvent, el: HTMLElement | undefined, onclose: () => void): void {
  if (e.key === 'Escape') {
    e.stopPropagation();
    onclose();
    return;
  }
  if (!el) return;
  // A form handling ⌘↵ itself (StartWorkSheet) has already prevented the default.
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && !e.defaultPrevented) {
    const primary =
      el.querySelector<HTMLButtonElement>('footer .k-button.primary:not(:disabled)') ??
      el.querySelector<HTMLButtonElement>('footer .k-button.danger:not(:disabled)');
    if (primary) {
      e.preventDefault();
      e.stopPropagation();
      primary.click();
    }
    return;
  }
  if (e.key !== 'Tab') return;
  const focusables = [...el.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (f) => !f.hasAttribute('disabled'),
  );
  const first = focusables[0];
  const last = focusables[focusables.length - 1];
  if (!first || !last) return;
  if (e.shiftKey && (document.activeElement === first || document.activeElement === el)) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
}
