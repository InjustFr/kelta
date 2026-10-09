// The single requestAnimationFrame scheduler of the UI (BUILD_PLAN §1.5: the only module allowed to
// call rAF besides the one-shot first-paint mark in main.ts). Work is queued by key; a frame is
// requested only when the queue goes from empty to non-empty, so an idle UI owns no frame callbacks.
//
// Used for: ack batching (one `session_ack` per frame and session), fit/resize coalescing, the
// project switch mark and gutter drags.

export type FrameTask = () => void;

export interface FrameDriver {
  request(callback: () => void): unknown;
  cancel(handle: unknown): void;
}

/** rAF when the page is visible, a one-shot timeout otherwise (rAF is paused in hidden pages). */
export const browserDriver: FrameDriver = {
  request(callback) {
    if (
      typeof requestAnimationFrame === 'function' &&
      !(typeof document !== 'undefined' && document.hidden)
    ) {
      return { raf: requestAnimationFrame(callback) };
    }
    // one-shot: rAF does not fire in hidden pages and acks must not starve
    return { timeout: setTimeout(callback, 16) };
  },
  cancel(handle) {
    const h = handle as { raf?: number; timeout?: ReturnType<typeof setTimeout> };
    if (h.raf !== undefined && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(h.raf);
    if (h.timeout !== undefined) clearTimeout(h.timeout);
  },
};

export class FrameScheduler {
  #driver: FrameDriver;
  #tasks = new Map<unknown, FrameTask>();
  #handle: unknown = null;
  /** Frames that actually ran (tests, perf HUD). */
  frames = 0;

  constructor(driver: FrameDriver = browserDriver) {
    this.#driver = driver;
  }

  /** Queues `task` for the next frame. A later `schedule` with the same key replaces the task. */
  schedule(key: unknown, task: FrameTask): void {
    this.#tasks.set(key, task);
    if (this.#handle === null) this.#handle = this.#driver.request(() => this.#run());
  }

  cancel(key: unknown): void {
    this.#tasks.delete(key);
    if (this.#tasks.size === 0 && this.#handle !== null) {
      this.#driver.cancel(this.#handle);
      this.#handle = null;
    }
  }

  /** Number of queued tasks. */
  get pending(): number {
    return this.#tasks.size;
  }

  /** True while a frame callback is armed (must be false at idle). */
  get armed(): boolean {
    return this.#handle !== null;
  }

  /** Runs the queued tasks now instead of at the next frame. */
  flush(): void {
    if (this.#handle !== null) {
      this.#driver.cancel(this.#handle);
      this.#handle = null;
    }
    this.#run();
  }

  #run(): void {
    this.#handle = null;
    this.frames += 1;
    const batch = [...this.#tasks.values()];
    this.#tasks.clear();
    for (const task of batch) {
      try {
        task();
      } catch (err) {
        console.error('[kelta] frame task failed', err);
      }
    }
    // Tasks scheduled while running were queued into `#tasks`; a frame was requested for them.
  }
}

export const frames = new FrameScheduler();

/**
 * Measures the frame rate over `durationMs` (renderer probe, ARCHITECTURE §9.2). The only recursive
 * rAF loop of the UI: it runs for a bounded time and only when the probe is started.
 */
export function measureFps(durationMs: number, onFrame?: (index: number) => void): Promise<number> {
  return new Promise((resolve) => {
    let count = 0;
    let start = 0;
    const step = (now: number): void => {
      if (start === 0) start = now;
      count += 1;
      onFrame?.(count);
      if (now - start >= durationMs) {
        resolve((count * 1000) / Math.max(1, now - start));
        return;
      }
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
}
