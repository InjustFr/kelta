// Motion that explains (#398, DESIGN §8): the Svelte side of the motion tokens. CSS keyframes read
// `--k-shift` / `--k-pulses` from <html data-motion>; Svelte transitions (Web Animations, which the
// CSS reduced-motion rules do not reach) ask `motion()` here instead.

import { flip, type AnimationConfig } from 'svelte/animate';
import { cubicOut } from 'svelte/easing';
import type { TransitionConfig } from 'svelte/transition';

import type { Animations } from '$lib/gen';

export type Motion = 'on' | 'reduced' | 'off';

/** Mirrors --k-duration-fast / --k-duration (ms); cubicOut is --k-ease. */
export const DURATION = { fast: 90, base: 150 } as const;

let chosen: Motion = 'on';

/** `app.animations`: auto is on, reduced in safe graphics mode (a slow or broken GPU path). */
export function resolveMotion(setting: Animations | null | undefined, safeGraphics: boolean): Motion {
  if (setting === 'on' || setting === 'reduced' || setting === 'off') return setting;
  return safeGraphics ? 'reduced' : 'on';
}

/** Applies the level to <html data-motion> (the CSS side) and to the transitions below. */
export function setMotion(level: Motion): void {
  chosen = level;
  if (typeof document !== 'undefined') document.documentElement.dataset.motion = level;
}

/** The level now: the OS "reduce motion" preference, or a DOM without Web Animations (jsdom), is off. */
export function motion(): Motion {
  if (typeof matchMedia !== 'function' || typeof Element === 'undefined' || !('animate' in Element.prototype))
    return 'off';
  return matchMedia('(prefers-reduced-motion: reduce)').matches ? 'off' : chosen;
}

/**
 * In/out transition of floating surfaces, toasts and rows: fades, and moves by `x`/`y` px unless reduced.
 * `translate`, not `transform`, so it composes with a surface's own centring transform.
 */
export function fadeMove(
  _node: Element,
  { x = 0, y = 0 }: { x?: number; y?: number } = {},
): TransitionConfig {
  const m = motion();
  if (m === 'off') return { duration: 0 };
  const k = m === 'on' ? 1 : 0;
  return {
    duration: DURATION.fast,
    easing: cubicOut,
    css: (t, u) => `opacity: ${t}; translate: ${u * x * k}px ${u * y * k}px`,
  };
}

/** FLIP for keyed lists (`animate:glide`): a row that moves slides to its new place; never when reduced. */
export function glide(node: Element, rects: { from: DOMRect; to: DOMRect }): AnimationConfig {
  if (motion() !== 'on') return { duration: 0 };
  return flip(node, rects, { duration: DURATION.base, easing: cubicOut });
}
