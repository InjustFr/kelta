// Entry point: selects the IPC transport (VITE_IPC=mock → in-memory mock), mounts the shell and
// loads the startup data. Keep this file small: it is part of the initial bundle.

import './styles/tokens.css';
import './styles/base.css';

import { mount } from 'svelte';

import * as actions from '$lib/actions';
import { appReady } from '$lib/ipc/commands';
import { createTauriTransport } from '$lib/ipc/tauri';
import { setTransport } from '$lib/ipc/transport';
import * as stores from '$lib/stores';
import { injectSprite } from '$lib/ui/icons';

import * as registry from './app/registry';
import Shell from './shell/Shell.svelte';

// Action handlers of every lane (BUILD_PLAN §2.4): ui/src/shell/actions.ts + ui/src/views/*/actions.ts.
import.meta.glob(['./shell/actions.ts', './views/*/actions.ts'], { eager: true });

async function start(): Promise<void> {
  if (import.meta.env.VITE_IPC === 'mock') {
    const { createMockTransport } = await import('$lib/ipc/mock');
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    window.__keltaMock = controls;
    window.__kelta = { stores, registry, actions };
  } else {
    setTransport(createTauriTransport());
  }

  injectSprite();
  const target = document.getElementById('app');
  if (!target) throw new Error('#app missing');
  mount(Shell, { target });
  void stores.bootstrap();

  // First frame after mount: clears the launch crash guard (ARCH §12.4) and marks cold start.
  requestAnimationFrame(() => {
    appReady({ t_ms: performance.now() }).catch(() => {});
  });
}

void start();
