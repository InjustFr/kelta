// IPC transport: the real Tauri bridge or the in-memory mock (VITE_IPC=mock).
// `commands.ts` and `events.ts` only talk to the active transport, never to Tauri directly.

import type { KeltaError } from '$lib/gen';
import type { ErrorCode } from '$lib/gen';

/** A one-way message channel from Rust to the UI (Tauri `Channel<T>`). */
export interface IpcChannel<T> {
  /** Opaque value passed as a command argument (`channel` key). */
  readonly handle: unknown;
  onmessage: (message: T) => void;
}

export interface InvokeOptions {
  headers?: Record<string, string>;
}

export interface IpcTransport {
  readonly kind: 'tauri' | 'mock';
  /** Invoke a command. `args` is the single argument object (or raw bytes for binary commands). */
  invoke<T>(cmd: string, args?: Record<string, unknown> | Uint8Array, options?: InvokeOptions): Promise<T>;
  /** Create a channel that a command can stream into. */
  channel<T>(onmessage: (message: T) => void): IpcChannel<T>;
}

/** Error thrown by every IPC wrapper. Carries the `KeltaError` sent by Rust (or the mock). */
export class IpcError extends Error {
  readonly code: ErrorCode;
  readonly detail: KeltaError['detail'];
  readonly retryAfterMs: number | null;
  readonly command: string;

  constructor(command: string, error: KeltaError) {
    super(error.message);
    this.name = 'IpcError';
    this.command = command;
    this.code = error.code;
    this.detail = error.detail;
    this.retryAfterMs = error.retry_after_ms;
  }

  toKeltaError(): KeltaError {
    return { code: this.code, message: this.message, detail: this.detail, retry_after_ms: this.retryAfterMs };
  }
}

const ERROR_CODES: readonly ErrorCode[] = [
  'not_found',
  'invalid_argument',
  'conflict',
  'permission_denied',
  'needs_auth',
  'rate_limited',
  'network',
  'upstream',
  'timeout',
  'unsupported',
  'untrusted',
  'needs_fields',
  'dirty',
  'cancelled',
  'internal',
];

export function isKeltaErrorShape(value: unknown): value is KeltaError {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as Record<string, unknown>;
  return typeof v.message === 'string' && ERROR_CODES.includes(v.code as ErrorCode);
}

/** Normalizes anything a transport rejects with into an `IpcError`. */
export function toIpcError(command: string, raw: unknown): IpcError {
  if (raw instanceof IpcError) return raw;
  if (isKeltaErrorShape(raw)) {
    return new IpcError(command, {
      code: raw.code,
      message: raw.message,
      detail: raw.detail ?? null,
      retry_after_ms: raw.retry_after_ms ?? null,
    });
  }
  const message = raw instanceof Error ? raw.message : typeof raw === 'string' ? raw : 'unknown IPC error';
  return new IpcError(command, { code: 'internal', message, detail: null, retry_after_ms: null });
}

/** True if `err` is an `IpcError`, optionally with the given code. */
export function isIpcError(err: unknown, code?: ErrorCode): err is IpcError {
  return err instanceof IpcError && (code === undefined || err.code === code);
}

let active: IpcTransport | null = null;

export function setTransport(transport: IpcTransport): void {
  active = transport;
}

export function getTransport(): IpcTransport {
  if (!active) throw new Error('IPC transport not initialised (call initIpc() first)');
  return active;
}

export function hasTransport(): boolean {
  return active !== null;
}
