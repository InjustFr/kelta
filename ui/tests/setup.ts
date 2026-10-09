// Vitest setup (jsdom). Every test starts with the in-memory mock transport installed.
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

beforeEach(() => {
  setTransport(createMockTransport().transport);
});

afterEach(() => {
  document.body.innerHTML = '';
});
