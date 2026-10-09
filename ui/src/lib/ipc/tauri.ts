// Real transport over `@tauri-apps/api/core` (invoke + Channel).

import { Channel, invoke } from '@tauri-apps/api/core';

import type { IpcChannel, IpcTransport } from './transport';

export function createTauriTransport(): IpcTransport {
  return {
    kind: 'tauri',
    invoke<T>(
      cmd: string,
      args?: Record<string, unknown> | Uint8Array,
      options?: { headers?: Record<string, string> },
    ) {
      return invoke<T>(cmd, args, options?.headers ? { headers: options.headers } : undefined);
    },
    channel<T>(onmessage: (message: T) => void): IpcChannel<T> {
      const ch = new Channel<T>();
      const wrapper: IpcChannel<T> = {
        handle: ch,
        onmessage,
      };
      ch.onmessage = (message: T) => wrapper.onmessage(message);
      return wrapper;
    },
  };
}
