// Entry point: selects the IPC transport (VITE_IPC=mock → in-memory mock), mounts the shell and
// loads the startup data. Keep this file small: it is part of the initial bundle.

import './styles/tokens.css';
import './styles/base.css';

import { mount } from 'svelte';

import * as actions from '$lib/actions';
import { appClose, appReady } from '$lib/ipc/commands';
import { setDetached } from '$lib/layout';
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
    const { transport, controls } = createMockTransport({ keepGlobalLayout: true });
    setTransport(transport);
    window.__keltaMock = controls;
    window.__kelta = { stores, registry, actions };
  } else {
    setTransport(createTauriTransport());
    // The webview is destroyed on close: save the debounced layout first, then let Rust decide
    // (quit question or close). Without preventDefault Tauri destroys the window right after.
    // Closing a detached window (#342) brings its tab back to the main window.
    const shown = window.__KELTA_DETACHED__;
    void import('@tauri-apps/api/window').then(({ getCurrentWindow }) =>
      getCurrentWindow().onCloseRequested(async (e) => {
        e.preventDefault();
        if (shown) stores.layout.update(shown.project_id, (l) => setDetached(l, shown.tab_id, false));
        await stores.layout.flushForClose();
        if (shown) await getCurrentWindow().destroy();
        else await appClose().catch(() => getCurrentWindow().destroy()); // never leave an unclosable window
      }),
    );
  }
  // shortcut: best effort on reload / Cmd+Q (app.exit does not wait for the page), upgrade by routing quit through the UI.
  window.addEventListener('beforeunload', () => void stores.layout.flush());

  injectSprite();
  const target = document.getElementById('app');
  if (!target) throw new Error('#app missing');
  mount(Shell, { target });
  void stores.bootstrap();

  // First frame after mount: clears the launch crash guard (ARCH §12.4) and marks cold start (the
  // main window's, not a detached one's).
  if (!window.__KELTA_DETACHED__) {
    requestAnimationFrame(() => {
      appReady({ t_ms: performance.now() }).catch(() => {});
    });
  }
}

void start();
