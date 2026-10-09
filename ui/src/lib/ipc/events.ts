// UiEvent channel (ARCHITECTURE §6.2): one `events_subscribe` per window, fanned out locally.

import type { UiEvent } from '$lib/gen';

import { eventsSubscribe } from './commands';

export type UiEventType = UiEvent['type'];
export type UiEventOf<T extends UiEventType> = Extract<UiEvent, { type: T }>;

type AnyListener = (event: UiEvent) => void;

const listeners = new Set<AnyListener>();
let subscription: Promise<number> | null = null;

/** Delivers one event to every local listener. Listener errors are isolated and logged. */
export function deliverUiEvent(event: UiEvent): void {
  for (const listener of [...listeners]) {
    try {
      listener(event);
    } catch (err) {
      console.error(`[kelta] UiEvent listener failed for ${event.type}`, err);
    }
  }
}

/** Listens to every UiEvent. Returns an unsubscribe function. */
export function onAnyUiEvent(listener: AnyListener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Listens to one UiEvent type. Returns an unsubscribe function. */
export function onUiEvent<T extends UiEventType>(
  type: T,
  listener: (event: UiEventOf<T>) => void,
): () => void {
  return onAnyUiEvent((event) => {
    if (event.type === type) listener(event as UiEventOf<T>);
  });
}

/**
 * Opens the backend channel (idempotent). Resolves with the subscription id. On failure the next
 * call retries.
 */
export function connectUiEvents(): Promise<number> {
  if (!subscription) {
    subscription = eventsSubscribe(deliverUiEvent).then(
      (r) => r.sub_id,
      (err: unknown) => {
        subscription = null;
        throw err;
      },
    );
  }
  return subscription;
}

/** Test helper: drops every listener and the subscription. */
export function resetUiEventsForTests(): void {
  listeners.clear();
  subscription = null;
}
