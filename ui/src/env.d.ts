/// <reference types="svelte" />
/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** `mock` selects the in-memory IPC mock; anything else uses Tauri IPC. */
  readonly VITE_IPC?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

interface Window {
  /** Mock IPC controls (VITE_IPC=mock only). */
  __keltaMock?: import('$lib/ipc/mock').MockControls;
  /** Debug handles for e2e tests (VITE_IPC=mock only). */
  __kelta?: {
    stores: typeof import('$lib/stores');
    registry: typeof import('./app/registry');
    actions: typeof import('$lib/actions');
  };
}
