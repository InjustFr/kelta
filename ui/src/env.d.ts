/// <reference types="svelte" />
/// <reference types="vite/client" />

declare module 'virtual:lucide-names' {
  /** Every icon name in `lucide-static` (vite.config.ts). */
  const names: string[];
  export default names;
}

interface ImportMetaEnv {
  /** `mock` selects the in-memory IPC mock; anything else uses Tauri IPC. */
  readonly VITE_IPC?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

interface Window {
  /** Set by Rust in a detached window (#342): the tab it shows. */
  __KELTA_DETACHED__?: import('./shell/detached').DetachedTab;
  /** Mock IPC controls (VITE_IPC=mock only). */
  __keltaMock?: import('$lib/ipc/mock').MockControls;
  /** Debug handles for e2e tests (VITE_IPC=mock only). */
  __kelta?: {
    stores: typeof import('$lib/stores');
    registry: typeof import('./app/registry');
    actions: typeof import('$lib/actions');
  };
}
