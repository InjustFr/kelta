// Terminal channel frames (ARCHITECTURE §6.1) and ack batching.
//
// Frame = tag byte + payload. Data: raw PTY bytes → `term.write(bytes, () => ack(n))`. Snapshot:
// ANSI repaint → full reset then write, ack. Exit: i32 LE exit code (`-1` = signal). Keyboard: u8
// kitty keyboard flags (a Snapshot resets them to 0), not acknowledged.
// Acks are batched per animation frame (`session_ack` with the summed bytes).

import { FRAME_DATA, FRAME_EXIT, FRAME_KEYBOARD, FRAME_SNAPSHOT } from '$lib/gen/constants';

import { frames as sharedFrames, type FrameScheduler } from './raf';

export type DecodedFrame =
  | { kind: 'data'; bytes: Uint8Array }
  | { kind: 'snapshot'; bytes: Uint8Array }
  | { kind: 'exit'; code: number }
  | { kind: 'keyboard'; flags: number }
  | { kind: 'unknown'; tag: number };

export function decodeFrame(frame: Uint8Array): DecodedFrame {
  if (frame.length === 0) return { kind: 'unknown', tag: -1 };
  const tag = frame[0]!;
  switch (tag) {
    case FRAME_DATA:
      return { kind: 'data', bytes: frame.subarray(1) };
    case FRAME_SNAPSHOT:
      return { kind: 'snapshot', bytes: frame.subarray(1) };
    case FRAME_EXIT: {
      if (frame.length < 5) return { kind: 'exit', code: -1 };
      const view = new DataView(frame.buffer, frame.byteOffset + 1, 4);
      return { kind: 'exit', code: view.getInt32(0, true) };
    }
    case FRAME_KEYBOARD:
      return frame.length === 2 ? { kind: 'keyboard', flags: frame[1]! } : { kind: 'unknown', tag };
    default:
      return { kind: 'unknown', tag };
  }
}

/** Sums acknowledged bytes and sends one `session_ack` per animation frame. */
export class AckBatcher {
  #pending = 0;
  #generation: number | null = null;
  #send: (generation: number, bytes: number) => void;
  #scheduler: FrameScheduler;

  constructor(send: (generation: number, bytes: number) => void, scheduler: FrameScheduler = sharedFrames) {
    this.#send = send;
    this.#scheduler = scheduler;
  }

  /** Bytes not yet sent. */
  get pending(): number {
    return this.#pending;
  }

  /**
   * Generation of the current attachment. Acks that accumulate before the attach resolves are held
   * and sent once it is known; `reset()` (detach) drops acks of the previous attachment.
   */
  setGeneration(generation: number | null): void {
    this.#generation = generation;
    if (generation !== null && this.#pending > 0) this.#scheduler.schedule(this, () => this.flush());
  }

  add(bytes: number): void {
    if (bytes <= 0) return;
    this.#pending += bytes;
    this.#scheduler.schedule(this, () => this.flush());
  }

  flush(): void {
    if (this.#generation === null || this.#pending === 0) return;
    const bytes = this.#pending;
    this.#pending = 0;
    this.#send(this.#generation, bytes);
  }

  /** Drops pending acks (detach, generation change). */
  reset(): void {
    this.#pending = 0;
    this.#generation = null;
    this.#scheduler.cancel(this);
  }
}

/** The part of xterm the frame handler needs (also satisfied by `@xterm/headless`). */
export interface TermSink {
  write(data: Uint8Array | string, callback?: () => void): void;
}

export interface FrameHandlerOptions {
  term: TermSink;
  acks: Pick<AckBatcher, 'add'>;
  onExit?: (code: number) => void;
  onSnapshot?: () => void;
  onKeyboard?: (flags: number) => void;
  onData?: (bytes: number) => void;
}

/** Applies decoded frames to a terminal. Create one per attachment; `close()` ignores later frames. */
export class FrameHandler {
  #opts: FrameHandlerOptions;
  #closed = false;

  constructor(options: FrameHandlerOptions) {
    this.#opts = options;
  }

  close(): void {
    this.#closed = true;
  }

  handle(frame: Uint8Array): void {
    if (this.#closed) return;
    const decoded = decodeFrame(frame);
    const { term, acks } = this.#opts;
    switch (decoded.kind) {
      case 'data': {
        const n = decoded.bytes.length;
        if (n === 0) return;
        this.#opts.onData?.(n);
        term.write(decoded.bytes, () => {
          if (!this.#closed) acks.add(n);
        });
        return;
      }
      case 'snapshot': {
        // RIS goes through the write queue so it lands after earlier data and before the snapshot
        // (`term.reset()` would not drop writes that are still queued).
        term.write('\x1bc');
        this.#opts.onSnapshot?.();
        const n = decoded.bytes.length;
        term.write(decoded.bytes, () => {
          if (!this.#closed) acks.add(n);
        });
        return;
      }
      case 'exit':
        this.#opts.onExit?.(decoded.code);
        return;
      case 'keyboard':
        this.#opts.onKeyboard?.(decoded.flags);
        return;
      case 'unknown':
        return;
    }
  }
}
