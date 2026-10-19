// Vitest setup (jsdom). Every test starts with the in-memory mock transport installed.
import { cleanup, configure } from '@testing-library/svelte';
import { afterEach, beforeEach } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

// jsdom has no ResizeObserver (Svelte's bind:clientHeight uses it).
if (typeof globalThis.ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  } as unknown as typeof ResizeObserver;
}

// jsdom has no Web Animations: Svelte's animate: directive asks a leaving row for its animations.
Element.prototype.getAnimations ??= () => [];

// Every waitFor/findBy defaults to 1 s: one slow render under load (other lanes building) fails a test
// (#529, #534). Kept below vite.config.ts's 30 s testTimeout, so a real hang still fails.
configure({ asyncUtilTimeout: 8000 });

beforeEach(() => {
  setTransport(createMockTransport().transport);
});

// No vitest globals, so testing-library registers no auto-cleanup: unmount here, or every test's
// components stay mounted and keep reacting to the shared stores (a later test then waits seconds).
afterEach(() => {
  cleanup();
  document.body.innerHTML = '';
});
