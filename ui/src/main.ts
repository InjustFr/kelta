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
import { setLocale } from '$lib/i18n/index.svelte';
import { terminalPool, type TerminalView } from '$lib/terminal';
import { shellEscape } from '$lib/terminal/drop';
import { injectSprite } from '$lib/ui/icons';

import * as registry from './app/registry';
import Shell from './shell/Shell.svelte';
import { offerOnboarding } from './views/onboarding/firstRun';

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
    // Linux only: macOS keeps the native drop handler off (#480), so this never fires there.
    // Dropped files paste their escaped paths into the terminal under the cursor (#524).
    void import('@tauri-apps/api/webview').then(({ getCurrentWebview }) =>
      getCurrentWebview().onDragDropEvent(({ payload: p }) => {
        if (p.type !== 'drop') return;
        // wry's GTK handler passes the widget's logical x,y through unscaled (typed as physical), so no / devicePixelRatio.
        const host = document
          .elementFromPoint(p.position.x, p.position.y)
          ?.closest<HTMLElement>('[data-terminal]');
        const id = host?.dataset.session;
        if (id) void terminalPool.get<TerminalView>(id)?.pasteText(p.paths.map(shellEscape).join(' ') + ' ');
      }),
    );
  }
  // shortcut: best effort on reload / Cmd+Q (app.exit does not wait for the page), upgrade by routing quit through the UI.
  window.addEventListener('beforeunload', () => void stores.layout.flush());

  // On macOS the webview gets Finder files and dragged links (no native drag-drop handler, #480): one that
  // no target takes would navigate the page away. In-app drags carry only text/plain and stay untouched.
  const external = (e: DragEvent): void => {
    if (e.dataTransfer?.types.some((t) => t === 'Files' || t === 'text/uri-list')) e.preventDefault();
  };
  window.addEventListener('dragover', external);
  window.addEventListener('drop', external);

  injectSprite();
  await setLocale(null); // the OS language from the first frame; settings may override it once loaded
  const target = document.getElementById('app');
  if (!target) throw new Error('#app missing');
  mount(Shell, { target });
  void stores.bootstrap().then(offerOnboarding);

  // First frame after mount: clears the launch crash guard (ARCH §12.4) and marks cold start (the
  // main window's, not a detached one's).
  if (!window.__KELTA_DETACHED__) {
    requestAnimationFrame(() => {
      appReady({ t_ms: performance.now() }).catch(() => {});
    });
  }
}

void start();
