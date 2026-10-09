// IPC entry point: typed commands, the UiEvent channel and the transport switch.

export * from './commands';
export * from './events';
export { IpcError, getTransport, hasTransport, isIpcError, setTransport, toIpcError } from './transport';
export type { IpcChannel, IpcTransport } from './transport';
