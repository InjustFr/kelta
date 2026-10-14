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
        // The backend held one desktop notification; `calls` is the order it handled them in, and
        // turning it on is the slower one.
        const on = (args as { on: boolean }).on;
        return new Promise<T>((r) =>
          setTimeout(
            () => {
              calls.push(args);
              r((on ? [] : ['PR approved']) as T);
            },
            on ? 5 : 0,
          ),
        );
      },
    });
  });

  it('a quick on-off reaches the backend in order', async () => {
    const entering = focus.toggle();
    await focus.toggle();
    await entering;
    expect(calls).toEqual([{ on: true }, { on: false }]);
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
    // The held pill (#442): the toasts held here plus the desktop notifications the backend counts.
    expect(focus.held).toBe(1);
    toasts.apply({ type: 'focus.held', count: 2 });
    expect(focus.held).toBe(3);

    await dispatch('view.unfocus');
    expect(focus.on).toBe(false);
    expect(hasAction('view.unfocus')).toBe(false);
    expect(focus.held).toBe(0);
    expect(calls).toEqual([{ on: true }, { on: false }]);
    expect(toasts.list.at(-1)?.toast.text).toBe(
      'While you were focused: PR approved, SHOP-1 ready to review',
    );
    toasts.info('Later', { view: null });
    expect(toasts.list.at(-1)?.toast.text).toBe('Later');
  });
});
