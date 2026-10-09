import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import ModalHarness from './ModalHarness.test.svelte';

// FLOW §7.1: the same keyboard rules for every Dialog and Sheet.
describe.each(['dialog', 'sheet'] as const)('%s keyboard', (kind) => {
  function mount() {
    const onsubmit = vi.fn();
    const onclose = vi.fn();
    render(ModalHarness, { props: { kind, onsubmit, onclose } });
    return { onsubmit, onclose, modal: screen.getByRole('dialog', { name: 'Ship' }) };
  }

  it('focuses the main input, Space toggles, ⌘↵ submits, Esc cancels', async () => {
    const { onsubmit, onclose, modal } = mount();
    expect(document.activeElement).toBe(screen.getByLabelText('Title'));
    const draft = screen.getByRole('switch', { name: 'Draft' });
    // Space toggles natively: switches are focusable checkbox inputs (jsdom has no key activation).
    expect(draft).toBeInstanceOf(HTMLInputElement);
    expect((draft as HTMLInputElement).type).toBe('checkbox');
    draft.focus();
    await fireEvent.click(draft);
    await fireEvent.keyDown(draft, { key: 'Enter', metaKey: true });
    expect(onsubmit).toHaveBeenCalledWith(true);
    await fireEvent.keyDown(modal, { key: 'Enter', ctrlKey: true });
    expect(onsubmit).toHaveBeenCalledTimes(2);
    await fireEvent.keyDown(modal, { key: 'Escape' });
    expect(onclose).toHaveBeenCalledOnce();
  });

  it('keeps Tab and ⇧Tab inside', async () => {
    const { modal } = mount();
    const ship = screen.getByRole('button', { name: 'Ship' });
    ship.focus();
    await fireEvent.keyDown(ship, { key: 'Tab' });
    expect(modal.contains(document.activeElement)).toBe(true);
    expect(document.activeElement).not.toBe(screen.getByRole('button', { name: 'outside' }));
    const first = document.activeElement as HTMLElement;
    await fireEvent.keyDown(first, { key: 'Tab', shiftKey: true });
    expect(document.activeElement).toBe(ship);
  });

  it('a form that handles ⌘↵ itself is not submitted twice', async () => {
    const { onsubmit, modal } = mount();
    const ev = new KeyboardEvent('keydown', { key: 'Enter', metaKey: true, bubbles: true, cancelable: true });
    ev.preventDefault();
    modal.dispatchEvent(ev);
    expect(onsubmit).not.toHaveBeenCalled();
  });
});
