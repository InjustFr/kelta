// Effective settings (global and per project), the JSON schema and account statuses.
// `settings.changed` reloads every loaded effective view; `account.status` updates the account map.

import type { AccountId, EffectiveSettings, JsonValue, Layer, ProjectId, Settings, UiEvent } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { idle, settle, type Loadable } from './loadable';
import { reduceAccounts, type AccountState } from './reducers';

const GLOBAL = '';

export interface SettingsChange {
  layers: Layer[];
  paths: string[];
  requiresRestart: string[];
}

export class SettingsStore {
  effective = $state<Record<string, Loadable<EffectiveSettings>>>({});
  schema = $state<Loadable<JsonValue>>(idle());
  accounts = $state<Record<AccountId, AccountState>>({});
  /** Paths changed since start that need a restart (Settings shows a marker). */
  pendingRestart = $state<string[]>([]);
  lastChange = $state<SettingsChange | null>(null);

  /** Typed effective settings (global when `projectId` is omitted), or null before load. */
  value(projectId?: ProjectId | null): Settings | null {
    const slot = this.effective[projectId ?? GLOBAL] ?? this.effective[GLOBAL];
    return (slot?.data?.value as unknown as Settings | undefined) ?? null;
  }

  sources(projectId?: ProjectId | null): Record<string, Layer> {
    return (this.effective[projectId ?? GLOBAL]?.data?.sources ?? {}) as Record<string, Layer>;
  }

  /** `app.theme` resolved against the OS preference. */
  get theme(): 'dark' | 'light' {
    const t = this.value()?.app.theme ?? 'system';
    if (t === 'dark' || t === 'light') return t;
    const dark = typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches;
    return dark ? 'dark' : 'light';
  }

  async load(projectId?: ProjectId | null): Promise<Loadable<EffectiveSettings>> {
    const key = projectId ?? GLOBAL;
    const prev = this.effective[key] ?? idle<EffectiveSettings>();
    this.effective = { ...this.effective, [key]: { ...prev, loading: true } };
    const next = await settle(prev, () => ipc.settingsEffective({ project_id: projectId ?? null }));
    this.effective = { ...this.effective, [key]: next };
    return next;
  }

  async loadSchema(): Promise<Loadable<JsonValue>> {
    if (this.schema.data) return this.schema;
    this.schema = { ...this.schema, loading: true };
    this.schema = await settle(this.schema, () => ipc.settingsSchema());
    return this.schema;
  }

  /** Stores a result of settings_set/reset/write_raw for the matching view. */
  accept(projectId: ProjectId | null, value: EffectiveSettings): void {
    const key = projectId ?? GLOBAL;
    this.effective = {
      ...this.effective,
      [key]: { ...idle<EffectiveSettings>(), data: value, fetchedAt: Date.now() },
    };
  }

  apply(ev: UiEvent): void {
    if (ev.type === 'account.status') {
      this.accounts = reduceAccounts(this.accounts, ev);
      return;
    }
    if (ev.type !== 'settings.changed') return;
    this.lastChange = { layers: ev.layers, paths: ev.paths, requiresRestart: ev.requires_restart };
    const add = ev.requires_restart.filter((p) => !this.pendingRestart.includes(p));
    if (add.length) this.pendingRestart = [...this.pendingRestart, ...add];
    for (const key of Object.keys(this.effective)) void this.load(key === GLOBAL ? null : key);
  }
}
