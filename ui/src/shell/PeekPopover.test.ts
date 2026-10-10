import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, sessions } from '$lib/stores';

import { HOVER_MS, peek, peekHover, peekWaiting } from './peek.svelte';
import PeekPopover from './PeekPopover.svelte';

const ASKING = '0199a6b2-0000-7000-8000-000000000005'; // billing Claude, needs_input
const DORMANT = '0199a6b2-0000-7000-8000-000000000011';
const MENU = ' Do you want to proceed?\n ❯ 1. Yes\n   2. No, and tell Claude what to do differently (esc)';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  const invoke = created.transport.invoke.bind(created.transport);
  // Real tails come from the Rust model; the mock's are plain lines.
  created.transport.invoke = (async (
    cmd: string,
    args?: Record<string, unknown> | Uint8Array,
    o?: unknown,
  ) =>
    cmd === 'session_text_tail' && (args as { id: string }).id === ASKING
      ? MENU
      : invoke(cmd, args, o as never)) as typeof created.transport.invoke;
  setTransport(created.transport);
  await Promise.all([projects.load(), sessions.load()]);
});

afterEach(() => peek.close(false));

const writes = (): string[] =>
  mock.calls
    .filter((c) => c.cmd === 'session_write')
    .map((c) => new TextDecoder().decode((c.args as { data: Uint8Array }).data));

describe('PeekPopover', () => {
  it('opens on the key with the tail, answers a menu with a raw digit and gives focus back', async () => {
    const pane = document.createElement('button');
    document.body.append(pane);
    pane.focus();
    render(PeekPopover);
    peekWaiting();
    flushSync();
    await waitFor(() => expect(screen.getByTestId('peek-menu').textContent).toContain('Yes'));
    expect(screen.getByTestId('peek-tail').textContent).toContain('Do you want to proceed?');
    expect(document.activeElement).toBe(screen.getByTestId('peek-reply'));

    await fireEvent.keyDown(screen.getByTestId('peek-reply'), { key: '1' });
    await waitFor(() => expect(writes()).toEqual(['1']));
    flushSync();
    expect(screen.queryByTestId('peek')).toBeNull();
    expect(document.activeElement).toBe(pane);
    pane.remove();
  });

  it('sends a typed reply as a bracketed paste then Enter', async () => {
    // No menu: the hook says waiting_user, so the numbered tail is not a menu.
    sessions.upsert({ ...sessions.get(ASKING)!, status: 'waiting_user' });
    render(PeekPopover);
    peek.open(ASKING, null, true);
    flushSync();
    const input = await screen.findByTestId('peek-reply');
    await fireEvent.input(input, { target: { value: 'use \x1bpostgres' } });
    await fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(writes()).toEqual(['\x1b[200~use postgres\x1b[201~', '\r']));
  });

  it('never sends a typed reply over a menu (the Enter would confirm option 1)', async () => {
    render(PeekPopover);
    peek.open(ASKING, null, true);
    flushSync();
    await waitFor(() => expect(screen.getByTestId('peek-menu')).toBeTruthy());
    const input = screen.getByTestId('peek-reply');
    await fireEvent.input(input, { target: { value: 'no, use the staging db' } });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(writes()).toEqual([]);
    expect(screen.getByTestId('peek')).toBeTruthy();
  });

  it('Esc closes; Tab moves to the next session needing input', async () => {
    sessions.upsert({ ...sessions.get(DORMANT)!, id: 'other', status: 'needs_input', lifecycle: 'live' });
    render(PeekPopover);
    peek.open(ASKING, null, true);
    flushSync();
    await fireEvent.keyDown(await screen.findByTestId('peek-reply'), { key: 'Tab' });
    flushSync();
    expect(screen.getByTestId('peek').dataset.sessionId).toBe('other');
    await fireEvent.keyDown(screen.getByTestId('peek-reply'), { key: 'Tab' });
    flushSync();
    expect(peek.id).toBe(ASKING);
    await fireEvent.keyDown(screen.getByTestId('peek-reply'), { key: 'Escape' });
    flushSync();
    expect(screen.queryByTestId('peek')).toBeNull();
  });

  it('a dormant session shows its tail and Resume, without a reply field', async () => {
    render(PeekPopover);
    peek.open(DORMANT, null, true);
    flushSync();
    await waitFor(() => expect(screen.getByTestId('peek-tail').textContent).not.toBe(''));
    expect(screen.getByTestId('peek-resume')).toBeTruthy();
    expect(screen.queryByTestId('peek-reply')).toBeNull();
  });

  it('a hover over another anchor does not swap a popover that holds the keyboard', () => {
    vi.useFakeTimers();
    try {
      peek.open(ASKING, null, true);
      const anchor = document.createElement('div');
      const action = peekHover(anchor, () => DORMANT);
      anchor.dispatchEvent(new MouseEvent('mouseenter'));
      vi.advanceTimersByTime(HOVER_MS);
      expect(peek.id).toBe(ASKING);
      action.destroy();
    } finally {
      vi.useRealTimers();
    }
  });
});
