import { afterEach, describe, expect, it, vi } from 'vitest';

import { fadeMove, motion, resolveMotion, setMotion } from './motion';

describe('motion (#398)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    setMotion('on');
  });

  it('auto is on, reduced under safe graphics; an explicit choice wins', () => {
    expect(resolveMotion('auto', false)).toBe('on');
    expect(resolveMotion(undefined, false)).toBe('on');
    expect(resolveMotion('auto', true)).toBe('reduced');
    expect(resolveMotion('on', true)).toBe('on');
    expect(resolveMotion('off', false)).toBe('off');
  });

  it('writes the level on <html> for the CSS side', () => {
    setMotion('reduced');
    expect(document.documentElement.dataset.motion).toBe('reduced');
  });

  it('the OS reduce-motion preference turns everything off', () => {
    const proto = Element.prototype as { animate?: unknown };
    const had = 'animate' in proto;
    proto.animate ??= () => {};
    vi.stubGlobal('matchMedia', (q: string) => ({ matches: q.includes('reduce') }));
    setMotion('on');
    expect(motion()).toBe('off');
    expect(fadeMove(document.body).duration).toBe(0);
    vi.stubGlobal('matchMedia', () => ({ matches: false }));
    expect(motion()).toBe('on');
    setMotion('reduced');
    expect(fadeMove(document.body, { y: 8 }).css?.(0.5, 0.5)).toBe('opacity: 0.5; translate: 0px 0px');
    if (!had) delete proto.animate;
  });
});
