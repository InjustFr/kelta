// Plugins and triggers lists, plus relay of `plugin.event` UiEvents to plugin screen hosts.

import type { JsonValue, PluginInfo, ScreenInstanceId, TriggerInfo, UiEvent } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { idle, settle, type Loadable } from './loadable';

export type PluginEventListener = (name: string, payload: JsonValue) => void;

export class PluginsStore {
  plugins = $state<Loadable<PluginInfo[]>>(idle());
  triggers = $state<Loadable<TriggerInfo[]>>(idle());

  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive listener list
  #screenListeners = new Map<ScreenInstanceId, Set<PluginEventListener>>();

  async load(): Promise<void> {
    this.plugins = { ...this.plugins, loading: true };
    this.plugins = await settle(this.plugins, () => ipc.pluginList());
  }

  async loadTriggers(): Promise<void> {
    this.triggers = { ...this.triggers, loading: true };
    this.triggers = await settle(this.triggers, () => ipc.triggerList({ project_id: null }));
  }

  /** Subscribes a plugin screen host to its relayed bus events. Returns an unsubscribe fn. */
  onScreenEvent(instanceId: ScreenInstanceId, listener: PluginEventListener): () => void {
    let set = this.#screenListeners.get(instanceId);
    if (!set) {
      // eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive listener list
      set = new Set();
      this.#screenListeners.set(instanceId, set);
    }
    set.add(listener);
    return () => {
      set.delete(listener);
      if (set.size === 0) this.#screenListeners.delete(instanceId);
    };
  }

  apply(ev: UiEvent): void {
    if (ev.type === 'plugin.event') {
      for (const l of [...(this.#screenListeners.get(ev.instance_id) ?? [])]) l(ev.name, ev.payload);
    } else if (ev.type === 'settings.changed') {
      if (this.plugins.fetchedAt !== null) void this.load();
      if (this.triggers.fetchedAt !== null) void this.loadTriggers();
    }
  }
}
