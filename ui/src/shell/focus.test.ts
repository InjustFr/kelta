import { beforeEach, describe, expect, it } from 'vitest';

import { dispatch, hasAction } from '$lib/actions';
import { createMockTransport } from '$lib/ipc/mock';
import { getTransport, setTransport } from '$lib/ipc/transport';
import { toasts } from '$lib/stores';

import './actions';
import { focus, heldSummary } from './focus.svelte';

describe('focus mode (#421)', () => {
  const calls: unknown[] = [];

  beforeEach(async () => {
    await focus.leave();
    toasts.clear();
    calls.length = 0;
    setTransport(createMockTransport().transport);
    const base = getTransport();
    setTransport({
      ...base,
      invoke: <T>(cmd: string, args?: Record<string, unknown> | Uint8Array) => {
        if (cmd !== 'notify_focus') return base.invoke<T>(cmd, args);
        calls.push(args);
        // The backend held one desktop notification.
        return Promise.resolve((args as { on: boolean }).on ? [] : ['PR approved']) as Promise<T>;
      },
    });
  });

  it('sums up what was held, counting repeats and capping the list', () => {
    expect(heldSummary([])).toBeNull();
    expect(heldSummary(['A ready to review', 'PR approved', 'PR approved'])).toBe(
      'While you were focused: A ready to review, 2× PR approved',
    );
    expect(heldSummary(['a', 'b', 'c', 'd', 'e', 'f'])).toBe('While you were focused: a, b, c, d +2 more');
  });

  it('holds background info toasts, tells the backend, and leaves with one summary toast', async () => {
    await dispatch('view.focus');
    expect(focus.on).toBe(true);
    expect(hasAction('view.unfocus')).toBe(true);
    toasts.info('SHOP-1 ready to review', { view: null });
    toasts.warn('Disk almost full');
    toasts.push({ level: 'info', text: 'Install?', action: { label: 'Install', command: 'x', args: null } });
    toasts.info('Copied', { view: 'project:p' });
    expect(toasts.list.map((e) => e.toast.text)).toEqual(['Disk almost full', 'Install?', 'Copied']);

    await dispatch('view.unfocus');
    expect(focus.on).toBe(false);
    expect(hasAction('view.unfocus')).toBe(false);
    expect(calls).toEqual([{ on: true }, { on: false }]);
    expect(toasts.list.at(-1)?.toast.text).toBe(
      'While you were focused: PR approved, SHOP-1 ready to review',
    );
    toasts.info('Later', { view: null });
    expect(toasts.list.at(-1)?.toast.text).toBe('Later');
  });
});
