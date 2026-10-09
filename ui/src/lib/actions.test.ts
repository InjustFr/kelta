import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  actionMeta,
  clearActionsForTests,
  dispatch,
  hasAction,
  isBuiltinAction,
  registerAction,
} from './actions';

afterEach(() => clearActionsForTests());

describe('actions', () => {
  it('registers, dispatches and unregisters', async () => {
    const fn = vi.fn();
    const off = registerAction('palette.open', fn);
    expect(hasAction('palette.open')).toBe(true);
    expect(await dispatch('palette.open', { q: 1 })).toBe(true);
    expect(fn).toHaveBeenCalledWith({ q: 1 });
    off();
    expect(await dispatch('palette.open')).toBe(false);
  });

  it('routes plugin commands to the generic handler', async () => {
    const fn = vi.fn();
    registerAction('plugin.command.*', fn);
    expect(hasAction('plugin.command.hello.say')).toBe(true);
    expect(await dispatch('plugin.command.hello.say', { a: 1 })).toBe(true);
    expect(fn).toHaveBeenCalledWith({ a: 1, command_id: 'hello.say' });
  });

  it('knows the built-in catalog', () => {
    expect(isBuiltinAction('tab.next')).toBe(true);
    expect(isBuiltinAction('nope')).toBe(false);
    expect(actionMeta('palette.open')?.prefix).toBe(':');
  });
});
