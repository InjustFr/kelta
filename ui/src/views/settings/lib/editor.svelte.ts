// State of the Settings pane: which layer is edited, the layer document, per-path errors, and the
// write operations. One instance per pane, shared with the sections through Svelte context.

import { getContext, setContext } from 'svelte';

import type { EffectiveSettings, JsonValue, Layer, LayerDoc } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { toIpcError } from '$lib/ipc/transport';
import { settings as store } from '$lib/stores';

import { getAt, splitPath } from './paths';
import { infoAt, repoAllows, type PathInfo, type SchemaNode } from './schema';

export type EditLayer = 'global' | 'project' | 'repo';

export interface Access {
  editable: boolean;
  /** Why the field is read-only at this layer. */
  reason: string | null;
}

const KEY = Symbol('kelta-settings-editor');

export interface EditorTarget {
  layer: EditLayer;
  projectId: string | null;
  repoId: string | null;
}

export class SettingsEditor {
  layer = $state<EditLayer>('global');
  projectId = $state<string | null>(null);
  repoId = $state<string | null>(null);
  doc = $state<LayerDoc | null>(null);
  errors = $state<Record<string, string>>({});
  saving = $state<Record<string, boolean>>({});
  loading = $state(false);
  loadError = $state<string | null>(null);

  constructor(target: Partial<EditorTarget> = {}) {
    this.layer = target.layer ?? 'global';
    this.projectId = target.projectId ?? null;
    this.repoId = target.repoId ?? null;
  }

  // ---- reads ------------------------------------------------------------------------------

  /** Project whose effective view is shown (global view when editing the global layer). */
  get viewProject(): string | null {
    return this.layer === 'global' ? null : this.projectId;
  }

  get schema(): SchemaNode | null {
    return (store.schema.data as unknown as SchemaNode | null) ?? null;
  }

  get effective(): EffectiveSettings | null {
    return store.effective[this.viewProject ?? '']?.data ?? null;
  }

  get ready(): boolean {
    return this.schema !== null && this.effective !== null;
  }

  valueOf(path: string): JsonValue | undefined {
    return getAt(this.effective?.value, splitPath(path));
  }

  sourceOf(path: string): Layer {
    return (this.effective?.sources[path] as Layer | undefined) ?? 'default';
  }

  layerValue(path: string): JsonValue | undefined {
    return getAt(this.doc?.value, splitPath(path));
  }

  /** The key is defined in the layer being edited (a reset has an effect). */
  isSetHere(path: string): boolean {
    return this.layerValue(path) !== undefined || this.sourceOf(path) === this.layer;
  }

  info(path: string): PathInfo | null {
    return this.schema ? infoAt(this.schema, splitPath(path)) : null;
  }

  access(path: string): Access {
    if (this.layer === 'global') return { editable: true, reason: null };
    if (!this.projectId) return { editable: false, reason: 'Select a project first' };
    if (this.layer === 'project') {
      const info = this.info(path);
      if (info && info.known && !info.scope.includes('project')) {
        return { editable: false, reason: 'Global only: this key cannot be set per project' };
      }
      return { editable: true, reason: null };
    }
    if (!repoAllows(path)) return { editable: false, reason: 'Not allowed in a repo-local config' };
    if (this.doc?.trusted === false) {
      return { editable: false, reason: 'Read-only until this repo config is trusted' };
    }
    return { editable: true, reason: null };
  }

  // ---- loading ----------------------------------------------------------------------------

  #args(): { layer: Layer; project_id: string | null; repo_id: string | null } {
    return {
      layer: this.layer,
      project_id: this.layer === 'global' ? null : this.projectId,
      repo_id: this.layer === 'repo' ? this.repoId : null,
    };
  }

  async load(): Promise<void> {
    this.loading = true;
    this.loadError = null;
    try {
      await store.loadSchema();
      await store.load(this.viewProject);
      await this.refreshDoc();
    } catch (err) {
      this.loadError = toIpcError('settings', err).message;
    } finally {
      this.loading = false;
    }
  }

  async refreshDoc(): Promise<void> {
    if (this.layer !== 'global' && !this.projectId) {
      this.doc = null;
      return;
    }
    try {
      this.doc = await ipc.settingsLayerGet(this.#args());
    } catch (err) {
      this.doc = null;
      this.loadError = toIpcError('settings_layer_get', err).message;
    }
  }

  /** Switch layer / project / repo and reload. */
  async target(t: Partial<EditorTarget>): Promise<void> {
    if (t.layer !== undefined) this.layer = t.layer;
    if (t.projectId !== undefined) this.projectId = t.projectId;
    if (t.repoId !== undefined) this.repoId = t.repoId;
    this.errors = {};
    await this.load();
  }

  // ---- writes -----------------------------------------------------------------------------

  #fail(path: string, err: unknown): void {
    this.errors = { ...this.errors, [path]: toIpcError('settings', err).message };
  }

  #clear(path: string): void {
    if (!(path in this.errors)) return;
    const next = { ...this.errors };
    delete next[path];
    this.errors = next;
  }

  async #run(path: string, op: () => Promise<EffectiveSettings>): Promise<boolean> {
    this.saving = { ...this.saving, [path]: true };
    try {
      const eff = await op();
      store.accept(this.viewProject, eff);
      this.#clear(path);
      await this.refreshDoc();
      return true;
    } catch (err) {
      this.#fail(path, err);
      return false;
    } finally {
      const next = { ...this.saving };
      delete next[path];
      this.saving = next;
    }
  }

  /** Write `value` at `path` in the edited layer. `null` resets the key. */
  async set(path: string, value: JsonValue): Promise<boolean> {
    const access = this.access(path);
    if (!access.editable) {
      this.#fail(path, access.reason ?? 'read-only');
      return false;
    }
    if (value === null) return this.reset(path);
    return this.#run(path, () => ipc.settingsSet({ ...this.#args(), path, value }));
  }

  async reset(path: string): Promise<boolean> {
    const access = this.access(path);
    if (!access.editable) {
      this.#fail(path, access.reason ?? 'read-only');
      return false;
    }
    return this.#run(path, () => ipc.settingsReset({ ...this.#args(), path }));
  }

  /** Raw document replacement (Edit TOML). Throws the IPC error (it carries file:line:col). */
  async writeRaw(text: string): Promise<void> {
    const eff = await ipc.settingsWriteRaw({ ...this.#args(), text });
    store.accept(this.viewProject, eff);
    await this.refreshDoc();
  }
}

export function provideEditor(editor: SettingsEditor): SettingsEditor {
  setContext(KEY, editor);
  return editor;
}

/** Editor from context, or a fresh one for a section rendered on its own. */
export function useEditor(fallback: () => Partial<EditorTarget> = () => ({})): SettingsEditor {
  const existing = getContext<SettingsEditor | undefined>(KEY);
  if (existing) return existing;
  const e = new SettingsEditor(fallback());
  void e.load();
  return e;
}
